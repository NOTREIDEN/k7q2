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
    /// `isInWeb`: the last move ended with the box in a cobweb.
    pub in_web: bool,
    /// Block boxes of the move in progress, kept to avoid allocating.
    scratch: Vec<Aabb>,
}

/// `Entity.stepHeight` of a living entity.
pub const STEP_HEIGHT: f64 = 0.6f32 as f64;

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
            in_web: false,
            scratch: Vec::new(),
        }
    }

    /// `Entity.setPosition`: recreates the bounding box from the position.
    #[inline]
    pub fn set_position(&mut self, p: V3) {
        self.pos = p;
        self.bb = Aabb::player(p);
    }

    /// `Entity.moveEntity(x, y, z)` without the sneaking edge-stop, with
    /// the step up of a living entity (`stepHeight` 0.6): a slab is walked
    /// onto, a full block has to be jumped. A move that starts in a cobweb
    /// goes a quarter as far sideways and a twentieth as far up or down,
    /// and the motion is gone; one that ends in a cobweb leaves the body in
    /// it (`doBlockCollisions`, `setInWeb`).
    pub fn move_entity(&mut self, world: &World, x: f64, y: f64, z: f64, track_fall: bool) {
        self.move_entity_step(world, x, y, z, track_fall, STEP_HEIGHT);
    }

    /// The same move for an entity of `stepHeight` `step`: 0 for one that
    /// never steps up, as a rod's hook. The box keeps its size, whatever it is.
    pub fn move_entity_step(&mut self, world: &World, mut x: f64, mut y: f64, mut z: f64, track_fall: bool, step: f64) {
        if self.in_web {
            self.in_web = false;
            x *= 0.25;
            y *= 0.05000000074505806;
            z *= 0.25;
            self.motion = [0.0; 3];
        }
        let (d3, d4, d5) = (x, y, z);
        let start = self.bb;
        let mut list = std::mem::take(&mut self.scratch);
        list.clear();
        world.colliding_boxes(&self.bb.add_coord(x, y, z), &mut list);

        for s in list.iter() {
            y = s.calc_y_offset(&self.bb, y);
        }
        self.bb = self.bb.offset(0.0, y, 0.0);
        let flag1 = self.on_ground || d4 != y && d4 < 0.0;
        for s in list.iter() {
            x = s.calc_x_offset(&self.bb, x);
        }
        self.bb = self.bb.offset(x, 0.0, 0.0);
        for s in list.iter() {
            z = s.calc_z_offset(&self.bb, z);
        }
        self.bb = self.bb.offset(0.0, 0.0, z);

        if step > 0.0 && flag1 && (d3 != x || d5 != z) {
            let (d11, d7, d8) = (x, y, z);
            let bb3 = self.bb;
            self.bb = start;
            y = step;
            list.clear();
            world.colliding_boxes(&self.bb.add_coord(d3, y, d5), &mut list);
            let mut bb4 = self.bb;
            let bb5 = bb4.add_coord(d3, 0.0, d5);
            let mut d9 = y;
            for s in list.iter() {
                d9 = s.calc_y_offset(&bb5, d9);
            }
            bb4 = bb4.offset(0.0, d9, 0.0);
            let mut d15 = d3;
            for s in list.iter() {
                d15 = s.calc_x_offset(&bb4, d15);
            }
            bb4 = bb4.offset(d15, 0.0, 0.0);
            let mut d16 = d5;
            for s in list.iter() {
                d16 = s.calc_z_offset(&bb4, d16);
            }
            bb4 = bb4.offset(0.0, 0.0, d16);
            let mut bb14 = self.bb;
            let mut d17 = y;
            for s in list.iter() {
                d17 = s.calc_y_offset(&bb14, d17);
            }
            bb14 = bb14.offset(0.0, d17, 0.0);
            let mut d18 = d3;
            for s in list.iter() {
                d18 = s.calc_x_offset(&bb14, d18);
            }
            bb14 = bb14.offset(d18, 0.0, 0.0);
            let mut d19 = d5;
            for s in list.iter() {
                d19 = s.calc_z_offset(&bb14, d19);
            }
            bb14 = bb14.offset(0.0, 0.0, d19);
            let d20 = d15 * d15 + d16 * d16;
            let d10 = d18 * d18 + d19 * d19;
            if d20 > d10 {
                x = d15;
                z = d16;
                y = -d9;
                self.bb = bb4;
            } else {
                x = d18;
                z = d19;
                y = -d17;
                self.bb = bb14;
            }
            for s in list.iter() {
                y = s.calc_y_offset(&self.bb, y);
            }
            self.bb = self.bb.offset(0.0, y, 0.0);
            if d11 * d11 + d8 * d8 >= x * x + z * z {
                x = d11;
                y = d7;
                z = d8;
                self.bb = bb3;
            }
        }
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
        if world.in_web(&self.bb) {
            self.in_web = true;
            self.fall_distance = 0.0;
        }
    }
}
