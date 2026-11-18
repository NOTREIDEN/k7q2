//! A voxel world of full solid cubes and still water, bounded on all sides.
//!
//! Collision and the crosshair ray are the 1.8.9 code of the PvP simulator
//! (`World.getCollidingBoundingBoxes`, `World.rayTraceBlocks`), here run
//! against a grid instead of a flat arena. Outside the grid every cell is
//! solid, so nothing leaves the map.
use crate::aabb::{dist_sq, Aabb, V3};
use crate::mth::floor_d;

pub const AIR: u8 = 0;
pub const SOLID: u8 = 1;
pub const WATER: u8 = 2;
/// A chest: solid for collision and for the ray, and the thing that is
/// clicked to claim a reward.
pub const CHEST: u8 = 3;

#[derive(Clone, Debug)]
pub struct World {
    pub sx: i32,
    pub sy: i32,
    pub sz: i32,
    cells: Vec<u8>,
}

/// Result of `World.rayTraceBlocks`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RayHit {
    /// `null`: the ray never left its starting cell.
    None,
    /// A MISS result placed where the ray entered the last air cell.
    Miss(V3),
    /// The ray hit a solid block at this point, in this cell.
    Block(V3, [i32; 3]),
}

impl World {
    pub fn new(sx: i32, sy: i32, sz: i32) -> World {
        World { sx, sy, sz, cells: vec![AIR; (sx * sy * sz) as usize] }
    }

    /// A flat floor of solid blocks up to (not including) `floor_y`.
    pub fn flat(sx: i32, sy: i32, sz: i32, floor_y: i32) -> World {
        let mut w = World::new(sx, sy, sz);
        for x in 0..sx {
            for z in 0..sz {
                for y in 0..floor_y {
                    w.set(x, y, z, SOLID);
                }
            }
        }
        w
    }

    /// Every cell, x fastest, then z, then y.
    pub fn cells(&self) -> &[u8] {
        &self.cells
    }

    #[inline]
    fn idx(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        if x < 0 || y < 0 || z < 0 || x >= self.sx || y >= self.sy || z >= self.sz {
            None
        } else {
            Some(((y * self.sz + z) * self.sx + x) as usize)
        }
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> u8 {
        match self.idx(x, y, z) {
            Some(i) => self.cells[i],
            // Open sky above the map, walls of bedrock around it.
            None if y >= self.sy && x >= 0 && z >= 0 && x < self.sx && z < self.sz => AIR,
            None => SOLID,
        }
    }

    #[inline]
    pub fn set(&mut self, x: i32, y: i32, z: i32, v: u8) {
        if let Some(i) = self.idx(x, y, z) {
            self.cells[i] = v;
        }
    }

    /// Is the block at these block coordinates a full solid cube?
    #[inline]
    pub fn solid(&self, x: i32, y: i32, z: i32) -> bool {
        let c = self.get(x, y, z);
        c == SOLID || c == CHEST
    }

    #[inline]
    pub fn water(&self, x: i32, y: i32, z: i32) -> bool {
        self.get(x, y, z) == WATER
    }

    /// Y of the first non-solid cell above the highest solid cell of the
    /// column: where something standing on the column has its feet.
    pub fn ground(&self, x: i32, z: i32) -> i32 {
        let mut y = self.sy - 1;
        while y >= 0 && !self.solid(x, y, z) {
            y -= 1;
        }
        y + 1
    }

    /// `Entity.handleWaterMovement`: does the box, shrunk the way the game
    /// shrinks it, touch water?
    pub fn in_water(&self, bb: &Aabb) -> bool {
        let b = Aabb::new([bb.min[0] + 0.001, bb.min[1] + 0.4 + 0.001, bb.min[2] + 0.001], [bb.max[0] - 0.001, bb.max[1] - 0.001, bb.max[2] - 0.001]);
        let (i, j) = (floor_d(b.min[0]), floor_d(b.max[0] + 1.0));
        let (k, l) = (floor_d(b.min[1]), floor_d(b.max[1] + 1.0));
        let (i1, j1) = (floor_d(b.min[2]), floor_d(b.max[2] + 1.0));
        for x in i..j {
            for y in k..l {
                for z in i1..j1 {
                    if self.water(x, y, z) && (y + 1) as f64 >= b.min[1] {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Is any cell the box overlaps water (`isOffsetPositionInLiquid` is its
    /// negation together with no collision)?
    pub fn any_water(&self, bb: &Aabb) -> bool {
        let (i, j) = (floor_d(bb.min[0]), floor_d(bb.max[0] - 1e-7) + 1);
        let (k, l) = (floor_d(bb.min[1]), floor_d(bb.max[1] - 1e-7) + 1);
        let (i1, j1) = (floor_d(bb.min[2]), floor_d(bb.max[2] - 1e-7) + 1);
        for x in i..j {
            for y in k..l {
                for z in i1..j1 {
                    if self.water(x, y, z) {
                        return true;
                    }
                }
            }
        }
        false
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
                    if self.solid(x, y, z) {
                        let lo = [x as f64, y as f64, z as f64];
                        let b = Aabb::new(lo, [lo[0] + 1.0, lo[1] + 1.0, lo[2] + 1.0]);
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
                    if self.solid(x, y, z) {
                        let lo = [x as f64, y as f64, z as f64];
                        if bb.intersects(&Aabb::new(lo, [lo[0] + 1.0, lo[1] + 1.0, lo[2] + 1.0])) {
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

    /// `Block.collisionRayTrace` for a full cube at block (x, y, z).
    fn cube_trace(pos: [i32; 3], start: V3, end: V3) -> Option<V3> {
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
            for value in [0.0, 1.0] {
                if let Some(p) = Self::intermediate(s, e, axis, value) {
                    if p[u] >= 0.0 && p[u] <= 1.0 && p[w] >= 0.0 && p[w] <= 1.0 {
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
        if self.solid(l, i1, j1) {
            if let Some(h) = Self::cube_trace([l, i1, j1], a, b) {
                return RayHit::Block(h, [l, i1, j1]);
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
            if self.solid(l, i1, j1) {
                if let Some(h) = Self::cube_trace([l, i1, j1], a, b) {
                    return RayHit::Block(h, [l, i1, j1]);
                }
            } else {
                last = Some(a);
            }
        }
        finish(last)
    }

    /// Is there a solid block between two points (`canEntityBeSeen`)?
    pub fn blocked(&self, a: V3, b: V3) -> bool {
        matches!(self.ray_trace_blocks(a, b, false), RayHit::Block(..))
    }
}
