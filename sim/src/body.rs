//! Position, bounding box, motion and the collision move shared by the
//! client-side player and the server-side player (`Entity.moveEntity`).
use crate::aabb::{Aabb, V3};
use crate::world::World;

#[derive(Clone, Debug)]
pub struct Body {
    pub pos: V3,
    pub bb: Aabb,
    pub motion: V3,
    pub on_ground: bool,
    pub collided_h: bool,
    pub collided_v: bool,
    pub fall_distance: f32,
}

impl Body {
    pub fn new(pos: V3) -> Self {
        Body {
            pos,
            bb: Aabb::player(pos),
            motion: [0.0; 3],
            on_ground: false,
            collided_h: false,
            collided_v: false,
            fall_distance: 0.0,
        }
    }

    /// `Entity.setPosition`: recreates the bounding box from the position.
    #[inline]
    pub fn set_position(&mut self, p: V3) {
        self.pos = p;
        self.bb = Aabb::player(p);
    }

    /// `Entity.moveEntity(x, y, z)` without webs, sneaking edge-stop or
    /// step-up. Step-up never changes the outcome against full-height walls
    /// on a flat floor, which is all the arena contains.
    pub fn move_entity(&mut self, world: &World, mut x: f64, mut y: f64, mut z: f64, track_fall: bool) {
        let (d3, d4, d5) = (x, y, z);
        let query = self.bb.add_coord(x, y, z);

        for s in world.solids.iter().filter(|s| s.intersects(&query)) {
            y = s.calc_y_offset(&self.bb, y);
        }
        self.bb = self.bb.offset(0.0, y, 0.0);
        for s in world.solids.iter().filter(|s| s.intersects(&query)) {
            x = s.calc_x_offset(&self.bb, x);
        }
        self.bb = self.bb.offset(x, 0.0, 0.0);
        for s in world.solids.iter().filter(|s| s.intersects(&query)) {
            z = s.calc_z_offset(&self.bb, z);
        }
        self.bb = self.bb.offset(0.0, 0.0, z);

        // resetPositionToBB
        self.pos = [
            (self.bb.min[0] + self.bb.max[0]) / 2.0,
            self.bb.min[1],
            (self.bb.min[2] + self.bb.max[2]) / 2.0,
        ];
        self.collided_h = d3 != x || d5 != z;
        self.collided_v = d4 != y;
        self.on_ground = self.collided_v && d4 < 0.0;

        if track_fall {
            // updateFallState
            if self.on_ground {
                self.fall_distance = 0.0;
            } else if y < 0.0 {
                self.fall_distance = (self.fall_distance as f64 - y) as f32;
            }
        }
        if d3 != x { self.motion[0] = 0.0 }
        if d5 != z { self.motion[2] = 0.0 }
        if d4 != y { self.motion[1] = 0.0 }
    }
}
