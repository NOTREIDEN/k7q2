//! One game client: the local player (`EntityPlayerSP`), the other player
//! as this client sees it (`EntityOtherPlayerMP`), the crosshair pick
//! (`EntityRenderer.getMouseOver`) and one `Minecraft.runTick`.
use crate::aabb::{dist, Aabb, V3, EYE_HEIGHT};
use crate::living::Living;
use crate::mth;
use crate::net::{C2S, S2C};
use crate::world::{RayHit, World};

/// Keys and mouse for one client tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    /// W = 1, S = -1.
    pub forward: i8,
    /// A (left) = 1, D (right) = -1, as in `MovementInputFromOptions`.
    pub strafe: i8,
    pub jump: bool,
    pub sprint_key: bool,
    /// One full left click (press and release) inside this tick.
    pub attack: bool,
    /// Further full clicks inside the same tick, after the one above. A
    /// mouse clicked more than twenty times a second sends two in some
    /// ticks.
    pub extra_clicks: u8,
    /// Mouse counts since the previous tick. Positive x turns right,
    /// positive y looks down.
    pub dx: i32,
    pub dy: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct ClientCfg {
    /// `f * f * f * 8` from the sensitivity slider; degrees per count is
    /// this times 0.15.
    pub mouse_f1: f32,
    /// Vanilla 1.8.9 aims the tick-start pick with the head yaw of the
    /// previous tick. `true` uses the current yaw instead, which is what the
    /// common mouse-delay fix does.
    pub mouse_delay_fix: bool,
    /// Knockback enchantment level on the held sword.
    pub kb_ench: i32,
}

impl ClientCfg {
    pub fn with_sensitivity(sens: f32) -> ClientCfg {
        let f = sens * 0.6f32 + 0.2f32;
        ClientCfg { mouse_f1: f * f * f * 8.0f32, mouse_delay_fix: false, kb_ench: 0 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pick {
    /// Crosshair is on the other player within attack reach.
    pub entity: bool,
    /// Distance from the eye to the point where the ray enters the box.
    pub dist: f64,
}

#[derive(Clone, Debug)]
pub struct RemotePlayer {
    pub pos: V3,
    pub prev_pos: V3,
    pub bb: Aabb,
    pub yaw: f32,
    pub pitch: f32,
    pub server_pos: [i32; 3],
    pub target: V3,
    pub target_yaw: f64,
    pub target_pitch: f64,
    pub increments: i32,
    pub on_ground: bool,
    pub sprinting: bool,
    pub hurt_time: i32,
    pub swing_in_progress: bool,
    pub swing_progress_int: i32,
    /// Bookkeeping for observations, not game state.
    pub ticks_since_hurt: i32,
}

impl RemotePlayer {
    /// State right after `handleSpawnPlayer` for a player whose true
    /// position is `pos`.
    pub fn spawn(pos: V3, yaw: f32, pitch: f32, on_ground: bool) -> Self {
        let sp = [mth::floor_d(pos[0] * 32.0), mth::floor_d(pos[1] * 32.0), mth::floor_d(pos[2] * 32.0)];
        let p = [sp[0] as f64 / 32.0, sp[1] as f64 / 32.0, sp[2] as f64 / 32.0];
        let by = mth::floor_f(yaw * 256.0f32 / 360.0f32) as i8;
        let bp = mth::floor_f(pitch * 256.0f32 / 360.0f32) as i8;
        let y = (by as i32 * 360) as f32 / 256.0f32;
        let pt = (bp as i32 * 360) as f32 / 256.0f32;
        RemotePlayer {
            pos: p,
            prev_pos: p,
            bb: Aabb::player(p),
            yaw: y,
            pitch: pt,
            server_pos: sp,
            target: p,
            target_yaw: y as f64,
            target_pitch: pt as f64,
            increments: 0,
            on_ground,
            sprinting: false,
            hurt_time: 0,
            swing_in_progress: false,
            swing_progress_int: 0,
            ticks_since_hurt: 1000,
        }
    }

    #[inline]
    fn set_position(&mut self, p: V3) {
        self.pos = p;
        self.bb = Aabb::player(p);
    }

    /// `EntityOtherPlayerMP.setPositionAndRotation2`.
    #[inline]
    fn set_target(&mut self, p: V3, yaw: f32, pitch: f32) {
        self.target = p;
        self.target_yaw = yaw as f64;
        self.target_pitch = pitch as f64;
        self.increments = 3;
    }

    #[inline]
    fn byte_angle(b: i8) -> f32 {
        (b as i32 * 360) as f32 / 256.0f32
    }

    /// `NetHandlerPlayClient.handleEntityMovement`.
    fn rel_move(&mut self, d: [i8; 3], rot: Option<(i8, i8)>, on_ground: bool) {
        for a in 0..3 {
            self.server_pos[a] += d[a] as i32;
        }
        let p = [
            self.server_pos[0] as f64 / 32.0,
            self.server_pos[1] as f64 / 32.0,
            self.server_pos[2] as f64 / 32.0,
        ];
        let (yaw, pitch) = match rot {
            Some((y, p)) => (Self::byte_angle(y), Self::byte_angle(p)),
            None => (self.yaw, self.pitch),
        };
        self.set_target(p, yaw, pitch);
        self.on_ground = on_ground;
    }

    /// `NetHandlerPlayClient.handleEntityTeleport`.
    fn teleport(&mut self, sp: [i32; 3], yaw: i8, pitch: i8, on_ground: bool) {
        self.server_pos = sp;
        let d = [sp[0] as f64 / 32.0, sp[1] as f64 / 32.0, sp[2] as f64 / 32.0];
        let (f, f1) = (Self::byte_angle(yaw), Self::byte_angle(pitch));
        if (self.pos[0] - d[0]).abs() < 0.03125
            && (self.pos[1] - d[1]).abs() < 0.015625
            && (self.pos[2] - d[2]).abs() < 0.03125
        {
            let p = self.pos;
            self.set_target(p, f, f1);
        } else {
            self.set_target(d, f, f1);
        }
        self.on_ground = on_ground;
    }

    /// `EntityLivingBase.swingItem` on the client copy.
    fn swing_item(&mut self) {
        if !self.swing_in_progress || self.swing_progress_int >= 3 || self.swing_progress_int < 0 {
            self.swing_progress_int = -1;
            self.swing_in_progress = true;
        }
    }

    /// One `onUpdate` of `EntityOtherPlayerMP`.
    pub fn tick(&mut self) {
        self.prev_pos = self.pos;
        if self.hurt_time > 0 {
            self.hurt_time -= 1;
        }
        if self.ticks_since_hurt < 1000 {
            self.ticks_since_hurt += 1;
        }
        if self.increments > 0 {
            let n = self.increments as f64;
            let d0 = self.pos[0] + (self.target[0] - self.pos[0]) / n;
            let d1 = self.pos[1] + (self.target[1] - self.pos[1]) / n;
            let d2 = self.pos[2] + (self.target[2] - self.pos[2]) / n;
            let mut d3 = self.target_yaw - self.yaw as f64;
            while d3 < -180.0 {
                d3 += 360.0;
            }
            while d3 >= 180.0 {
                d3 -= 360.0;
            }
            self.yaw = (self.yaw as f64 + d3 / n) as f32;
            self.pitch = (self.pitch as f64 + (self.target_pitch - self.pitch as f64) / n) as f32;
            self.increments -= 1;
            self.set_position([d0, d1, d2]);
            self.yaw %= 360.0f32;
            self.pitch %= 360.0f32;
        }
        // updateArmSwingProgress
        if self.swing_in_progress {
            self.swing_progress_int += 1;
            if self.swing_progress_int >= 6 {
                self.swing_progress_int = 0;
                self.swing_in_progress = false;
            }
        } else {
            self.swing_progress_int = 0;
        }
    }
}

#[derive(Clone, Debug)]
pub struct LocalPlayer {
    pub l: Living,
    /// `rotationYawHead`, copied from the yaw once per tick.
    pub yaw_head: f32,
    /// `MovementInput` as left by the previous tick.
    pub mi_forward: f32,
    pub mi_strafe: f32,
    pub mi_jump: bool,
    pub sprinting_ticks_left: i32,
    pub sprint_toggle_timer: i32,
    pub server_sprint_state: bool,
    pub last_reported: V3,
    pub last_yaw: f32,
    pub last_pitch: f32,
    pub position_update_ticks: i32,
    pub hurt_time: i32,
    /// Bookkeeping for observations, not game state.
    pub last_kb: V3,
    pub ticks_since_kb: i32,
    pub ticks_since_hurt: i32,
    pub ticks_since_swing: i32,
    pub ticks_since_attack: i32,
}

impl LocalPlayer {
    pub fn new(pos: V3, yaw: f32, pitch: f32, speed_amp: i8) -> Self {
        let l = Living::new(pos, yaw, pitch, speed_amp);
        LocalPlayer {
            l,
            yaw_head: yaw,
            mi_forward: 0.0,
            mi_strafe: 0.0,
            mi_jump: false,
            sprinting_ticks_left: 0,
            sprint_toggle_timer: 0,
            server_sprint_state: false,
            last_reported: pos,
            last_yaw: yaw,
            last_pitch: pitch,
            position_update_ticks: 0,
            hurt_time: 0,
            last_kb: [0.0; 3],
            ticks_since_kb: 1000,
            ticks_since_hurt: 1000,
            ticks_since_swing: 1000,
            ticks_since_attack: 1000,
        }
    }

    /// `EntityPlayerSP.setSprinting`.
    #[inline]
    pub fn set_sprinting(&mut self, s: bool) {
        self.l.sprinting = s;
        self.l.attr.sprint = s;
        self.sprinting_ticks_left = if s { 600 } else { 0 };
    }

    /// `Entity.setAngles(yaw, pitch)`.
    #[inline]
    pub fn set_angles(&mut self, yaw: f32, pitch: f32) {
        self.l.yaw = (self.l.yaw as f64 + yaw as f64 * 0.15) as f32;
        self.l.pitch = (self.l.pitch as f64 - pitch as f64 * 0.15) as f32;
        if self.l.pitch < -90.0 {
            self.l.pitch = -90.0;
        } else if self.l.pitch > 90.0 {
            self.l.pitch = 90.0;
        }
    }

    #[inline]
    pub fn eye(&self) -> V3 {
        let p = self.l.body.pos;
        [p[0], p[1] + EYE_HEIGHT, p[2]]
    }
}

/// What one client tick did, for rewards and statistics.
#[derive(Clone, Copy, Debug, Default)]
pub struct TickOut {
    pub pick: Pick,
    pub swung: bool,
    pub attacked: bool,
    /// The attack was made with the client sprint flag set, so the client
    /// slowed itself down and dropped its own sprint.
    pub sprint_attack: bool,
    pub jumped: bool,
}

/// Ways a client can depart from the plain game. They stand for what some
/// opponents on real servers have, so that the agent meets them in
/// training. The default is the plain game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    /// Attack reach from the eye, in blocks (the game: 3.0).
    pub reach: f64,
    /// Share of the horizontal knockback the client takes (the game: 1.0).
    pub kb_taken: f64,
    /// The client may still hit the other player where it showed them up
    /// to this many ticks ago (the game: 0). This is what holding back the
    /// other player's movement packets gives.
    pub backtrack: u8,
}

impl Default for Edge {
    fn default() -> Self {
        Edge { reach: 3.0, kb_taken: 1.0, backtrack: 0 }
    }
}

/// Longest backtrack an `Edge` can ask for, in ticks.
pub const MAX_BACKTRACK: usize = 8;

#[derive(Clone, Debug)]
pub struct Client {
    pub lp: LocalPlayer,
    pub rp: RemotePlayer,
    pub edge: Edge,
    /// Where the other player was shown at the picks of the last ticks,
    /// newest first.
    shown: [Aabb; MAX_BACKTRACK],
}

/// `EntityRenderer.getMouseOver(1.0F)` for a world that holds one other
/// player. `yaw` is whichever yaw the caller wants the ray to use.
pub fn pick_with(world: &World, eye: V3, own_bb: &Aabb, pitch: f32, yaw: f32, remote_bb: &Aabb, reach_limit: f64) -> Pick {
    let d0 = 4.5f32 as f64;
    let look = mth::look_vec(pitch, yaw);
    let end = [eye[0] + look[0] * d0, eye[1] + look[1] * d0, eye[2] + look[2] * d0];
    // objectMouseOver from the block ray. In open air this is usually a
    // MISS placed where the ray entered its last cell, and the distance to
    // that point is the limit an entity has to beat.
    let trace = world.ray_trace_blocks(eye, end, true);
    let d1 = match trace {
        RayHit::Block(h) | RayHit::Miss(h) => dist(h, eye),
        RayHit::None => d0,
    };
    let query = own_bb.add_coord(look[0] * d0, look[1] * d0, look[2] * d0).expand(1.0, 1.0, 1.0);
    let mut pointed = false;
    let mut vec33 = eye;
    let mut d2 = d1;
    if remote_bb.intersects(&query) {
        let f1 = 0.1f32 as f64;
        let ebb = remote_bb.expand(f1, f1, f1);
        let mop = ebb.calculate_intercept(eye, end);
        if ebb.is_vec_inside(eye) {
            if d2 >= 0.0 {
                pointed = true;
                vec33 = mop.unwrap_or(eye);
                d2 = 0.0;
            }
        } else if let Some(h) = mop {
            let d3 = dist(eye, h);
            if d3 < d2 || d2 == 0.0 {
                pointed = true;
                vec33 = h;
                d2 = d3;
            }
        }
    }
    let reach = dist(eye, vec33);
    if pointed && reach > reach_limit {
        pointed = false;
    }
    if pointed && (d2 < d1 || trace == RayHit::None) {
        Pick { entity: true, dist: reach }
    } else {
        Pick { entity: false, dist: reach }
    }
}

/// `EntityPlayerSP.pushOutOfBlocks`.
#[inline]
fn push_out_of_blocks(world: &World, motion: &mut V3, x: f64, y: f64, z: f64) {
    let (bx, by, bz) = (mth::floor_d(x), mth::floor_d(y), mth::floor_d(z));
    // isOpenBlockSpace: neither this block nor the one above is a full cube.
    let open = |x: i32, z: i32| !world.solid(x, by, z) && !world.solid(x, by + 1, z);
    if open(bx, bz) {
        return;
    }
    let d0 = x - bx as f64;
    let d1 = z - bz as f64;
    let mut i = -1;
    let mut d2 = 9999.0f64;
    if open(bx - 1, bz) && d0 < d2 {
        d2 = d0;
        i = 0;
    }
    if open(bx + 1, bz) && 1.0 - d0 < d2 {
        d2 = 1.0 - d0;
        i = 1;
    }
    if open(bx, bz - 1) && d1 < d2 {
        d2 = d1;
        i = 4;
    }
    if open(bx, bz + 1) && 1.0 - d1 < d2 {
        i = 5;
    }
    let f = 0.1f32;
    match i {
        0 => motion[0] = -f as f64,
        1 => motion[0] = f as f64,
        4 => motion[2] = -f as f64,
        5 => motion[2] = f as f64,
        _ => {}
    }
}

impl Client {
    pub fn new(pos: V3, yaw: f32, pitch: f32, speed_amp: i8, other_pos: V3, other_yaw: f32, other_pitch: f32) -> Self {
        let rp = RemotePlayer::spawn(other_pos, other_yaw, other_pitch, true);
        let shown = [rp.bb; MAX_BACKTRACK];
        Client { lp: LocalPlayer::new(pos, yaw, pitch, speed_amp), rp, edge: Edge::default(), shown }
    }

    /// The pick the next tick would make if the mouse did not move.
    pub fn pick(&self, world: &World, cfg: &ClientCfg) -> Pick {
        let yaw = if cfg.mouse_delay_fix { self.lp.l.yaw } else { self.lp.yaw_head };
        self.pick_ray(world, self.lp.l.pitch, yaw)
    }

    /// The pick for a ray with the given angles: against the other player
    /// where this client shows them now and, for a client with a backtrack
    /// edge, where it showed them on its last ticks.
    pub fn pick_ray(&self, world: &World, pitch: f32, yaw: f32) -> Pick {
        let eye = self.lp.eye();
        let own = &self.lp.l.body.bb;
        let now = pick_with(world, eye, own, pitch, yaw, &self.rp.bb, self.edge.reach);
        if now.entity {
            return now;
        }
        for old in self.shown.iter().take(self.edge.backtrack as usize) {
            let p = pick_with(world, eye, own, pitch, yaw, old, self.edge.reach);
            if p.entity {
                return p;
            }
        }
        now
    }

    /// A packet handler of `NetHandlerPlayClient`, run on the main thread
    /// before the next tick.
    ///
    /// The answer to a server teleport is sent from inside the handler, so
    /// it leaves before the packets of the next tick; it goes to `out`.
    pub fn handle(&mut self, p: S2C, out: &mut Vec<C2S>) {
        match p {
            S2C::PosLook { pos, yaw, pitch } => {
                // handlePlayerPosLook, then Entity.setPositionAndRotation.
                let lp = &mut self.lp;
                lp.l.body.motion = [0.0; 3];
                lp.l.body.set_position(pos);
                lp.l.yaw = yaw % 360.0f32;
                lp.l.pitch = pitch % 360.0f32;
                out.push(C2S::Player {
                    pos: Some([lp.l.body.pos[0], lp.l.body.bb.min[1], lp.l.body.pos[2]]),
                    rot: Some((lp.l.yaw, lp.l.pitch)),
                    on_ground: false,
                });
            }
            S2C::Velocity { own, v } => {
                if own {
                    let k = self.edge.kb_taken;
                    let m = [v[0] as f64 / 8000.0 * k, v[1] as f64 / 8000.0, v[2] as f64 / 8000.0 * k];
                    self.lp.l.body.motion = m;
                    self.lp.last_kb = m;
                    self.lp.ticks_since_kb = 0;
                }
            }
            S2C::Hurt { own } => {
                if own {
                    self.lp.hurt_time = 10;
                    self.lp.ticks_since_hurt = 0;
                } else {
                    self.rp.hurt_time = 10;
                    self.rp.ticks_since_hurt = 0;
                }
            }
            S2C::RelMove { d, on_ground } => self.rp.rel_move(d, None, on_ground),
            S2C::Look { yaw, pitch, on_ground } => self.rp.rel_move([0, 0, 0], Some((yaw, pitch)), on_ground),
            S2C::LookMove { d, yaw, pitch, on_ground } => self.rp.rel_move(d, Some((yaw, pitch)), on_ground),
            S2C::Teleport { p, yaw, pitch, on_ground } => self.rp.teleport(p, yaw, pitch, on_ground),
            S2C::Meta { own, sprinting } => {
                if own {
                    // The flags byte is overwritten without going through
                    // setSprinting, so neither the modifier nor
                    // sprintingTicksLeft changes here.
                    self.lp.l.sprinting = sprinting;
                } else {
                    self.rp.sprinting = sprinting;
                }
            }
            S2C::Props { own, base, speed_amp, sprint } => {
                if own {
                    self.lp.l.attr.base = base;
                    self.lp.l.attr.speed_amp = speed_amp;
                    self.lp.l.attr.sprint = sprint;
                }
            }
            S2C::Swing => self.rp.swing_item(),
        }
    }

    /// One `Minecraft.runTick`: mouse, pick, click, then the entity updates.
    pub fn tick(&mut self, world: &World, inp: &Input, cfg: &ClientCfg, out: &mut Vec<C2S>) -> TickOut {
        let mut t = TickOut::default();

        // Mouse movement accumulated since the previous tick.
        if inp.dx != 0 || inp.dy != 0 {
            self.lp.set_angles(inp.dx as f32 * cfg.mouse_f1, -(inp.dy as f32 * cfg.mouse_f1));
        }

        // getMouseOver(1.0F)
        let ray_yaw = if cfg.mouse_delay_fix { self.lp.l.yaw } else { self.lp.yaw_head };
        t.pick = self.pick_ray(world, self.lp.l.pitch, ray_yaw);
        self.shown.copy_within(0..MAX_BACKTRACK - 1, 1);
        self.shown[0] = self.rp.bb;
        let lp = &mut self.lp;

        // clickMouse. The button is released again inside the same tick, so
        // leftClickCounter is back at zero before the next one.
        // Every click of the tick runs clickMouse against the same pick. A
        // click into the air sets leftClickCounter, which swallows the
        // clicks that follow it in this tick.
        let clicks = if inp.attack { 1 + inp.extra_clicks } else { 0 };
        for _ in 0..clicks {
            out.push(C2S::Swing);
            t.swung = true;
            lp.ticks_since_swing = 0;
            if !t.pick.entity {
                break;
            }
            out.push(C2S::Attack);
            t.attacked = true;
            lp.ticks_since_attack = 0;
            // attackTargetEntityWithCurrentItem on the client: the copy
            // of the other player always reports a successful hit.
            let i = cfg.kb_ench + if lp.l.sprinting { 1 } else { 0 };
            if i > 0 {
                lp.l.body.motion[0] *= 0.6;
                lp.l.body.motion[2] *= 0.6;
                lp.set_sprinting(false);
                t.sprint_attack = true;
            }
        }

        // --- EntityPlayerSP.onUpdate ---
        // onEntityUpdate
        if lp.hurt_time > 0 {
            lp.hurt_time -= 1;
        }
        // EntityPlayerSP.onLivingUpdate
        if lp.sprinting_ticks_left > 0 {
            lp.sprinting_ticks_left -= 1;
            if lp.sprinting_ticks_left == 0 {
                lp.set_sprinting(false);
            }
        }
        if lp.sprint_toggle_timer > 0 {
            lp.sprint_toggle_timer -= 1;
        }
        let f = 0.8f32;
        let flag2 = lp.mi_forward >= f;
        // MovementInputFromOptions.updatePlayerMoveState
        lp.mi_strafe = 0.0;
        lp.mi_forward = 0.0;
        if inp.forward > 0 {
            lp.mi_forward += 1.0;
        }
        if inp.forward < 0 {
            lp.mi_forward -= 1.0;
        }
        if inp.strafe > 0 {
            lp.mi_strafe += 1.0;
        }
        if inp.strafe < 0 {
            lp.mi_strafe -= 1.0;
        }
        lp.mi_jump = inp.jump;
        // pushOutOfBlocks at four points around the feet. They are inside
        // the player's own box, so this only acts on a player that has got
        // into a wall (which a server teleport next to one can do).
        let w = 0.6f32 as f64 * 0.35;
        let (px, py, pz) = (lp.l.body.pos[0], lp.l.body.bb.min[1] + 0.5, lp.l.body.pos[2]);
        push_out_of_blocks(world, &mut lp.l.body.motion, px - w, py, pz + w);
        push_out_of_blocks(world, &mut lp.l.body.motion, px - w, py, pz - w);
        push_out_of_blocks(world, &mut lp.l.body.motion, px + w, py, pz - w);
        push_out_of_blocks(world, &mut lp.l.body.motion, px + w, py, pz + w);
        if lp.l.body.on_ground && !flag2 && lp.mi_forward >= f && !lp.l.sprinting {
            if lp.sprint_toggle_timer <= 0 && !inp.sprint_key {
                lp.sprint_toggle_timer = 7;
            } else {
                lp.set_sprinting(true);
            }
        }
        if !lp.l.sprinting && lp.mi_forward >= f && inp.sprint_key {
            lp.set_sprinting(true);
        }
        if lp.l.sprinting && (lp.mi_forward < f || lp.l.body.collided_h) {
            lp.set_sprinting(false);
        }
        // EntityPlayer.onLivingUpdate -> EntityLivingBase.onLivingUpdate.
        // updateEntityActionState copies the input and the head yaw.
        lp.yaw_head = lp.l.yaw;
        let (s, fw, j) = (lp.mi_strafe, lp.mi_forward, lp.mi_jump);
        t.jumped = lp.l.living_update(world, s, fw, j, true);
        lp.l.player_post();

        // onUpdateWalkingPlayer
        let flag = lp.l.sprinting;
        if flag != lp.server_sprint_state {
            out.push(C2S::Sprint(flag));
            lp.server_sprint_state = flag;
        }
        let pos = lp.l.body.pos;
        let min_y = lp.l.body.bb.min[1];
        let d0 = pos[0] - lp.last_reported[0];
        let d1 = min_y - lp.last_reported[1];
        let d2 = pos[2] - lp.last_reported[2];
        let d3 = (lp.l.yaw - lp.last_yaw) as f64;
        let d4 = (lp.l.pitch - lp.last_pitch) as f64;
        let moved = d0 * d0 + d1 * d1 + d2 * d2 > 9.0E-4 || lp.position_update_ticks >= 20;
        let turned = d3 != 0.0 || d4 != 0.0;
        let on_ground = lp.l.body.on_ground;
        out.push(C2S::Player {
            pos: if moved { Some([pos[0], min_y, pos[2]]) } else { None },
            rot: if turned { Some((lp.l.yaw, lp.l.pitch)) } else { None },
            on_ground,
        });
        lp.position_update_ticks += 1;
        if moved {
            lp.last_reported = [pos[0], min_y, pos[2]];
            lp.position_update_ticks = 0;
        }
        if turned {
            lp.last_yaw = lp.l.yaw;
            lp.last_pitch = lp.l.pitch;
        }

        if lp.ticks_since_kb < 1000 {
            lp.ticks_since_kb += 1;
        }
        if lp.ticks_since_hurt < 1000 {
            lp.ticks_since_hurt += 1;
        }
        if lp.ticks_since_swing < 1000 {
            lp.ticks_since_swing += 1;
        }
        if lp.ticks_since_attack < 1000 {
            lp.ticks_since_attack += 1;
        }

        // The other player's entity updates after the local one.
        self.rp.tick();
        t
    }
}
