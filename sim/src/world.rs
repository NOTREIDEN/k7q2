//! The arena: a handful of solid boxes (floor and optional walls).
//! Vanilla collides against one box per block; for full cubes laid out on a
//! grid, merged boxes give the same result, so the arena uses merged boxes.
use crate::aabb::{Aabb, V3};

#[derive(Clone, Debug)]
pub struct World {
    pub solids: Vec<Aabb>,
    pub floor_y: f64,
    pub half: f64,
}

impl World {
    /// Square arena centred on the origin. `half` is the distance from the
    /// centre to the inner face of each wall.
    pub fn arena(floor_y: f64, half: f64, walls: bool) -> World {
        let far = half + 64.0;
        let mut solids = vec![Aabb::new([-far, floor_y - 8.0, -far], [far, floor_y, far])];
        if walls {
            let top = floor_y + 32.0;
            solids.push(Aabb::new([half, floor_y, -far], [far, top, far]));
            solids.push(Aabb::new([-far, floor_y, -far], [-half, top, far]));
            solids.push(Aabb::new([-far, floor_y, half], [far, top, far]));
            solids.push(Aabb::new([-far, floor_y, -far], [far, top, -half]));
        }
        World { solids, floor_y, half }
    }

    /// Distance along segment a-b to the first solid, if any.
    pub fn raytrace(&self, a: V3, b: V3) -> Option<V3> {
        let mut best: Option<V3> = None;
        let mut best_d = f64::INFINITY;
        for s in &self.solids {
            if let Some(p) = s.calculate_intercept(a, b) {
                let d = crate::aabb::dist_sq(a, p);
                if d < best_d {
                    best_d = d;
                    best = Some(p);
                }
            }
        }
        best
    }
}
