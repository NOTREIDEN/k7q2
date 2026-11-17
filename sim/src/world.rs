//! The arena: a flat floor of full blocks with optional full-height walls.
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

#[derive(Clone, Debug)]
pub struct World {
    pub floor_y: f64,
    pub half: f64,
    floor_i: i32,
    half_i: i32,
    top_i: i32,
    walls: bool,
    /// Single full cubes placed on top of the floor (parity scenarios).
    extra: HashSet<[i32; 3]>,
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
        World { floor_y, half, floor_i, half_i, top_i, walls, extra: HashSet::new() }
    }

    /// A flat floor with single full cubes at the given block coordinates.
    /// Used to rebuild the scenery of a recording made in the real game.
    pub fn with_blocks(floor_y: f64, blocks: &[[i32; 3]]) -> World {
        let mut w = World::arena(floor_y, 4096.0, false);
        w.extra.extend(blocks.iter().copied());
        w
    }

    /// Is the block at these block coordinates a full solid cube?
    #[inline]
    pub fn solid(&self, x: i32, y: i32, z: i32) -> bool {
        if y < self.floor_i {
            return true;
        }
        if !self.extra.is_empty() && self.extra.contains(&[x, y, z]) {
            return true;
        }
        self.walls && y < self.top_i && (x >= self.half_i || x < -self.half_i || z >= self.half_i || z < -self.half_i)
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
            if self.solid(l, i1, j1) {
                if let Some(h) = Self::cube_trace([l, i1, j1], a, b) {
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
