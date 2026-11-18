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
    /// Half the width and the height of the box (a player's by default).
    pub hw: f64,
    pub h: f64,
    /// Block boxes of the move in progress, kept to avoid allocating.
    scratch: Vec<Aabb>,
}

impl Body {
    pub fn new(pos: V3) -> Self {
        Self::sized(pos, crate::aabb::HALF_WIDTH, crate::aabb::HEIGHT)
    }

    /// A body with its own box size (a mob's).
    pub fn sized(pos: V3, hw: f64, h: f64) -> Self {
        Body {
            pos,
            bb: Aabb::new([pos[0] - hw, pos[1], pos[2] - hw], [pos[0] + hw, pos[1] + h, pos[2] + hw]),
            hw,
            h,
            motion: [0.0; 3],
            on_ground: false,
            collided_h: false,
            collided_v: false,
            fall_distance: 0.0,
            scratch: Vec::new(),
        }
    }

    /// `Entity.setPosition`: recreates the bounding box from the position.
    #[inline]
    pub fn set_position(&mut self, p: V3) {
        self.pos = p;
        self.bb = Aabb::new([p[0] - self.hw, p[1], p[2] - self.hw], [p[0] + self.hw, p[1] + self.h, p[2] + self.hw]);
    }

    /// `Entity.moveEntity(x, y, z)` without webs, sneaking edge-stop or
    /// step-up. Every block of the map is a full cube, which a step of 0.6
    /// never climbs, so step-up never changes the outcome.
    pub fn move_entity(&mut self, world: &World, mut x: f64, mut y: f64, mut z: f64, track_fall: bool) {
        let (d3, d4, d5) = (x, y, z);
        let mut list = std::mem::take(&mut self.scratch);
        list.clear();
        world.colliding_boxes(&self.bb.add_coord(x, y, z), &mut list);

        for s in list.iter() {
            y = s.calc_y_offset(&self.bb, y);
        }
        self.bb = self.bb.offset(0.0, y, 0.0);
        for s in list.iter() {
            x = s.calc_x_offset(&self.bb, x);
        }
        self.bb = self.bb.offset(x, 0.0, 0.0);
        for s in list.iter() {
            z = s.calc_z_offset(&self.bb, z);
        }
        self.bb = self.bb.offset(0.0, 0.0, z);
        self.scratch = list;

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
