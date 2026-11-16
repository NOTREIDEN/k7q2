//! The server side of a duel: `NetHandlerPlayServer` packet handlers, the
//! parts of `EntityPlayerMP` that decide hits and knockback, and the
//! `EntityTrackerEntry` that tells each client where the other player is.
use crate::aabb::{dist_sq, V3, EYE_HEIGHT};
use crate::living::Living;
use crate::mth::{self, Rng};
use crate::net::{C2S, S2C};
use crate::world::World;

/// Knockback numbers. The defaults are the vanilla constants; a test server
/// plugin exposes the same six values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KbProfile {
    /// Divisor applied to the victim's motion before the push (vanilla 2).
    pub friction: f64,
    pub horizontal: f64,
    pub vertical: f64,
    pub vertical_limit: f64,
    /// Extra push per sprint or Knockback level (vanilla 0.5F).
    pub extra_horizontal: f32,
    pub extra_vertical: f64,
}

impl KbProfile {
    pub const VANILLA: KbProfile = KbProfile {
        friction: 2.0,
        horizontal: 0.4000000059604645,
        vertical: 0.4000000059604645,
        vertical_limit: 0.4000000059604645,
        extra_horizontal: 0.5,
        extra_vertical: 0.1,
    };
}

/// Knockback as server plugins of the KnockbackManager kind set it: the
/// velocity sent to the victim is built from the hit alone, with one set of
/// numbers for a victim on the ground and one for a victim in the air,
/// instead of from the victim's own motion.
#[derive(Clone, Copy, Debug)]
pub struct KbPlugin {
    pub h_ground: f64,
    pub h_air: f64,
    /// Added along the attacker's view per sprint or Knockback level.
    pub h_sprint: f64,
    pub v_ground: f64,
    pub v_air: f64,
    pub v_sprint: f64,
    /// `true`: sprint knockback follows the server's sprint flag, which a
    /// hit clears, so it needs a sprint reset as in the plain game.
    /// `false`: it follows what the client last said, so every hit of a
    /// sprinting attacker carries it.
    pub stop_sprint: bool,
    /// Once the victim is higher than this above the last ground it
    /// touched, vertical knockback is `v_after_limit` until it lands.
    /// Negative turns the limit off.
    pub y_limit: f64,
    pub v_after_limit: f64,
}

impl KbPlugin {
    /// The profile the plugin ships with.
    pub const DEFAULT: KbPlugin = KbPlugin {
        h_ground: 0.4,
        h_air: 0.4,
        h_sprint: 0.5,
        v_ground: 0.36075,
        v_air: 0.24775,
        v_sprint: 0.1,
        stop_sprint: true,
        y_limit: 0.675,
        v_after_limit: 0.0,
    };
}

#[derive(Clone, Copy, Debug)]
pub struct ServerCfg {
    /// `generic.attackDamage` with the held sword.
    pub damage: f32,
    /// Sharpness bonus, added after the critical multiplier.
    pub ench_damage: f32,
    pub kb_ench: i32,
    pub infinite_health: bool,
    pub fall_damage: bool,
    /// `maxHurtResistantTime`: a player can be hit again once half of it
    /// has passed (the game: 20, a hit every 10 ticks). Servers lower it.
    pub max_no_damage: i32,
}

impl Default for ServerCfg {
    fn default() -> Self {
        // Diamond sword in 1.8: 1 (hand) + 7.
        ServerCfg { damage: 8.0, ench_damage: 0.0, kb_ench: 0, infinite_health: true, fall_damage: true, max_no_damage: 20 }
    }
}

/// A hit that passed the invulnerability check and produced knockback.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub attacker: usize,
    pub victim: usize,
    pub sprint: bool,
    pub crit: bool,
    /// Velocity sent to the victim, blocks per tick.
    pub kb: V3,
    /// Attacker to victim distance on the server, feet to feet.
    pub dist: f64,
}

#[derive(Clone, Debug)]
pub struct ServerPlayer {
    pub l: Living,
    pub hurt_resistant_time: i32,
    pub last_damage: f32,
    pub hurt_time: i32,
    pub health: f32,
    pub is_airborne: bool,
    pub velocity_changed: bool,
    pub meta_dirty: bool,
    pub attr_dirty: bool,
    pub swing_in_progress: bool,
    pub swing_progress_int: i32,
    // EntityTrackerEntry for this player.
    pub enc_pos: [i32; 3],
    pub enc_yaw: i32,
    pub enc_pitch: i32,
    pub update_counter: i32,
    pub ticks_since_teleport: i32,
    pub tracker_on_ground: bool,
    // NetHandlerPlayServer for this player.
    /// False from a server teleport until the client answers from there.
    pub has_moved: bool,
    pub last_pos: V3,
    pub net_ticks: i32,
    /// `networkTickCount` when a movement packet was last accepted.
    pub last_move_tick: i32,
    /// Chunk and 16-block slice the world has this player filed under. It
    /// is refreshed once per world tick, and other entities look a player
    /// up through it.
    pub chunk: [i32; 3],
    // What a knockback plugin keeps about this player.
    /// Sprint state as the client last reported it; a hit does not clear it.
    pub client_sprint: bool,
    /// Height of the last position the client reported standing on.
    pub last_ground_y: f64,
    /// Went above the plugin's height limit and has not landed since.
    pub touched_top: bool,
}

/// `chunkCoordX/Y/Z` for a position.
#[inline]
pub fn chunk_of(p: V3) -> [i32; 3] {
    [mth::floor_d(p[0] / 16.0), mth::floor_d(p[1] / 16.0).clamp(0, 15), mth::floor_d(p[2] / 16.0)]
}

impl ServerPlayer {
    pub fn new(pos: V3, yaw: f32, pitch: f32, speed_amp: i8) -> Self {
        let l = Living::new(pos, yaw % 360.0f32, pitch % 360.0f32, speed_amp);
        ServerPlayer {
            enc_pos: [mth::floor_d(pos[0] * 32.0), mth::floor_d(pos[1] * 32.0), mth::floor_d(pos[2] * 32.0)],
            enc_yaw: mth::floor_f(l.yaw * 256.0f32 / 360.0f32),
            enc_pitch: mth::floor_f(l.pitch * 256.0f32 / 360.0f32),
            l,
            hurt_resistant_time: 0,
            last_damage: 0.0,
            hurt_time: 0,
            health: 20.0,
            is_airborne: false,
            velocity_changed: false,
            meta_dirty: false,
            attr_dirty: false,
            swing_in_progress: false,
            swing_progress_int: 0,
            update_counter: 1,
            ticks_since_teleport: 0,
            tracker_on_ground: true,
            has_moved: true,
            last_pos: pos,
            net_ticks: 0,
            last_move_tick: 0,
            chunk: chunk_of(pos),
            client_sprint: false,
            last_ground_y: pos[1],
            touched_top: false,
        }
    }

    #[inline]
    fn add_velocity(&mut self, x: f64, y: f64, z: f64) {
        self.l.body.motion[0] += x;
        self.l.body.motion[1] += y;
        self.l.body.motion[2] += z;
        self.is_airborne = true;
    }

    /// `EntityLivingBase.setSprinting` on the server.
    fn set_sprinting(&mut self, s: bool) {
        if self.l.sprinting != s {
            self.l.sprinting = s;
            self.meta_dirty = true;
        }
        if self.l.attr.sprint {
            self.l.attr.sprint = false;
            self.attr_dirty = true;
        }
        if s {
            self.l.attr.sprint = true;
            self.attr_dirty = true;
        }
    }

    /// `Entity.setPositionAndRotation`.
    #[inline]
    fn set_position_and_rotation(&mut self, p: V3, yaw: f32, pitch: f32) {
        self.l.body.set_position(p);
        self.l.yaw = yaw % 360.0f32;
        self.l.pitch = pitch % 360.0f32;
    }
}

/// `new S12PacketEntityVelocity(entity)`.
#[inline]
pub fn encode_velocity(m: V3) -> [i32; 3] {
    let mut v = [0i32; 3];
    for a in 0..3 {
        let c = m[a].clamp(-3.9, 3.9);
        v[a] = (c * 8000.0) as i32;
    }
    v
}

#[derive(Clone, Debug)]
pub struct Server {
    pub p: [ServerPlayer; 2],
    pub kb: KbProfile,
    /// When set, the velocity a hit sends comes from this instead of `kb`.
    pub plugin: Option<KbPlugin>,
    pub cfg: ServerCfg,
    /// Hits registered since the caller last drained this list.
    pub hits: Vec<Hit>,
    /// Attack packets that reached the server, per attacker.
    pub attacks_received: [u32; 2],
}

/// Packets produced by the server: (destination client, packet).
pub type Outbox = Vec<(usize, S2C)>;

impl Server {
    pub fn new(p0: ServerPlayer, p1: ServerPlayer, kb: KbProfile, cfg: ServerCfg) -> Self {
        Server { p: [p0, p1], kb, plugin: None, cfg, hits: Vec::with_capacity(4), attacks_received: [0; 2] }
    }

    /// A packet handler, run at the start of a server tick in arrival order.
    pub fn handle(&mut self, from: usize, pkt: C2S, world: &World, rng: &mut Rng, out: &mut Outbox) {
        match pkt {
            C2S::Swing => {
                let p = &mut self.p[from];
                if !p.swing_in_progress || p.swing_progress_int >= 3 || p.swing_progress_int < 0 {
                    p.swing_progress_int = -1;
                    p.swing_in_progress = true;
                    out.push((1 - from, S2C::Swing));
                }
            }
            C2S::Attack => self.use_entity_attack(from, world, rng, out),
            C2S::Sprint(s) => {
                self.p[from].set_sprinting(s);
                self.p[from].client_sprint = s;
            }
            C2S::Player { pos, rot, on_ground } => self.process_player(from, pos, rot, on_ground, world, rng, out),
        }
    }

    /// `NetHandlerPlayServer.setPlayerLocation` with no relative flags.
    fn set_player_location(&mut self, i: usize, pos: V3, yaw: f32, pitch: f32, out: &mut Outbox) {
        let p = &mut self.p[i];
        p.has_moved = false;
        p.last_pos = pos;
        p.last_ground_y = pos[1];
        p.set_position_and_rotation(pos, yaw, pitch);
        out.push((i, S2C::PosLook { pos, yaw, pitch }));
    }

    /// `NetHandlerPlayServer.processPlayer`, walking branch.
    fn process_player(
        &mut self,
        i: usize,
        pos: Option<V3>,
        rot: Option<(f32, f32)>,
        pkt_ground: bool,
        world: &World,
        rng: &mut Rng,
        out: &mut Outbox,
    ) {
        {
            let p = &mut self.p[i];
            if let Some(d) = pos {
                let d3 = dist_sq(p.last_pos, d);
                if !p.has_moved && d3 < 0.25 {
                    p.has_moved = true;
                }
            }
            if !p.has_moved {
                // Waiting for the client to answer a teleport: the packet is
                // dropped, and the teleport repeated once a second.
                if p.net_ticks - p.last_move_tick > 20 {
                    let (lp, yaw, pitch) = (p.last_pos, p.l.yaw, p.l.pitch);
                    self.set_player_location(i, lp, yaw, pitch, out);
                }
                return;
            }
            p.last_move_tick = p.net_ticks;
        }
        let last = self.p[i].l.body.pos;
        self.p[i].last_pos = last;
        let d7 = last[1];
        let d = pos.unwrap_or(last);
        let (f1, f2) = rot.unwrap_or((self.p[i].l.yaw, self.p[i].l.pitch));

        self.on_update_entity(i, world);

        let p = &mut self.p[i];
        p.set_position_and_rotation(last, f1, f2);
        let d11 = d[0] - p.l.body.pos[0];
        let d12 = d[1] - p.l.body.pos[1];
        let d13 = d[2] - p.l.body.pos[2];
        let m = p.l.body.motion;
        let d14 = m[0] * m[0] + m[1] * m[1] + m[2] * m[2];
        let d15 = d11 * d11 + d12 * d12 + d13 * d13;
        if d15 - d14 > 100.0 {
            // "moved too quickly!"
            let (yaw, pitch) = (p.l.yaw, p.l.pitch);
            self.set_player_location(i, last, yaw, pitch, out);
            return;
        }
        let f3 = 0.0625f32 as f64;
        let flag = !world.collides(&p.l.body.bb.contract(f3, f3, f3));
        if p.l.body.on_ground && !pkt_ground && d12 > 0.0 {
            p.l.jump();
            p.is_airborne = true;
        }
        p.l.body.move_entity(world, d11, d12, d13, false);
        p.l.body.on_ground = pkt_ground;
        // The vertical difference is thrown away by a condition that is
        // always true in the original.
        let e11 = d[0] - p.l.body.pos[0];
        let e13 = d[2] - p.l.body.pos[2];
        let flag1 = e11 * e11 + 0.0 + e13 * e13 > 0.0625; // "moved wrongly!"
        p.set_position_and_rotation(d, f1, f2);
        let flag2 = !world.collides(&p.l.body.bb.contract(f3, f3, f3));
        if flag && (flag1 || !flag2) {
            self.set_player_location(i, last, f1, f2, out);
            return;
        }
        p.l.body.on_ground = pkt_ground;
        // The plugin's note of where the player last stood (its move handler).
        if pkt_ground && pos.is_some() && d != last {
            p.last_ground_y = d[1];
            p.touched_top = false;
        }

        // EntityPlayerMP.handleFalling -> Entity.updateFallState
        let y = p.l.body.pos[1] - d7;
        if pkt_ground {
            if p.l.body.fall_distance > 0.0 {
                let dist = p.l.body.fall_distance;
                p.l.body.fall_distance = 0.0;
                if self.cfg.fall_damage {
                    let dmg = (dist - 3.0f32).ceil() as i32;
                    if dmg > 0 {
                        self.attack_entity_from(i, dmg as f32, None, rng, out);
                    }
                }
            }
        } else if y < 0.0 {
            p.l.body.fall_distance = (p.l.body.fall_distance as f64 - y) as f32;
        }
    }

    /// `EntityPlayerMP.onUpdateEntity`: the server runs a living update with
    /// no input on its own copy of the motion, then the caller snaps the
    /// position back. Only the motion and the ground flag survive.
    fn on_update_entity(&mut self, i: usize, world: &World) {
        {
            let p = &mut self.p[i];
            if p.hurt_time > 0 {
                p.hurt_time -= 1;
            }
            // updateArmSwingProgress
            if p.swing_in_progress {
                p.swing_progress_int += 1;
                if p.swing_progress_int >= 6 {
                    p.swing_progress_int = 0;
                    p.swing_in_progress = false;
                }
            } else {
                p.swing_progress_int = 0;
            }
            p.l.living_update(world, 0.0, 0.0, false, false);
        }
        // collideWithNearbyEntities: exists only on the server in 1.8.
        let j = 1 - i;
        let q = self.p[i].l.body.bb.expand(0.20000000298023224, 0.0, 0.20000000298023224);
        // getEntitiesInAABBexcluding looks through the chunks within two
        // blocks of the box, where a player is filed by its position at the
        // last world tick.
        let c = self.p[j].chunk;
        let near = |lo: f64, hi: f64, v: i32| mth::floor_d((lo - 2.0) / 16.0) <= v && v <= mth::floor_d((hi + 2.0) / 16.0);
        let filed = near(q.min[0], q.max[0], c[0])
            && near(q.min[2], q.max[2], c[2])
            && mth::floor_d((q.min[1] - 2.0) / 16.0).clamp(0, 15) <= c[1]
            && c[1] <= mth::floor_d((q.max[1] + 2.0) / 16.0).clamp(0, 15);
        if filed && self.p[j].l.body.bb.intersects(&q) {
            // other.applyEntityCollision(this)
            let mut d0 = self.p[i].l.body.pos[0] - self.p[j].l.body.pos[0];
            let mut d1 = self.p[i].l.body.pos[2] - self.p[j].l.body.pos[2];
            let mut d2 = d0.abs().max(d1.abs());
            if d2 >= 0.009999999776482582 {
                d2 = mth::sqrt_d(d2) as f64;
                d0 /= d2;
                d1 /= d2;
                let mut d3 = 1.0 / d2;
                if d3 > 1.0 {
                    d3 = 1.0;
                }
                d0 *= d3;
                d1 *= d3;
                d0 *= 0.05000000074505806;
                d1 *= 0.05000000074505806;
                self.p[j].add_velocity(-d0, 0.0, -d1);
                self.p[i].add_velocity(d0, 0.0, d1);
            }
        }
        self.p[i].l.player_post();
    }

    /// `processUseEntity` with action ATTACK, then
    /// `EntityPlayer.attackTargetEntityWithCurrentItem`.
    fn use_entity_attack(&mut self, a: usize, world: &World, rng: &mut Rng, out: &mut Outbox) {
        let v = 1 - a;
        self.attacks_received[a] += 1;
        let pa = self.p[a].l.body.pos;
        let pv = self.p[v].l.body.pos;
        let seen = !world.blocked([pa[0], pa[1] + EYE_HEIGHT, pa[2]], [pv[0], pv[1] + EYE_HEIGHT, pv[2]]);
        let limit = if seen { 36.0 } else { 9.0 };
        let dsq = dist_sq(pa, pv);
        if !(dsq < limit) {
            return;
        }
        let mut f = self.cfg.damage;
        let f1 = self.cfg.ench_damage;
        let mut i = self.cfg.kb_ench;
        if self.p[a].l.sprinting {
            i += 1;
        }
        if f > 0.0 || f1 > 0.0 {
            let crit = self.p[a].l.body.fall_distance > 0.0 && !self.p[a].l.body.on_ground;
            if crit && f > 0.0 {
                f *= 1.5;
            }
            f += f1;
            let saved = self.p[v].l.body.motion;
            let n_hits = self.hits.len();
            let flag2 = self.attack_entity_from(v, f, Some(a), rng, out);
            if flag2 {
                if i > 0 {
                    let yaw = self.p[a].l.yaw;
                    let ax = (-mth::sin(yaw * mth::PI_F / 180.0f32) * i as f32 * self.kb.extra_horizontal) as f64;
                    let az = (mth::cos(yaw * mth::PI_F / 180.0f32) * i as f32 * self.kb.extra_horizontal) as f64;
                    let ev = self.kb.extra_vertical;
                    self.p[v].add_velocity(ax, ev, az);
                    self.p[a].l.body.motion[0] *= 0.6;
                    self.p[a].l.body.motion[2] *= 0.6;
                    self.p[a].set_sprinting(false);
                }
                if self.p[v].velocity_changed {
                    let mut m = self.p[v].l.body.motion;
                    let mut sprint_kb = i > 0;
                    if let (Some(k), true) = (self.plugin, self.hits.len() > n_hits) {
                        // The plugin replaces the velocity in the event
                        // the server raises before sending it.
                        let (vel, sp) = self.plugin_velocity(&k, a, v, i > self.cfg.kb_ench, rng);
                        m = vel;
                        sprint_kb = sp;
                    }
                    let enc = encode_velocity(m);
                    out.push((v, S2C::Velocity { own: true, v: enc }));
                    self.p[v].velocity_changed = false;
                    self.p[v].l.body.motion = saved;
                    if self.hits.len() > n_hits {
                        let h = self.hits.last_mut().unwrap();
                        h.sprint = sprint_kb;
                        h.crit = crit;
                        h.kb = [enc[0] as f64 / 8000.0, enc[1] as f64 / 8000.0, enc[2] as f64 / 8000.0];
                        h.dist = dsq.sqrt();
                    }
                }
            }
        }
    }

    /// The velocity a `KbPlugin` sends for a hit of `a` on `v`, and whether
    /// it carried sprint knockback. `server_sprint` is the attacker's
    /// server-side sprint flag as it was when the hit was made.
    fn plugin_velocity(&mut self, k: &KbPlugin, a: usize, v: usize, server_sprint: bool, rng: &mut Rng) -> (V3, bool) {
        let pa = self.p[a].l.body.pos;
        let pv = self.p[v].l.body.pos;
        let (mut dx, mut dz) = (pv[0] - pa[0], pv[2] - pa[2]);
        while (dx * dx + dz * dz).sqrt() < 1.0E-4 {
            let r = (rng.f64() - rng.f64()) * 0.01;
            dx = r;
            dz = r;
        }
        let dist = (dx * dx + dz * dz).sqrt();
        // The ground state is the one the victim's client last reported.
        let (h, up) = if self.p[v].l.body.on_ground { (k.h_ground, k.v_ground) } else { (k.h_air, k.v_air) };
        let mut vel = [dx / dist * h, up, dz / dist * h];
        let sprint = if k.stop_sprint { server_sprint } else { self.p[a].client_sprint };
        let level = self.cfg.kb_ench + sprint as i32;
        if level > 0 {
            let yaw = (self.p[a].l.yaw * 0.017453292f32) as f64;
            vel[0] -= yaw.sin() * level as f64 * k.h_sprint;
            vel[1] += k.v_sprint;
            vel[2] += yaw.cos() * level as f64 * k.h_sprint;
        }
        if k.y_limit >= 0.0 {
            if pv[1] - self.p[v].last_ground_y > k.y_limit {
                self.p[v].touched_top = true;
            }
            if self.p[v].touched_top {
                vel[1] = k.v_after_limit;
            }
        }
        (vel, level > 0)
    }

    /// `EntityLivingBase.attackEntityFrom` as reached through
    /// `EntityPlayerMP` and `EntityPlayer`.
    fn attack_entity_from(&mut self, v: usize, amount: f32, attacker: Option<usize>, rng: &mut Rng, out: &mut Outbox) -> bool {
        if self.p[v].health <= 0.0 || amount == 0.0 {
            return false;
        }
        let mut flag = true;
        let max = self.cfg.max_no_damage;
        if (self.p[v].hurt_resistant_time as f32) > max as f32 / 2.0f32 {
            if amount <= self.p[v].last_damage {
                return false;
            }
            let extra = amount - self.p[v].last_damage;
            self.damage_entity(v, extra);
            self.p[v].last_damage = amount;
            flag = false;
        } else {
            self.p[v].last_damage = amount;
            self.p[v].hurt_resistant_time = max;
            self.damage_entity(v, amount);
            self.p[v].hurt_time = 10;
        }
        if flag {
            // setEntityState(this, 2): tracking players first, then self.
            out.push((1 - v, S2C::Hurt { own: false }));
            out.push((v, S2C::Hurt { own: true }));
            // setBeenAttacked
            self.p[v].velocity_changed = true;
            if let Some(a) = attacker {
                let mut d1 = self.p[a].l.body.pos[0] - self.p[v].l.body.pos[0];
                let mut d0 = self.p[a].l.body.pos[2] - self.p[v].l.body.pos[2];
                while d1 * d1 + d0 * d0 < 1.0E-4 {
                    d1 = (rng.f64() - rng.f64()) * 0.01;
                    d0 = (rng.f64() - rng.f64()) * 0.01;
                }
                self.knock_back(v, d1, d0);
                self.hits.push(Hit { attacker: a, victim: v, sprint: false, crit: false, kb: [0.0; 3], dist: 0.0 });
            }
        }
        true
    }

    fn damage_entity(&mut self, v: usize, amount: f32) {
        if !self.cfg.infinite_health && amount != 0.0 {
            let p = &mut self.p[v];
            let h = (p.health - amount).clamp(0.0, 20.0);
            if h != p.health {
                p.health = h;
                // Health is a watched value, so the tracker entry updates
                // on this tick even when the counter is odd.
                p.meta_dirty = true;
            }
        }
    }

    /// `EntityLivingBase.knockBack`.
    fn knock_back(&mut self, v: usize, x: f64, z: f64) {
        let kb = self.kb;
        let p = &mut self.p[v];
        p.is_airborne = true;
        let f = mth::sqrt_d(x * x + z * z);
        let m = &mut p.l.body.motion;
        m[0] /= kb.friction;
        m[1] /= kb.friction;
        m[2] /= kb.friction;
        m[0] -= x / f as f64 * kb.horizontal;
        m[1] += kb.vertical;
        m[2] -= z / f as f64 * kb.horizontal;
        if m[1] > kb.vertical_limit {
            m[1] = kb.vertical_limit;
        }
    }

    /// The part of a server tick after the packet handlers:
    /// `EntityPlayerMP.onUpdate` for both players, the entity tracker, then
    /// the network tick.
    pub fn tick(&mut self, out: &mut Outbox) {
        for p in self.p.iter_mut() {
            if p.hurt_resistant_time > 0 {
                p.hurt_resistant_time -= 1;
            }
            p.chunk = chunk_of(p.l.body.pos);
        }
        for i in 0..2 {
            self.tracker_update(i, out);
        }
        // NetHandlerPlayServer.update
        for p in self.p.iter_mut() {
            p.net_ticks += 1;
        }
    }

    /// `EntityTrackerEntry.updatePlayerList` for player `i`, update
    /// frequency 2, no velocity updates.
    fn tracker_update(&mut self, i: usize, out: &mut Outbox) {
        let other = 1 - i;
        let p = &mut self.p[i];
        if p.update_counter % 2 == 0 || p.is_airborne || p.meta_dirty {
            p.ticks_since_teleport += 1;
            let pos = p.l.body.pos;
            let k = mth::floor_d(pos[0] * 32.0);
            let j1 = mth::floor_d(pos[1] * 32.0);
            let k1 = mth::floor_d(pos[2] * 32.0);
            let l1 = mth::floor_f(p.l.yaw * 256.0f32 / 360.0f32);
            let i2 = mth::floor_f(p.l.pitch * 256.0f32 / 360.0f32);
            let j2 = k - p.enc_pos[0];
            let k2 = j1 - p.enc_pos[1];
            let ii = k1 - p.enc_pos[2];
            let flag = j2.abs() >= 4 || k2.abs() >= 4 || ii.abs() >= 4 || p.update_counter % 60 == 0;
            let flag1 = (l1 - p.enc_yaw).abs() >= 4 || (i2 - p.enc_pitch).abs() >= 4;
            let og = p.l.body.on_ground;
            if p.update_counter > 0 {
                let in_range = |v: i32| (-128..128).contains(&v);
                if in_range(j2) && in_range(k2) && in_range(ii) && p.ticks_since_teleport <= 400 && p.tracker_on_ground == og {
                    if !flag || !flag1 {
                        if flag {
                            out.push((other, S2C::RelMove { d: [j2 as i8, k2 as i8, ii as i8], on_ground: og }));
                        } else if flag1 {
                            out.push((other, S2C::Look { yaw: l1 as i8, pitch: i2 as i8, on_ground: og }));
                        }
                    } else {
                        out.push((
                            other,
                            S2C::LookMove { d: [j2 as i8, k2 as i8, ii as i8], yaw: l1 as i8, pitch: i2 as i8, on_ground: og },
                        ));
                    }
                } else {
                    p.tracker_on_ground = og;
                    p.ticks_since_teleport = 0;
                    out.push((other, S2C::Teleport { p: [k, j1, k1], yaw: l1 as i8, pitch: i2 as i8, on_ground: og }));
                }
            }
            // sendMetadataToAllAssociatedPlayers: tracking players, then self.
            if p.meta_dirty {
                p.meta_dirty = false;
                out.push((other, S2C::Meta { own: false, sprinting: p.l.sprinting }));
                out.push((i, S2C::Meta { own: true, sprinting: p.l.sprinting }));
            }
            if p.attr_dirty {
                p.attr_dirty = false;
                let a = p.l.attr;
                out.push((other, S2C::Props { own: false, base: a.base, speed_amp: a.speed_amp, sprint: a.sprint }));
                out.push((i, S2C::Props { own: true, base: a.base, speed_amp: a.speed_amp, sprint: a.sprint }));
            }
            if flag {
                p.enc_pos = [k, j1, k1];
            }
            if flag1 {
                p.enc_yaw = l1;
                p.enc_pitch = i2;
            }
            p.is_airborne = false;
        }
        p.update_counter += 1;
        if p.velocity_changed {
            let enc = encode_velocity(p.l.body.motion);
            p.velocity_changed = false;
            out.push((other, S2C::Velocity { own: false, v: enc }));
            out.push((i, S2C::Velocity { own: true, v: enc }));
        }
    }
}
