//! The arena: a flat floor of full blocks with optional full-height walls,
//! and on top of it, for some duels, terrain: steps, slabs, pillars,
//! builds, pits and drops into the void (`Terrain`).
//!
//! Movement collides against one box per block, found the way
//! `World.getCollidingBoundingBoxes` finds them. Merging blocks into larger
//! boxes gives the same answer almost always, but not when a player's box
//! overlaps a wall by one unit in the last place (which the float width of
//! the box does produce): then the next wall block along the way stops the
//! player, and the server calls it "moved wrongly". The crosshair ray walks
//! the block grid cell by cell like `World.rayTraceBlocks`.
use crate::aabb::{dist_sq, Aabb, V3};
use crate::mth::floor_d;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct World {
    pub floor_y: f64,
    pub half: f64,
    floor_i: i32,
    half_i: i32,
    top_i: i32,
    walls: bool,
    /// The top of the highest block on the floor: the floor itself when
    /// there is no terrain.
    terrain_top: f64,
    /// Single full cubes placed on top of the floor (parity scenarios).
    extra: HashSet<[i32; 3]>,
    /// Cobwebs placed the same way.
    extra_webs: HashSet<[i32; 3]>,
    /// Whether there is a cobweb anywhere, in the terrain or among those.
    has_webs: bool,
    /// What stands on the floor and what is dug out of it, inside the walls.
    terrain: Option<Arc<Terrain>>,
    /// Below this height a player has fallen out of the world.
    pub void_y: f64,
}

/// What a block of terrain is.
pub const AIR: u8 = 0;
pub const FULL: u8 = 1;
/// A bottom slab: a block with half the height.
pub const SLAB: u8 = 2;
/// A cobweb: nothing to collide with, a whole block to a ray (a throw
/// stops in it, the crosshair and the line of sight end on it), and an
/// entity whose box is in one moves a quarter as far sideways and a
/// twentieth as far up or down on its next move, with its motion gone.
pub const WEB: u8 = 3;

/// How far below the floor a pit can reach and how high terrain can stand.
pub const TERRAIN_BELOW: i32 = 6;
pub const TERRAIN_ABOVE: i32 = 8;
/// A player this far below the floor has fallen out of the world.
pub const VOID_DEPTH: f64 = 12.0;

/// The blocks of a box of the world, from `TERRAIN_BELOW` levels under the
/// floor to `TERRAIN_ABOVE` over it. Under the box a column is solid down
/// to the bottom of the world unless it is open to the void.
#[derive(Clone, Debug)]
pub struct Terrain {
    pub x0: i32,
    pub y0: i32,
    pub z0: i32,
    pub nx: i32,
    pub ny: i32,
    pub nz: i32,
    pub cells: Vec<u8>,
    pub void: Vec<bool>,
}

impl Terrain {
    /// The floor of the arena with nothing on it: solid below `floor_i`.
    pub fn flat(x0: i32, z0: i32, nx: i32, nz: i32, floor_i: i32) -> Terrain {
        let (y0, ny) = (floor_i - TERRAIN_BELOW, TERRAIN_BELOW + TERRAIN_ABOVE);
        let mut cells = vec![AIR; (nx * nz * ny) as usize];
        for x in 0..nx {
            for z in 0..nz {
                for y in 0..TERRAIN_BELOW {
                    cells[((x * nz + z) * ny + y) as usize] = FULL;
                }
            }
        }
        Terrain { x0, y0, z0, nx, ny, nz, cells, void: vec![false; (nx * nz) as usize] }
    }

    #[inline]
    fn column(&self, x: i32, z: i32) -> Option<usize> {
        let (i, k) = (x - self.x0, z - self.z0);
        if i < 0 || k < 0 || i >= self.nx || k >= self.nz {
            return None;
        }
        Some((i * self.nz + k) as usize)
    }

    /// The block at these coordinates, or None outside the box's columns
    /// or above it.
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        let c = self.column(x, z)?;
        let j = y - self.y0;
        if j < 0 {
            return Some(if self.void[c] { AIR } else { FULL });
        }
        if j >= self.ny {
            return None;
        }
        Some(self.cells[c * self.ny as usize + j as usize])
    }

    /// Sets a block inside the box; outside it nothing happens.
    pub fn set(&mut self, x: i32, y: i32, z: i32, b: u8) {
        if let Some(c) = self.column(x, z) {
            let j = y - self.y0;
            if j >= 0 && j < self.ny {
                self.cells[c * self.ny as usize + j as usize] = b;
            }
        }
    }

    /// Opens a column to the void: nothing under the floor any more.
    pub fn dig_void(&mut self, x: i32, z: i32) {
        if let Some(c) = self.column(x, z) {
            self.void[c] = true;
            for j in 0..self.ny as usize {
                self.cells[c * self.ny as usize + j] = AIR;
            }
        }
    }

    /// The floor again in a column: solid under `floor`, air from there up.
    pub fn fill_floor(&mut self, x: i32, z: i32, floor: i32) {
        if let Some(c) = self.column(x, z) {
            self.void[c] = false;
            for j in 0..self.ny {
                self.cells[c * self.ny as usize + j as usize] = if self.y0 + j < floor { FULL } else { AIR };
            }
        }
    }

    /// The highest top of a block in the box, as a height.
    pub fn top(&self) -> f64 {
        let mut best = f64::NEG_INFINITY;
        for c in 0..(self.nx * self.nz) as usize {
            for j in (0..self.ny as usize).rev() {
                let b = self.cells[c * self.ny as usize + j];
                if b != AIR {
                    best = best.max((self.y0 + j as i32) as f64 + ray_height(b));
                    break;
                }
            }
        }
        best
    }
}

/// The height of a block's collision box within its cell: none for air
/// and a cobweb.
#[inline]
pub fn height(b: u8) -> f64 {
    match b {
        AIR | WEB => 0.0,
        SLAB => 0.5,
        _ => 1.0,
    }
}

/// The height of the box a ray meets in a block (`Block.collisionRayTrace`
/// over the block's bounds): a cobweb is a whole block to it.
#[inline]
pub fn ray_height(b: u8) -> f64 {
    match b {
        AIR => 0.0,
        SLAB => 0.5,
        _ => 1.0,
    }
}

/// Result of `World.rayTraceBlocks`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RayHit {
    /// `null`: the ray never left its starting cell.
    None,
    /// A MISS result placed where the ray entered the last air cell.
    Miss(V3),
    /// The ray hit a solid block at this point.
    Block(V3),
}

impl World {
    /// Square arena centred on the origin. `half` is the distance from the
    /// centre to the inner face of each wall. Both numbers are rounded to
    /// whole blocks.
    pub fn arena(floor_y: f64, half: f64, walls: bool) -> World {
        let floor_i = floor_y.round() as i32;
        let half_i = half.round() as i32;
        let (floor_y, half) = (floor_i as f64, half_i as f64);
        // Higher than an ender pearl can be thrown.
        let top_i = floor_i + 64;
        World {
            floor_y,
            half,
            floor_i,
            half_i,
            top_i,
            walls,
            extra: HashSet::new(),
            extra_webs: HashSet::new(),
            has_webs: false,
            terrain: None,
            void_y: f64::NEG_INFINITY,
            terrain_top: floor_y,
        }
    }

    /// An empty terrain box over the inside of this arena, to build on.
    pub fn blank_terrain(&self) -> Terrain {
        Terrain::flat(-self.half_i, -self.half_i, 2 * self.half_i, 2 * self.half_i, self.floor_i)
    }

    /// Puts terrain into the arena.
    pub fn set_terrain(&mut self, t: Terrain) {
        self.terrain_top = t.top().max(self.floor_y);
        self.has_webs = !self.extra_webs.is_empty() || t.cells.iter().any(|&b| b == WEB);
        self.void_y = if t.void.iter().any(|&v| v) { self.floor_y - VOID_DEPTH } else { f64::NEG_INFINITY };
        self.terrain = Some(Arc::new(t));
    }

    pub fn terrain(&self) -> Option<&Terrain> {
        self.terrain.as_deref()
    }

    /// What the block at these block coordinates is: air, a full cube or
    /// a slab.
    #[inline]
    pub fn block(&self, x: i32, y: i32, z: i32) -> u8 {
        if let Some(t) = &self.terrain {
            if let Some(b) = t.get(x, y, z) {
                return b;
            }
        }
        if self.has_webs && self.extra_webs.contains(&[x, y, z]) {
            return WEB;
        }
        self.base_solid(x, y, z) as u8
    }

    /// The top of the box a ray meets in the block at these coordinates.
    #[inline]
    pub fn ray_block_height(&self, x: i32, y: i32, z: i32) -> f64 {
        ray_height(self.block(x, y, z))
    }

    /// Whether there is a cobweb anywhere.
    #[inline]
    pub fn has_webs(&self) -> bool {
        self.has_webs
    }

    /// Is the block at these coordinates a cobweb?
    #[inline]
    pub fn web(&self, x: i32, y: i32, z: i32) -> bool {
        self.has_webs && self.block(x, y, z) == WEB
    }

    /// `Entity.doBlockCollisions` for cobwebs: whether a block whose cell
    /// the box reaches into by more than 0.001 is a cobweb.
    pub fn in_web(&self, bb: &Aabb) -> bool {
        if !self.has_webs {
            return false;
        }
        let (x0, x1) = (floor_d(bb.min[0] + 0.001), floor_d(bb.max[0] - 0.001));
        let (y0, y1) = (floor_d(bb.min[1] + 0.001), floor_d(bb.max[1] - 0.001));
        let (z0, z1) = (floor_d(bb.min[2] + 0.001), floor_d(bb.max[2] - 0.001));
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    if self.block(x, y, z) == WEB {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// The top of the box of the block at these coordinates, above its
    /// bottom: 0 for air.
    #[inline]
    pub fn block_height(&self, x: i32, y: i32, z: i32) -> f64 {
        height(self.block(x, y, z))
    }

    /// A flat floor with single full cubes at the given block coordinates.
    /// Used to rebuild the scenery of a recording made in the real game.
    pub fn with_blocks(floor_y: f64, blocks: &[[i32; 3]]) -> World {
        World::with_blocks_and_webs(floor_y, blocks, &[])
    }

    /// The same with cobwebs as well.
    pub fn with_blocks_and_webs(floor_y: f64, blocks: &[[i32; 3]], webs: &[[i32; 3]]) -> World {
        let mut w = World::arena(floor_y, 4096.0, false);
        w.extra.extend(blocks.iter().copied());
        w.extra_webs.extend(webs.iter().copied());
        w.has_webs = !w.extra_webs.is_empty();
        if w.has_webs {
            w.terrain_top = webs.iter().map(|b| b[1] as f64 + 1.0).fold(w.terrain_top, f64::max);
        }
        w
    }

    /// Is the block at these block coordinates a full solid cube?
    #[inline]
    pub fn solid(&self, x: i32, y: i32, z: i32) -> bool {
        if self.terrain.is_some() {
            return self.block(x, y, z) == FULL;
        }
        self.base_solid(x, y, z)
    }

    #[inline]
    fn base_solid(&self, x: i32, y: i32, z: i32) -> bool {
        if y < self.floor_i {
            return true;
        }
        if !self.extra.is_empty() && self.extra.contains(&[x, y, z]) {
            return true;
        }
        self.walls && y < self.top_i && (x >= self.half_i || x < -self.half_i || z >= self.half_i || z < -self.half_i)
    }

    /// Whether the straight path from `a` to `b` is certain to touch no
    /// block: both ends lie in the open space above the floor and between
    /// the walls, and that space has no corner to cut.
    #[inline]
    pub fn clear_path(&self, a: V3, b: V3) -> bool {
        if !self.extra.is_empty() || !self.extra_webs.is_empty() {
            return false;
        }
        let open = |p: &V3| {
            p[1] >= self.terrain_top
                && (!self.walls
                    || (p[1] < self.top_i as f64 && p[0] >= -self.half && p[0] < self.half && p[2] >= -self.half && p[2] < self.half))
        };
        open(&a) && open(&b)
    }

    /// `World.getCollidingBoundingBoxes`: the boxes of the solid blocks
    /// that overlap `bb`, appended to `out`.
    pub fn colliding_boxes(&self, bb: &Aabb, out: &mut Vec<Aabb>) {
        let (i, j) = (floor_d(bb.min[0]), floor_d(bb.max[0] + 1.0));
        let (k, l) = (floor_d(bb.min[1]), floor_d(bb.max[1] + 1.0));
        let (i1, j1) = (floor_d(bb.min[2]), floor_d(bb.max[2] + 1.0));
        for x in i..j {
            for z in i1..j1 {
                for y in (k - 1)..l {
                    let h = self.block_height(x, y, z);
                    if h > 0.0 {
                        let lo = [x as f64, y as f64, z as f64];
                        let b = Aabb::new(lo, [lo[0] + 1.0, lo[1] + h, lo[2] + 1.0]);
                        if bb.intersects(&b) {
                            out.push(b);
                        }
                    }
                }
            }
        }
    }

    /// `!getCollidingBoundingBoxes(bb).isEmpty()`.
    pub fn collides(&self, bb: &Aabb) -> bool {
        let (i, j) = (floor_d(bb.min[0]), floor_d(bb.max[0] + 1.0));
        let (k, l) = (floor_d(bb.min[1]), floor_d(bb.max[1] + 1.0));
        let (i1, j1) = (floor_d(bb.min[2]), floor_d(bb.max[2] + 1.0));
        for x in i..j {
            for z in i1..j1 {
                for y in (k - 1)..l {
                    let h = self.block_height(x, y, z);
                    if h > 0.0 {
                        let lo = [x as f64, y as f64, z as f64];
                        if bb.intersects(&Aabb::new(lo, [lo[0] + 1.0, lo[1] + h, lo[2] + 1.0])) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// `Vec3.getIntermediateWith{X,Y,Z}Value`.
    #[inline]
    fn intermediate(a: V3, b: V3, axis: usize, value: f64) -> Option<V3> {
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        if d[axis] * d[axis] < 1.0000000116860974E-7 {
            return None;
        }
        let t = (value - a[axis]) / d[axis];
        if t >= 0.0 && t <= 1.0 {
            Some([a[0] + d[0] * t, a[1] + d[1] * t, a[2] + d[2] * t])
        } else {
            None
        }
    }

    /// `Block.collisionRayTrace` for a block at (x, y, z) whose box is
    /// `top` high (a full cube, a cobweb or a slab).
    fn cube_trace(pos: [i32; 3], top: f64, start: V3, end: V3) -> Option<V3> {
        let hi = [1.0, top, 1.0];
        let off = [-(pos[0] as f64), -(pos[1] as f64), -(pos[2] as f64)];
        let s = [start[0] + off[0], start[1] + off[1], start[2] + off[2]];
        let e = [end[0] + off[0], end[1] + off[1], end[2] + off[2]];
        let mut best: Option<V3> = None;
        for axis in 0..3 {
            let (u, w) = match axis {
                0 => (1, 2),
                1 => (0, 2),
                _ => (0, 1),
            };
            for value in [0.0, hi[axis]] {
                if let Some(p) = Self::intermediate(s, e, axis, value) {
                    if p[u] >= 0.0 && p[u] <= hi[u] && p[w] >= 0.0 && p[w] <= hi[w] {
                        let better = match best {
                            None => true,
                            Some(b) => dist_sq(s, p) < dist_sq(s, b),
                        };
                        if better {
                            best = Some(p);
                        }
                    }
                }
            }
        }
        best.map(|p| [p[0] + pos[0] as f64, p[1] + pos[1] as f64, p[2] + pos[2] as f64])
    }

    /// `World.rayTraceBlocks(a, b, false, false, return_last)`.
    pub fn ray_trace_blocks(&self, mut a: V3, b: V3, return_last: bool) -> RayHit {
        let (i, j, k) = (floor_d(b[0]), floor_d(b[1]), floor_d(b[2]));
        let (mut l, mut i1, mut j1) = (floor_d(a[0]), floor_d(a[1]), floor_d(a[2]));
        let top = self.ray_block_height(l, i1, j1);
        if top > 0.0 {
            if let Some(h) = Self::cube_trace([l, i1, j1], top, a, b) {
                return RayHit::Block(h);
            }
        }
        let mut last: Option<V3> = None;
        let finish = |last: Option<V3>| match last {
            Some(p) if return_last => RayHit::Miss(p),
            _ => RayHit::None,
        };
        for _ in 0..201 {
            if l == i && i1 == j && j1 == k {
                return finish(last);
            }
            let (mut fx, mut fy, mut fz) = (true, true, true);
            let (mut d0, mut d1, mut d2) = (999.0f64, 999.0f64, 999.0f64);
            if i > l {
                d0 = l as f64 + 1.0;
            } else if i < l {
                d0 = l as f64 + 0.0;
            } else {
                fx = false;
            }
            if j > i1 {
                d1 = i1 as f64 + 1.0;
            } else if j < i1 {
                d1 = i1 as f64 + 0.0;
            } else {
                fy = false;
            }
            if k > j1 {
                d2 = j1 as f64 + 1.0;
            } else if k < j1 {
                d2 = j1 as f64 + 0.0;
            } else {
                fz = false;
            }
            let (mut d3, mut d4, mut d5) = (999.0f64, 999.0f64, 999.0f64);
            let d6 = b[0] - a[0];
            let d7 = b[1] - a[1];
            let d8 = b[2] - a[2];
            if fx {
                d3 = (d0 - a[0]) / d6;
            }
            if fy {
                d4 = (d1 - a[1]) / d7;
            }
            if fz {
                d5 = (d2 - a[2]) / d8;
            }
            if d3 == 0.0 {
                d3 = -1.0E-4;
            }
            if d4 == 0.0 {
                d4 = -1.0E-4;
            }
            if d5 == 0.0 {
                d5 = -1.0E-4;
            }
            // Which face of the next cell the ray enters through.
            let (east, up, south);
            if d3 < d4 && d3 < d5 {
                east = !(i > l);
                up = false;
                south = false;
                a = [d0, a[1] + d7 * d3, a[2] + d8 * d3];
            } else if d4 < d5 {
                east = false;
                up = !(j > i1);
                south = false;
                a = [a[0] + d6 * d4, d1, a[2] + d8 * d4];
            } else {
                east = false;
                up = false;
                south = !(k > j1);
                a = [a[0] + d6 * d5, a[1] + d7 * d5, d2];
            }
            l = floor_d(a[0]) - east as i32;
            i1 = floor_d(a[1]) - up as i32;
            j1 = floor_d(a[2]) - south as i32;
            let top = self.ray_block_height(l, i1, j1);
            if top > 0.0 {
                if let Some(h) = Self::cube_trace([l, i1, j1], top, a, b) {
                    return RayHit::Block(h);
                }
            } else {
                last = Some(a);
            }
        }
        finish(last)
    }

    /// Is there a solid block between two points (`canEntityBeSeen`)?
    pub fn blocked(&self, a: V3, b: V3) -> bool {
        matches!(self.ray_trace_blocks(a, b, false), RayHit::Block(_))
    }
}
