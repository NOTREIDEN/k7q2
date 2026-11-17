//! Scripted opponents. They read the same client state a mod could read and
//! produce the same kind of input as the learning agent. They give early
//! training a target, and later serve as fixed yardsticks.
use crate::aabb::V3;
use crate::client::{Client, ClientCfg, Input};
use crate::mth::{self, Rng};
use crate::world::World;

#[derive(Clone, Copy, Debug)]
pub struct BotParams {
    pub level: u8,
    /// Largest turn per tick, degrees.
    pub max_turn: f64,
    /// Standard deviation of the aim error added each tick, degrees.
    pub aim_noise: f64,
    /// Chance to click on a tick where the crosshair will be on target.
    pub click_prob: f64,
    pub strafe: bool,
    pub wtap: bool,
    pub jump_reset: bool,
    /// Chance to follow a hit with a short block (a block hit) instead of
    /// letting go of W.
    pub blockhit: f64,
    /// Chance to block for a moment when the opponent starts a swing
    /// within reach.
    pub block_react: f64,
}

impl BotParams {
    pub fn level(level: u8) -> BotParams {
        match level {
            0 | 1 => BotParams {
                level,
                max_turn: 4.0,
                aim_noise: 0.0,
                click_prob: 0.0,
                strafe: false,
                wtap: false,
                jump_reset: false,
                blockhit: 0.0,
                block_react: 0.0,
            },
            2 => BotParams {
                level,
                max_turn: 12.0,
                aim_noise: 2.0,
                click_prob: 0.45,
                strafe: false,
                wtap: false,
                jump_reset: false,
                blockhit: 0.0,
                block_react: 0.0,
            },
            3 => BotParams {
                level,
                max_turn: 30.0,
                aim_noise: 0.8,
                click_prob: 0.7,
                strafe: true,
                wtap: true,
                jump_reset: false,
                blockhit: 0.25,
                block_react: 0.0,
            },
            _ => BotParams {
                level: 4,
                max_turn: 90.0,
                aim_noise: 0.2,
                click_prob: 1.0,
                strafe: true,
                wtap: true,
                jump_reset: true,
                blockhit: 0.4,
                block_react: 0.25,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BotState {
    strafe_dir: i8,
    strafe_left: i32,
    wtap_left: i32,
    walk_fwd: i8,
    walk_side: i8,
    walk_left: i32,
    walk_turn: i32,
    walk_sprint: bool,
    block_left: i32,
    saw_swing: bool,
}

fn gauss(rng: &mut Rng) -> f64 {
    let u1 = rng.f64().max(1e-12);
    let u2 = rng.f64();
    (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
}

/// Yaw and pitch (degrees) that point from `eye` at `p`.
pub fn angles_to(eye: V3, p: V3) -> (f64, f64) {
    let dx = p[0] - eye[0];
    let dy = p[1] - eye[1];
    let dz = p[2] - eye[2];
    let h = (dx * dx + dz * dz).sqrt();
    ((-dx).atan2(dz).to_degrees(), -(dy.atan2(h)).to_degrees())
}

pub fn act(c: &Client, world: &World, cfg: &ClientCfg, p: &BotParams, st: &mut BotState, rng: &mut Rng) -> Input {
    let mut inp = Input::default();
    let deg_per_count = cfg.mouse_f1 as f64 * 0.15;
    if p.level == 0 {
        return inp;
    }
    if p.level == 1 {
        // Wanderer: holds random keys for random stretches, never attacks.
        if st.walk_left <= 0 {
            st.walk_left = 5 + (rng.next_u64() % 30) as i32;
            st.walk_fwd = (rng.next_u64() % 3) as i8 - 1;
            st.walk_side = (rng.next_u64() % 3) as i8 - 1;
            st.walk_turn = (rng.next_u64() % 41) as i32 - 20;
            st.walk_sprint = rng.chance(0.5);
        }
        st.walk_left -= 1;
        inp.forward = st.walk_fwd;
        inp.strafe = st.walk_side;
        inp.sprint_key = st.walk_sprint;
        inp.jump = rng.chance(0.03);
        inp.dx = st.walk_turn;
        // Keep the head roughly level.
        inp.dy = (-(c.lp.l.pitch as f64) / deg_per_count * 0.2).round() as i32;
        return inp;
    }

    let lp = &c.lp;
    let rp = &c.rp;
    let eye = lp.eye();
    // Lead the target by one tick of its visible movement.
    let lead = [rp.pos[0] - rp.prev_pos[0], rp.pos[1] - rp.prev_pos[1], rp.pos[2] - rp.prev_pos[2]];
    let ty = eye[1].clamp(rp.pos[1] + 0.3, rp.pos[1] + 1.6);
    let aim = [rp.pos[0] + lead[0], ty + lead[1], rp.pos[2] + lead[2]];
    let (yaw_to, pitch_to) = angles_to(eye, aim);
    let mut dyaw = mth::wrap_deg(yaw_to - lp.l.yaw as f64);
    let mut dpitch = pitch_to - lp.l.pitch as f64;
    dyaw = dyaw.clamp(-p.max_turn, p.max_turn) + p.aim_noise * gauss(rng);
    dpitch = dpitch.clamp(-p.max_turn, p.max_turn) + p.aim_noise * 0.5 * gauss(rng);
    inp.dx = (dyaw / deg_per_count).round() as i32;
    inp.dy = (dpitch / deg_per_count).round() as i32;

    let dx = rp.pos[0] - lp.l.body.pos[0];
    let dz = rp.pos[2] - lp.l.body.pos[2];
    let hdist = (dx * dx + dz * dz).sqrt();

    inp.sprint_key = true;
    inp.forward = if hdist > 1.2 { 1 } else { 0 };
    if p.wtap {
        if rp.hurt_time == 10 && lp.ticks_since_attack <= 12 {
            // A hit landed: reset the sprint, by letting go of W or by a
            // short block.
            if rng.chance(p.blockhit) {
                st.block_left = 1 + (rng.next_u64() % 2) as i32;
            } else {
                st.wtap_left = 2;
            }
        }
        if st.wtap_left > 0 {
            st.wtap_left -= 1;
            inp.forward = 0;
        }
    }
    let swing_starts = rp.swing_in_progress && !st.saw_swing;
    st.saw_swing = rp.swing_in_progress;
    if swing_starts && hdist < 3.6 && lp.hurt_time <= 1 && st.block_left == 0 && rng.chance(p.block_react) {
        st.block_left = 2;
    }
    if st.block_left > 0 {
        st.block_left -= 1;
        inp.use_item = true;
    }
    if p.strafe && hdist < 6.0 {
        if st.strafe_left <= 0 {
            st.strafe_left = 6 + (rng.next_u64() % 18) as i32;
            st.strafe_dir = if rng.chance(0.5) { 1 } else { -1 };
        }
        st.strafe_left -= 1;
        inp.strafe = st.strafe_dir;
    }
    if p.jump_reset && lp.hurt_time == 10 && lp.l.body.on_ground {
        inp.jump = true;
    }

    // Would the crosshair be on the target after this turn?
    let new_yaw = (lp.l.yaw as f64 + inp.dx as f64 * deg_per_count) as f32;
    let new_pitch = ((lp.l.pitch as f64 + inp.dy as f64 * deg_per_count) as f32).clamp(-90.0, 90.0);
    let ray_yaw = if cfg.mouse_delay_fix { new_yaw } else { lp.yaw_head };
    let pk = c.pick_ray(world, new_pitch, ray_yaw);
    if pk.entity && rng.chance(p.click_prob) {
        inp.attack = true;
    }
    inp
}
