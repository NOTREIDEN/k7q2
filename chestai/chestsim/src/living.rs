//! State and movement code shared by the client-side player
//! (`EntityPlayerSP`) and the server-side player (`EntityPlayerMP`):
//! the `generic.movementSpeed` attribute, `jump`, `moveFlying`,
//! `moveEntityWithHeading` and the common part of `onLivingUpdate`.
use crate::body::Body;
use crate::mth;
use crate::world::World;

/// `generic.movementSpeed` with the two modifiers that matter in a duel.
/// Both use operation 2, and `ModifiableAttributeInstance.computeValue`
/// multiplies them in `HashSet` iteration order. For these two UUIDs that
/// order is sprint first (bucket 6), then the speed potion (bucket 13).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Attr {
    pub base: f64,
    /// Speed potion amplifier, or -1 when there is no speed effect.
    pub speed_amp: i8,
    /// Slowness amplifier, or -1 when there is none. Slowness X (9) takes
    /// the attribute to zero: a player can then only move by jumping.
    pub slow_amp: i8,
    pub sprint: bool,
}

pub const BASE_WALK_SPEED: f64 = 0.1f32 as f64;
const SPRINT_BOOST: f64 = 0.30000001192092896;
const SPEED_POTION: f64 = 0.20000000298023224;
const SLOW_POTION: f64 = 0.15000000596046448;

impl Attr {
    pub fn new(speed_amp: i8) -> Self {
        Attr { base: BASE_WALK_SPEED, speed_amp, slow_amp: -1, sprint: false }
    }

    pub fn value(&self) -> f64 {
        let mut d1 = self.base;
        if self.sprint {
            d1 *= 1.0 + SPRINT_BOOST;
        }
        if self.speed_amp >= 0 {
            d1 *= 1.0 + SPEED_POTION * (self.speed_amp as i32 + 1) as f64;
        }
        if self.slow_amp >= 0 {
            d1 *= 1.0 - SLOW_POTION * (self.slow_amp as i32 + 1) as f64;
        }
        d1.clamp(0.0, 1024.0)
    }
}

#[derive(Clone, Debug)]
pub struct Living {
    pub body: Body,
    pub yaw: f32,
    pub pitch: f32,
    /// Sprint bit of the entity flags (`DataWatcher` index 0, bit 3).
    pub sprinting: bool,
    pub attr: Attr,
    /// `jumpMovementFactor`, refreshed at the end of each living update, so
    /// a sprint change reaches air control a tick late. Ground speed has no
    /// such lag: `EntityPlayer.getAIMoveSpeed` reads the attribute directly.
    pub jump_factor: f32,
    pub jump_ticks: i32,
}

impl Living {
    pub fn new(pos: [f64; 3], yaw: f32, pitch: f32, speed_amp: i8) -> Self {
        let attr = Attr::new(speed_amp);
        let mut body = Body::new(pos);
        body.on_ground = true;
        // Resting vertical motion of a player standing still.
        body.motion[1] = -0.0784000015258789;
        Living {
            body,
            yaw,
            pitch,
            sprinting: false,
            attr,
            jump_factor: 0.02,
            jump_ticks: 0,
        }
    }

    /// `EntityLivingBase.jump`.
    pub fn jump(&mut self) {
        self.body.motion[1] = 0.42f32 as f64;
        if self.sprinting {
            let f = self.yaw * 0.017453292f32;
            self.body.motion[0] -= (mth::sin(f) * 0.2f32) as f64;
            self.body.motion[2] += (mth::cos(f) * 0.2f32) as f64;
        }
    }

    /// `Entity.moveFlying`.
    pub fn move_flying(&mut self, mut strafe: f32, mut forward: f32, friction: f32) {
        let mut f = strafe * strafe + forward * forward;
        if f >= 1.0E-4f32 {
            f = mth::sqrt_f(f);
            if f < 1.0 {
                f = 1.0;
            }
            f = friction / f;
            strafe *= f;
            forward *= f;
            let f1 = mth::sin(self.yaw * mth::PI_F / 180.0f32);
            let f2 = mth::cos(self.yaw * mth::PI_F / 180.0f32);
            self.body.motion[0] += (strafe * f2 - forward * f1) as f64;
            self.body.motion[2] += (forward * f2 + strafe * f1) as f64;
        }
    }

    /// `EntityLivingBase.moveEntityWithHeading`, land branch. Every block in
    /// the arena has the default slipperiness 0.6.
    pub fn move_entity_with_heading(&mut self, world: &World, strafe: f32, forward: f32, track_fall: bool) {
        if world.in_water(&self.body.bb) {
            // Water branch: slow push, strong drag, weak gravity, and a hop
            // out where the bank is one block high.
            let y0 = self.body.pos[1];
            self.move_flying(strafe, forward, 0.02);
            let m = self.body.motion;
            self.body.move_entity(world, m[0], m[1], m[2], track_fall);
            self.body.motion[0] *= 0.800000011920929;
            self.body.motion[1] *= 0.800000011920929;
            self.body.motion[2] *= 0.800000011920929;
            self.body.motion[1] -= 0.02;
            if self.body.collided_h {
                let m = self.body.motion;
                let off = self.body.bb.offset(m[0], m[1] + 0.6000000238418579 - self.body.pos[1] + y0, m[2]);
                if !world.collides(&off) && !world.any_water(&off) {
                    self.body.motion[1] = 0.30000001192092896;
                }
            }
            return;
        }
        let mut f4 = 0.91f32;
        if self.body.on_ground {
            f4 = 0.6f32 * 0.91f32;
        }
        let f = 0.16277136f32 / (f4 * f4 * f4);
        let f5 = if self.body.on_ground { self.attr.value() as f32 * f } else { self.jump_factor };
        self.move_flying(strafe, forward, f5);
        f4 = 0.91f32;
        if self.body.on_ground {
            f4 = 0.6f32 * 0.91f32;
        }
        let m = self.body.motion;
        self.body.move_entity(world, m[0], m[1], m[2], track_fall);
        self.body.motion[1] -= 0.08;
        self.body.motion[1] *= 0.9800000190734863;
        self.body.motion[0] *= f4 as f64;
        self.body.motion[2] *= f4 as f64;
    }

    /// Common part of `EntityLivingBase.onLivingUpdate` for a player that is
    /// not being interpolated. `strafe`/`forward`/`jumping` are what
    /// `updateEntityActionState` produced this tick.
    pub fn living_update(&mut self, world: &World, strafe: f32, forward: f32, jumping: bool, track_fall: bool) -> bool {
        if self.jump_ticks > 0 {
            self.jump_ticks -= 1;
        }
        for a in 0..3 {
            if self.body.motion[a].abs() < 0.005 {
                self.body.motion[a] = 0.0;
            }
        }
        let mut jumped = false;
        if jumping && world.in_water(&self.body.bb) {
            self.body.motion[1] += 0.03999999910593033;
        } else if jumping {
            if self.body.on_ground && self.jump_ticks == 0 {
                self.jump();
                self.jump_ticks = 10;
                jumped = true;
            }
        } else {
            self.jump_ticks = 0;
        }
        let s = strafe * 0.98f32;
        let f = forward * 0.98f32;
        self.move_entity_with_heading(world, s, f, track_fall);
        jumped
    }

    /// Tail of `EntityPlayer.onLivingUpdate`.
    pub fn player_post(&mut self) {
        self.jump_factor = 0.02f32;
        if self.sprinting {
            self.jump_factor = (self.jump_factor as f64 + 0.02f32 as f64 * 0.3) as f32;
        }
    }
}
