//! One player farming mob chests on a server it reaches through a link
//! with latency.
//!
//! The server owns the mobs, the chests, every projectile and the player's
//! health; the client owns the player's movement, as in the game. Both tick
//! at 20 Hz. What the client sends reaches the server `up` ticks later, and
//! what the server sends reaches the client `down` ticks later, so the
//! client sees the mobs and the holograms as they were, and a knockback (a
//! mob's hit, or the player's own Punch arrow) moves it a round trip after
//! the move that caused it.
use crate::aabb::{dist, Aabb, V3, EYE_HEIGHT};
use crate::living::Living;
use crate::mth::{self, Rng};
use crate::world::{RayHit, World};
use std::collections::VecDeque;

/// Ticks in 10 s: the time a wave has before its chest resets.
pub const WAVE_TICKS: i64 = 200;
/// Ticks in 3 min: the cooldown of a chest after its reward is claimed.
pub const CLAIM_CD_TICKS: i64 = 3600;
/// A chest spawns its wave only with a player this close.
pub const SPAWN_RADIUS: f64 = 12.0;
pub const MOBS_PER_CHEST: usize = 5;
/// Survival reach: 3 blocks for entities, 4.5 for blocks.
pub const ENTITY_REACH: f64 = 3.0;
pub const BLOCK_REACH: f64 = 4.5;
pub const EAT_TICKS: i32 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MobKind {
    /// Walks at the player and hits.
    Zombie = 0,
    /// Fast, keeps its distance and shoots arrows.
    Horseman = 1,
    /// Slow, throws snowballs that hurt and push.
    Golem = 2,
    /// Small; climbs onto the player's head and hits from there.
    Rider = 3,
}
pub const MOB_KINDS: usize = 4;

impl MobKind {
    pub fn from(i: usize) -> MobKind {
        match i {
            0 => MobKind::Zombie,
            1 => MobKind::Horseman,
            2 => MobKind::Golem,
            _ => MobKind::Rider,
        }
    }
    /// Half width and height of the box.
    pub fn size(self) -> (f64, f64) {
        match self {
            MobKind::Zombie => (0.3, 1.95),
            MobKind::Horseman => (0.7, 2.6),
            MobKind::Golem => (0.35, 1.9),
            MobKind::Rider => (0.2, 0.98),
        }
    }
}

/// What one mob of a wave is like. Server plugins make them tougher and
/// nastier than the vanilla ones; every number is drawn per wave.
#[derive(Clone, Copy, Debug)]
pub struct MobStats {
    pub hp: f32,
    /// Damage of a melee hit to the player, after armour.
    pub dmg: f32,
    pub kb_resist: f64,
    /// Push of a melee hit beyond the game's own 0.4.
    pub kb_extra: f64,
    pub speed: f64,
    pub attack_cd: i32,
    pub ranged_cd: (i32, i32),
    pub proj_dmg: f32,
    pub proj_punch: f64,
    pub slow_prob: f64,
    pub slow_ticks: i32,
    pub blind_prob: f64,
    pub blind_ticks: i32,
}

#[derive(Clone, Debug)]
pub struct Mob {
    pub id: u32,
    pub kind: MobKind,
    pub l: Living,
    pub hp: f32,
    pub st: MobStats,
    pub chest: usize,
    pub attack_cd: i32,
    pub ranged_cd: i32,
    pub hurt_resistant: i32,
    pub last_damage: f32,
    /// Red flash after a hit that went through (`hurtTime`), seen by the
    /// client.
    pub hurt_time: i32,
    pub alive: bool,
    /// A Rider sitting on the player's head.
    pub mounted: bool,
    pub strafe_dir: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjKind {
    Arrow,
    Snowball,
    Pearl,
    Potion,
}

#[derive(Clone, Copy, Debug)]
pub struct Proj {
    pub kind: ProjKind,
    /// -1 for the player, otherwise the index of the mob that shot it.
    pub owner: i32,
    pub pos: V3,
    pub motion: V3,
    pub ticks_in_air: i32,
    pub dmg: f64,
    pub punch: f64,
    pub crit: bool,
    pub alive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChestState {
    /// Counting down to the next wave.
    Cooldown = 0,
    /// Timer at zero, waiting for a player within 12 blocks.
    Ready = 1,
    /// Mobs alive; the hologram is red.
    Active = 2,
    /// Wave defeated: "Click here to claim the rewards!".
    Claimable = 3,
}

#[derive(Clone, Debug)]
pub struct Chest {
    pub block: [i32; 3],
    pub state: ChestState,
    pub ready_at: i64,
    pub active_since: i64,
    pub mobs: Vec<usize>,
    pub claims: u32,
    pub resets: u32,
    /// The running wave was set off by another player, who will kill it
    /// and take the reward.
    pub rival_wave: bool,
}

impl Chest {
    pub fn center(&self) -> V3 {
        [self.block[0] as f64 + 0.5, self.block[1] as f64 + 0.5, self.block[2] as f64 + 0.5]
    }
    /// The number the hologram shows: whole seconds to the next wave.
    pub fn shown_secs(&self, now: i64) -> i32 {
        match self.state {
            ChestState::Cooldown => (((self.ready_at - now).max(0) + 19) / 20) as i32,
            _ => 0,
        }
    }
}

/// What the server tells the client, other than the world snapshot.
#[derive(Clone, Copy, Debug)]
pub enum S2C {
    /// `S12PacketEntityVelocity` for the player itself.
    Velocity(V3),
    /// `S08PacketPlayerPosLook` (pearl landing).
    Teleport(V3),
}

#[derive(Clone, Copy, Debug)]
pub enum C2S {
    Pos { pos: V3, yaw: f32, pitch: f32, on_ground: bool },
    /// `slot`: what the hand held (the real client sends its slot before an attack).
    Attack { mob: u32, crit: bool, sprint: bool, yaw: f32, slot: u8 },
    Claim { chest: usize },
    StartUse { item: u8 },
    StopUse,
    /// Bow released after this many ticks of drawing.
    Shoot { ticks: i32 },
    ThrowPearl,
    ThrowPotion,
}

/// What the client sees of one mob.
#[derive(Clone, Copy, Debug)]
pub struct MobView {
    pub id: u32,
    pub kind: MobKind,
    pub pos: V3,
    pub motion: V3,
    pub hp: f32,
    pub max_hp: f32,
    pub hurt_time: i32,
    pub mounted: bool,
    pub hw: f64,
    pub h: f64,
}

/// What the client sees of the server in one tick.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub tick: i64,
    pub mobs: Vec<MobView>,
    pub projs: Vec<(ProjKind, i32, V3, V3)>,
    /// Hologram of each chest: state and shown seconds.
    pub holo: Vec<(ChestState, i32)>,
    pub hp: f32,
    pub absorption: f32,
    pub regen_left: i32,
    pub slow_left: i32,
    pub blind_left: i32,
    pub gapples: u32,
    pub pearls: u32,
    pub potions: u32,
    pub arrows: u32,
    pub pearl_cd_left: i32,
    pub rivals: Vec<V3>,
}

/// Things that happened on the server in one tick, for the reward and
/// the statistics.
#[derive(Clone, Copy, Debug, Default)]
pub struct Events {
    pub claims: u32,
    pub kills: u32,
    pub waves: u32,
    pub resets: u32,
    pub dmg_dealt: f32,
    pub dmg_taken: f32,
    pub leash_tps: u32,
    pub hits: u32,
    pub self_punch: u32,
    pub punch_boost: f64,
    pub gapples: u32,
    pub gapple_waste: u32,
    pub potions: u32,
    pub potion_heal: f32,
    pub potion_waste: u32,
    pub pearls: u32,
    pub died: bool,
    pub max_simul: u32,
    /// Ticks spent within 12 blocks of a chest another player is at.
    pub rival_near: u32,
}

#[derive(Clone, Debug)]
pub struct Cfg {
    pub up: i64,
    pub down: i64,
    pub jitter: i64,
    pub speed_amp: i8,
    pub cps: f64,
    pub sword_dmg: f32,
    pub punch: f64,
    pub arrow_self_dmg_mult: f32,
    pub leash_r: f64,
    pub leash_every: i64,
    pub reset_cd: (i64, i64),
    pub pearl_cd: i64,
    pub pearl_dmg: f32,
    pub holo_range: f64,
    pub kind_weights: [f64; MOB_KINDS],
    pub mob_hp: (f32, f32),
    pub mob_dmg: (f32, f32),
    pub mob_kb_resist: (f64, f64),
    pub mob_kb_extra: (f64, f64),
    pub mob_speed: (f64, f64),
    pub mob_attack_cd: (i32, i32),
    pub ranged_cd: (i32, i32),
    pub proj_dmg: (f32, f32),
    pub proj_punch: (f64, f64),
    pub slow_prob: f64,
    pub blind_prob: f64,
    pub effect_ticks: (i32, i32),
}

#[derive(Clone, Debug)]
pub struct ServerPlayer {
    pub pos: V3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub motion: V3,
    pub hp: f32,
    pub absorption: f32,
    pub abs_until: i64,
    pub regen_until: i64,
    pub slow_until: i64,
    pub blind_until: i64,
    pub hurt_resistant: i32,
    pub last_damage: f32,
    pub gapples: u32,
    pub pearls: u32,
    pub potions: u32,
    pub arrows: u32,
    pub pearl_cd_until: i64,
    pub using: Option<u8>,
    pub use_ticks: i32,
    pub fall_distance: f32,
    pub velocity_changed: bool,
}

impl ServerPlayer {
    pub fn bb(&self) -> Aabb {
        Aabb::player(self.pos)
    }
}

/// The player as its own game client runs it.
#[derive(Clone, Debug)]
pub struct ClientPlayer {
    pub l: Living,
    /// Hotbar slot: 0 sword, 1 bow, 2 golden apples, 3 pearls, 4 potions.
    pub slot: u8,
    pub using: bool,
    pub use_ticks: i32,
    pub tokens: f64,
    /// Slot to go back to on the next tick after a pearl or potion throw.
    pub back_to: Option<u8>,
    pub clicks: u32,
}

/// Another player farming the same chests. Played by a script: it walks
/// to a chest, stays there a while, kills what spawns and takes the reward.
#[derive(Clone, Debug)]
pub struct Rival {
    pub pos: V3,
    pub chest: usize,
    pub moving: bool,
    pub stay_until: i64,
    pub next_kill: i64,
}

pub struct Game {
    pub rivals: Vec<Rival>,
    pub world: World,
    pub cfg: Cfg,
    pub rng: Rng,
    pub t: i64,
    pub mobs: Vec<Mob>,
    pub next_id: u32,
    pub projs: Vec<Proj>,
    pub chests: Vec<Chest>,
    pub sp: ServerPlayer,
    pub cp: ClientPlayer,
    up_q: VecDeque<(i64, C2S)>,
    down_q: VecDeque<(i64, S2C)>,
    snaps: VecDeque<Snapshot>,
    pub view: Snapshot,
    pub ev: Events,
    pub dead: bool,
}

/// `Entity.setThrowableHeading` / `EntityArrow.setThrowableHeading`.
fn heading(rng: &mut Rng, d: V3, velocity: f64, inaccuracy: f64, arrow: bool) -> V3 {
    let f = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-9);
    let mut v = [d[0] / f, d[1] / f, d[2] / f];
    for c in v.iter_mut() {
        let sign = if arrow && rng.chance(0.5) { -1.0 } else { 1.0 };
        *c += rng.gaussian() * sign * 0.007499999832361937 * inaccuracy;
        *c *= velocity;
    }
    v
}

fn look(yaw: f32, pitch: f32) -> V3 {
    let (y, p) = (yaw as f64 * std::f64::consts::PI / 180.0, pitch as f64 * std::f64::consts::PI / 180.0);
    [-y.sin() * p.cos(), -p.sin(), y.cos() * p.cos()]
}

fn wrap(a: f32) -> f32 {
    let mut a = a % 360.0;
    if a >= 180.0 {
        a -= 360.0
    }
    if a < -180.0 {
        a += 360.0
    }
    a
}

impl Game {
    pub fn new(world: World, chest_blocks: &[[i32; 3]], spawn: V3, cfg: Cfg, rng: Rng, timers: &[i64], start: PlayerStart) -> Game {
        let chests = chest_blocks
            .iter()
            .zip(timers.iter())
            .map(|(b, &t)| Chest { block: *b, state: if t <= 0 { ChestState::Ready } else { ChestState::Cooldown }, ready_at: t.max(0), active_since: 0, mobs: Vec::new(), claims: 0, resets: 0, rival_wave: false })
            .collect();
        let yaw = start.yaw;
        let mut l = Living::new(spawn, yaw, 0.0, cfg.speed_amp);
        l.body.on_ground = true;
        let sp = ServerPlayer {
            pos: spawn,
            yaw,
            pitch: 0.0,
            on_ground: true,
            motion: [0.0; 3],
            hp: start.hp,
            absorption: 0.0,
            abs_until: 0,
            regen_until: 0,
            slow_until: 0,
            blind_until: 0,
            hurt_resistant: 0,
            last_damage: 0.0,
            gapples: start.gapples,
            pearls: start.pearls,
            potions: start.potions,
            arrows: start.arrows,
            pearl_cd_until: 0,
            using: None,
            use_ticks: 0,
            fall_distance: 0.0,
            velocity_changed: false,
        };
        let cp = ClientPlayer { l, slot: 0, using: false, use_ticks: 0, tokens: 1.0, back_to: None, clicks: 0 };
        let mut g = Game {
            rivals: Vec::new(),
            world,
            cfg,
            rng,
            t: 0,
            mobs: Vec::new(),
            next_id: 1,
            projs: Vec::new(),
            chests,
            sp,
            cp,
            up_q: VecDeque::new(),
            down_q: VecDeque::new(),
            snaps: VecDeque::new(),
            view: Snapshot::default(),
            ev: Events::default(),
            dead: false,
        };
        let s = g.snapshot();
        g.view = s.clone();
        g.snaps.push_back(s);
        g
    }

    fn send_up(&mut self, p: C2S) {
        let j = if self.cfg.jitter > 0 { (self.rng.f64() * (self.cfg.jitter + 1) as f64) as i64 } else { 0 };
        // Packets of one connection keep their order.
        let at = (self.t + self.cfg.up + j).max(self.up_q.back().map(|b| b.0).unwrap_or(0));
        self.up_q.push_back((at, p));
    }

    fn send_down(&mut self, p: S2C) {
        let at = (self.t + self.cfg.down).max(self.down_q.back().map(|b| b.0).unwrap_or(0));
        self.down_q.push_back((at, p));
    }

    // ------------------------------------------------------------------
    // Client
    // ------------------------------------------------------------------

    /// What the crosshair is on: the nearest mob box within entity reach,
    /// else the chest block within block reach. Uses what the client sees.
    pub fn pick(&self) -> (Option<u32>, Option<usize>) {
        let l = &self.cp.l;
        let eye = [l.body.pos[0], l.body.pos[1] + EYE_HEIGHT, l.body.pos[2]];
        let v = look(l.yaw, l.pitch);
        let far = [eye[0] + v[0] * BLOCK_REACH, eye[1] + v[1] * BLOCK_REACH, eye[2] + v[2] * BLOCK_REACH];
        let (block_d, block_cell) = match self.world.ray_trace_blocks(eye, far, false) {
            RayHit::Block(p, c) => (dist(eye, p), Some(c)),
            _ => (BLOCK_REACH + 1.0, None),
        };
        let mut best: Option<(f64, u32)> = None;
        let ent_end = [eye[0] + v[0] * ENTITY_REACH, eye[1] + v[1] * ENTITY_REACH, eye[2] + v[2] * ENTITY_REACH];
        for m in &self.view.mobs {
            let bb = Aabb::new([m.pos[0] - m.hw - 0.1, m.pos[1] - 0.1, m.pos[2] - m.hw - 0.1], [m.pos[0] + m.hw + 0.1, m.pos[1] + m.h + 0.1, m.pos[2] + m.hw + 0.1]);
            let d = if bb.is_vec_inside(eye) {
                Some(0.0)
            } else {
                bb.calculate_intercept(eye, ent_end).map(|p| dist(eye, p))
            };
            if let Some(d) = d {
                if d <= ENTITY_REACH && d < block_d && best.map_or(true, |b| d < b.0) {
                    best = Some((d, m.id));
                }
            }
        }
        if let Some((_, id)) = best {
            return (Some(id), None);
        }
        let chest = block_cell.and_then(|c| self.chests.iter().position(|ch| ch.block == c));
        (None, chest)
    }

    /// One client tick with the controls the agent chose.
    pub fn client_tick(&mut self, inp: &Input) {
        // Packets from the server that have arrived.
        while let Some(&(at, p)) = self.down_q.front() {
            if at > self.t {
                break;
            }
            self.down_q.pop_front();
            match p {
                S2C::Velocity(v) => self.cp.l.body.motion = v,
                S2C::Teleport(p) => {
                    self.cp.l.body.set_position(p);
                    self.cp.l.body.motion = [0.0; 3];
                }
            }
        }
        // The newest snapshot that has arrived.
        while self.snaps.len() > 1 && self.snaps[1].tick + self.cfg.down <= self.t {
            self.snaps.pop_front();
        }
        if let Some(s) = self.snaps.front() {
            if s.tick + self.cfg.down <= self.t {
                self.view = s.clone();
            }
        }
        let blind = self.view.blind_left > 0;
        self.cp.l.attr.slow_amp = if self.view.slow_left > 0 { 9 } else { -1 };

        // Mouse.
        let l = &mut self.cp.l;
        l.yaw += inp.dyaw;
        l.pitch = (l.pitch + inp.dpitch).clamp(-90.0, 90.0);

        // Hotbar: a pearl or potion throw is the number key, a right click
        // and the number key of the slot before, on the next tick.
        if let Some(b) = self.cp.back_to.take() {
            self.cp.slot = b;
        }
        let mut throw: Option<u8> = None;
        if inp.slot != 255 && inp.slot != self.cp.slot && !self.cp.using {
            if inp.slot >= 3 {
                let have = if inp.slot == 3 { self.view.pearls > 0 && self.view.pearl_cd_left == 0 } else { self.view.potions > 0 };
                if have {
                    throw = Some(inp.slot);
                    self.cp.back_to = Some(self.cp.slot);
                    self.cp.slot = inp.slot;
                }
            } else {
                self.cp.slot = inp.slot;
            }
        }
        if let Some(s) = throw {
            self.send_up(if s == 3 { C2S::ThrowPearl } else { C2S::ThrowPotion });
        }

        // Clicks: at most `cps` a second.
        self.cp.tokens = (self.cp.tokens + self.cfg.cps / 20.0).min(1.0);
        let (pick_mob, pick_chest) = self.pick();
        if inp.attack && !self.cp.using && self.cp.tokens >= 1.0 && throw.is_none() {
            self.cp.tokens -= 1.0;
            self.cp.clicks += 1;
            if let Some(id) = pick_mob {
                let l = &mut self.cp.l;
                let crit = l.body.fall_distance > 0.0 && !l.body.on_ground && !self.world.in_water(&l.body.bb) && l.body.motion[1] < 0.0;
                let sprint = l.sprinting;
                let yaw = l.yaw;
                if sprint {
                    // The client's half of the sprint hit: slowed and the
                    // sprint dropped (`attackTargetEntityWithCurrentItem`).
                    l.body.motion[0] *= 0.6;
                    l.body.motion[2] *= 0.6;
                    l.sprinting = false;
                    l.attr.sprint = false;
                }
                let slot = self.cp.slot;
                self.send_up(C2S::Attack { mob: id, crit, sprint, yaw, slot });
            } else if let Some(c) = pick_chest {
                self.send_up(C2S::Claim { chest: c });
            }
        }
        // Right button.
        if inp.use_ && throw.is_none() {
            if !self.cp.using {
                if let Some(c) = pick_chest.filter(|_| pick_mob.is_none()) {
                    self.send_up(C2S::Claim { chest: c });
                } else if (self.cp.slot == 1 && self.view.arrows > 0) || (self.cp.slot == 2 && self.view.gapples > 0) {
                    self.cp.using = true;
                    self.cp.use_ticks = 0;
                    self.send_up(C2S::StartUse { item: self.cp.slot });
                }
            }
        } else if self.cp.using {
            self.cp.using = false;
            if self.cp.slot == 1 {
                self.send_up(C2S::Shoot { ticks: self.cp.use_ticks });
            } else {
                self.send_up(C2S::StopUse);
            }
        }
        if self.cp.using {
            self.cp.use_ticks += 1;
            if self.cp.slot == 2 && self.cp.use_ticks >= EAT_TICKS {
                // Eaten: the server finishes it on its own count.
                self.cp.using = false;
            }
        }

        // Movement (`EntityPlayerSP.onLivingUpdate`).
        let using = self.cp.using;
        let l = &mut self.cp.l;
        let (mut strafe, mut forward) = (inp.strafe, inp.forward);
        if using {
            strafe *= 0.2;
            forward *= 0.2;
        }
        let can_sprint = forward >= 0.8 && !using && !blind;
        if inp.sprint && can_sprint && !l.sprinting {
            l.sprinting = true;
        }
        if l.sprinting && (forward < 0.8 || l.body.collided_h || using) {
            l.sprinting = false;
        }
        l.attr.sprint = l.sprinting;
        // Mobs push the player apart from them.
        let me = l.body.bb;
        for m in &self.view.mobs {
            if m.mounted {
                continue;
            }
            let bb = Aabb::new([m.pos[0] - m.hw, m.pos[1], m.pos[2] - m.hw], [m.pos[0] + m.hw, m.pos[1] + m.h, m.pos[2] + m.hw]);
            if bb.intersects(&me) {
                let (dx, dz) = (l.body.pos[0] - m.pos[0], l.body.pos[2] - m.pos[2]);
                let d = dx.abs().max(dz.abs());
                if d >= 0.01 {
                    let s = d.sqrt();
                    let k = (1.0 / s).min(1.0) * 0.05 / s;
                    l.body.motion[0] += dx * k;
                    l.body.motion[2] += dz * k;
                }
            }
        }
        l.living_update(&self.world, strafe, forward, inp.jump, true);
        l.player_post();
        // No fall damage on this server.
        if l.body.on_ground {
            l.body.fall_distance = 0.0;
        }
        let p = C2S::Pos { pos: l.body.pos, yaw: l.yaw, pitch: l.pitch, on_ground: l.body.on_ground };
        self.send_up(p);
    }

    // ------------------------------------------------------------------
    // Server
    // ------------------------------------------------------------------

    fn spawn_wave(&mut self, ci: usize) {
        let c = self.chests[ci].center();
        let cfg = self.cfg.clone();
        for _ in 0..MOBS_PER_CHEST {
            let mut r = self.rng.f64() * cfg.kind_weights.iter().sum::<f64>();
            let mut k = 0;
            while k + 1 < MOB_KINDS && r >= cfg.kind_weights[k] {
                r -= cfg.kind_weights[k];
                k += 1;
            }
            let kind = MobKind::from(k);
            let rng = &mut self.rng;
            let mut st = MobStats {
                hp: rng.range(cfg.mob_hp.0 as f64, cfg.mob_hp.1 as f64) as f32,
                dmg: rng.range(cfg.mob_dmg.0 as f64, cfg.mob_dmg.1 as f64) as f32,
                kb_resist: rng.range(cfg.mob_kb_resist.0, cfg.mob_kb_resist.1),
                kb_extra: rng.range(cfg.mob_kb_extra.0, cfg.mob_kb_extra.1),
                speed: rng.range(cfg.mob_speed.0, cfg.mob_speed.1),
                attack_cd: rng.range(cfg.mob_attack_cd.0 as f64, cfg.mob_attack_cd.1 as f64 + 1.0) as i32,
                ranged_cd: cfg.ranged_cd,
                proj_dmg: rng.range(cfg.proj_dmg.0 as f64, cfg.proj_dmg.1 as f64) as f32,
                proj_punch: rng.range(cfg.proj_punch.0, cfg.proj_punch.1),
                slow_prob: if rng.chance(0.5) { cfg.slow_prob } else { 0.0 },
                slow_ticks: rng.range(cfg.effect_ticks.0 as f64, cfg.effect_ticks.1 as f64) as i32,
                blind_prob: if rng.chance(0.5) { cfg.blind_prob } else { 0.0 },
                blind_ticks: rng.range(cfg.effect_ticks.0 as f64, cfg.effect_ticks.1 as f64) as i32,
            };
            match kind {
                MobKind::Horseman => st.speed *= 1.6,
                MobKind::Golem => {
                    st.speed *= 0.7;
                    st.hp *= 0.8;
                }
                MobKind::Rider => {
                    st.hp *= 0.6;
                    st.speed *= 1.3;
                }
                _ => {}
            }
            let (hw, h) = kind.size();
            let mut pos = c;
            for _ in 0..10 {
                let a = rng.range(0.0, std::f64::consts::TAU);
                let d = rng.range(1.0, 2.5);
                let (x, z) = (c[0] + a.cos() * d, c[2] + a.sin() * d);
                let gy = self.world.ground(x.floor() as i32, z.floor() as i32);
                let p = [x, gy as f64, z];
                let bb = Aabb::new([p[0] - hw, p[1], p[2] - hw], [p[0] + hw, p[1] + h, p[2] + hw]);
                if !self.world.collides(&bb) && (gy as f64 - c[1]).abs() < 3.0 {
                    pos = p;
                    break;
                }
            }
            if pos == c {
                pos = [c[0], c[1] + 0.5, c[2]];
            }
            let mut l = Living::new(pos, rng.range(-180.0, 180.0) as f32, 0.0, -1);
            l.body = crate::body::Body::sized(pos, hw, h);
            l.attr.base = st.speed;
            let rc = rng.range(st.ranged_cd.0 as f64, st.ranged_cd.1 as f64) as i32;
            let sd = if rng.chance(0.5) { 1.0 } else { -1.0 };
            let id = self.next_id;
            self.next_id += 1;
            self.chests[ci].mobs.push(self.mobs.len());
            self.mobs.push(Mob {
                id,
                kind,
                l,
                hp: st.hp,
                st,
                chest: ci,
                attack_cd: 10,
                ranged_cd: rc,
                hurt_resistant: 0,
                last_damage: 0.0,
                hurt_time: 0,
                alive: true,
                mounted: false,
                strafe_dir: sd,
            });
        }
    }

    /// `EntityLivingBase.attackEntityFrom` for the player. Returns whether
    /// the damage went through (and pushed). `from` is where the push
    /// comes from; `None` for a source at the player's own position, which
    /// the game turns into a tiny random direction.
    fn hurt_player(&mut self, amount: f32, from: Option<V3>) -> bool {
        let sp = &mut self.sp;
        let mut push = true;
        let applied;
        if sp.hurt_resistant > 10 {
            if amount <= sp.last_damage {
                return false;
            }
            applied = amount - sp.last_damage;
            sp.last_damage = amount;
            push = false;
        } else {
            applied = amount;
            sp.last_damage = amount;
            sp.hurt_resistant = 20;
        }
        let a = applied.min(sp.absorption);
        sp.absorption -= a;
        let rest = applied - a;
        sp.hp -= rest;
        self.ev.dmg_taken += applied;
        if push {
            let (mut dx, mut dz) = match from {
                Some(f) => (f[0] - sp.pos[0], f[2] - sp.pos[2]),
                None => (0.0, 0.0),
            };
            while dx * dx + dz * dz < 1.0e-4 {
                dx = (self.rng.f64() - self.rng.f64()) * 0.01;
                dz = (self.rng.f64() - self.rng.f64()) * 0.01;
            }
            let f = (dx * dx + dz * dz).sqrt();
            let m = &mut sp.motion;
            m[0] /= 2.0;
            m[1] /= 2.0;
            m[2] /= 2.0;
            m[0] -= dx / f * 0.4;
            m[1] += 0.4;
            m[2] -= dz / f * 0.4;
            if m[1] > 0.4000000059604645 {
                m[1] = 0.4000000059604645;
            }
            sp.velocity_changed = true;
        }
        if sp.hp <= 0.0 {
            self.dead = true;
            self.ev.died = true;
        }
        true
    }

    /// `EntityLivingBase.attackEntityFrom` for a mob, then its push.
    fn hurt_mob(&mut self, mi: usize, amount: f32, from: V3) -> bool {
        let rng_v = self.rng.f64();
        let m = &mut self.mobs[mi];
        if !m.alive {
            return false;
        }
        let mut push = true;
        let applied;
        if m.hurt_resistant > 10 {
            if amount <= m.last_damage {
                return false;
            }
            applied = amount - m.last_damage;
            m.last_damage = amount;
            push = false;
        } else {
            applied = amount;
            m.last_damage = amount;
            m.hurt_resistant = 20;
            m.hurt_time = 10;
        }
        let hp0 = m.hp;
        m.hp -= applied;
        self.ev.dmg_dealt += applied.min(hp0.max(0.0));
        if push && !m.mounted && rng_v >= m.st.kb_resist {
            let (dx, dz) = (from[0] - m.l.body.pos[0], from[2] - m.l.body.pos[2]);
            let f = (dx * dx + dz * dz).sqrt().max(1e-4);
            let mo = &mut m.l.body.motion;
            mo[0] /= 2.0;
            mo[1] /= 2.0;
            mo[2] /= 2.0;
            mo[0] -= dx / f * 0.4;
            mo[1] += 0.4;
            mo[2] -= dz / f * 0.4;
            if mo[1] > 0.4000000059604645 {
                mo[1] = 0.4000000059604645;
            }
        }
        if m.hp <= 0.0 {
            m.alive = false;
            m.mounted = false;
            let ci = m.chest;
            if !self.chests[ci].rival_wave {
                self.ev.kills += 1;
            }
        }
        true
    }

    fn apply_hit_effects(&mut self, st: MobStats) {
        if st.slow_prob > 0.0 && self.rng.chance(st.slow_prob) {
            self.sp.slow_until = self.sp.slow_until.max(self.t + st.slow_ticks as i64);
        }
        if st.blind_prob > 0.0 && self.rng.chance(st.blind_prob) {
            self.sp.blind_until = self.sp.blind_until.max(self.t + st.blind_ticks as i64);
        }
    }

    fn handle(&mut self, p: C2S) {
        match p {
            C2S::Pos { pos, yaw, pitch, on_ground } => {
                let dy = pos[1] - self.sp.pos[1];
                if !on_ground && dy < 0.0 {
                    self.sp.fall_distance -= dy as f32;
                } else if on_ground {
                    self.sp.fall_distance = 0.0;
                }
                self.sp.pos = pos;
                self.sp.yaw = yaw;
                self.sp.pitch = pitch;
                self.sp.on_ground = on_ground;
                if on_ground && self.sp.motion[1] < 0.0 {
                    self.sp.motion[1] = 0.0;
                }
            }
            C2S::Attack { mob, crit, sprint, yaw, slot } => {
                let Some(mi) = self.mobs.iter().position(|m| m.id == mob && m.alive) else { return };
                let mp = self.mobs[mi].l.body.pos;
                let eye = [self.sp.pos[0], self.sp.pos[1] + EYE_HEIGHT, self.sp.pos[2]];
                if dist(eye, mp) > 6.0 {
                    return;
                }
                // Only the sword hits hard; a bow, an apple or a pearl in the hand hits
                // like a fist (1 point), as in 1.8.
                let mut dmg = if slot == 0 { self.cfg.sword_dmg } else { 1.0 };
                if crit {
                    dmg *= 1.5;
                }
                let from = self.sp.pos;
                if self.hurt_mob(mi, dmg, from) {
                    self.ev.hits += 1;
                    if sprint {
                        let f = yaw * std::f32::consts::PI / 180.0;
                        let m = &mut self.mobs[mi];
                        if !m.mounted {
                            m.l.body.motion[0] += (-mth::sin(f) * 0.5) as f64;
                            m.l.body.motion[1] += 0.1;
                            m.l.body.motion[2] += (mth::cos(f) * 0.5) as f64;
                        }
                        self.sp.motion[0] *= 0.6;
                        self.sp.motion[2] *= 0.6;
                    }
                }
            }
            C2S::Claim { chest } => {
                let c = &mut self.chests[chest];
                let eye = [self.sp.pos[0], self.sp.pos[1] + EYE_HEIGHT, self.sp.pos[2]];
                if c.state == ChestState::Claimable && dist(eye, c.center()) <= 6.0 {
                    c.state = ChestState::Cooldown;
                    c.ready_at = self.t + CLAIM_CD_TICKS;
                    c.claims += 1;
                    self.ev.claims += 1;
                }
            }
            C2S::StartUse { item } => {
                self.sp.using = Some(item);
                self.sp.use_ticks = 0;
            }
            C2S::StopUse => self.sp.using = None,
            C2S::Shoot { ticks } => {
                self.sp.using = None;
                if self.sp.arrows == 0 {
                    return;
                }
                let mut f = ticks as f64 / 20.0;
                f = (f * f + f * 2.0) / 3.0;
                if f < 0.1 {
                    return;
                }
                let f = f.min(1.0);
                self.sp.arrows -= 1;
                let (yaw, pitch) = (self.sp.yaw, self.sp.pitch);
                let yr = yaw as f64 / 180.0 * std::f64::consts::PI;
                let pos = [self.sp.pos[0] - yr.cos() * 0.16, self.sp.pos[1] + EYE_HEIGHT - 0.1 - 0.1, self.sp.pos[2] - yr.sin() * 0.16];
                let v = heading(&mut self.rng, look(yaw, pitch), f * 2.0 * 1.5, 1.0, true);
                self.projs.push(Proj { kind: ProjKind::Arrow, owner: -1, pos, motion: v, ticks_in_air: 0, dmg: 2.0, punch: self.cfg.punch, crit: f >= 1.0, alive: true });
            }
            C2S::ThrowPearl => {
                if self.sp.pearls == 0 || self.t < self.sp.pearl_cd_until {
                    return;
                }
                self.sp.pearls -= 1;
                self.sp.pearl_cd_until = self.t + self.cfg.pearl_cd;
                self.ev.pearls += 1;
                self.throw(ProjKind::Pearl, 1.5, 0.0);
            }
            C2S::ThrowPotion => {
                if self.sp.potions == 0 {
                    return;
                }
                self.sp.potions -= 1;
                self.ev.potions += 1;
                if self.sp.hp >= 16.0 {
                    self.ev.potion_waste += 1;
                }
                self.throw(ProjKind::Potion, 0.5, -20.0);
            }
        }
    }

    /// `EntityThrowable(world, thrower)`.
    fn throw(&mut self, kind: ProjKind, velocity: f64, pitch_off: f32) {
        let (yaw, pitch) = (self.sp.yaw, self.sp.pitch);
        let yr = yaw as f64 / 180.0 * std::f64::consts::PI;
        let pos = [self.sp.pos[0] - yr.cos() * 0.16, self.sp.pos[1] + EYE_HEIGHT - 0.1, self.sp.pos[2] - yr.sin() * 0.16];
        let d = look(yaw, pitch + pitch_off);
        let d = [d[0] * 0.4, d[1] * 0.4, d[2] * 0.4];
        let v = heading(&mut self.rng, d, velocity, 1.0, false);
        self.projs.push(Proj { kind, owner: -1, pos, motion: v, ticks_in_air: 0, dmg: 0.0, punch: 0.0, crit: false, alive: true });
    }

    fn tick_projectiles(&mut self) {
        let n = self.projs.len();
        for i in 0..n {
            let mut e = self.projs[i];
            if !e.alive {
                continue;
            }
            e.ticks_in_air += 1;
            let start = e.pos;
            let mut end = [e.pos[0] + e.motion[0], e.pos[1] + e.motion[1], e.pos[2] + e.motion[2]];
            let block = match self.world.ray_trace_blocks(start, end, false) {
                RayHit::Block(p, _) => {
                    end = p;
                    Some(p)
                }
                _ => None,
            };
            // Entities along the way: the player, and the mobs for the
            // player's own shots.
            let grow = if e.kind == ProjKind::Arrow { 0.3 } else { 0.3 };
            let mut hit: Option<(f64, i32)> = None;
            let can_self = e.owner != -1 || e.ticks_in_air >= 5;
            if can_self && !self.dead {
                let bb = self.sp.bb().expand(grow, grow, grow);
                let d = if bb.is_vec_inside(start) { Some(0.0) } else { bb.calculate_intercept(start, end).map(|p| dist(start, p)) };
                if let Some(d) = d {
                    hit = Some((d, -1));
                }
            }
            if e.owner == -1 {
                for (mi, m) in self.mobs.iter().enumerate() {
                    if !m.alive {
                        continue;
                    }
                    let b = m.l.body.bb.expand(grow, grow, grow);
                    let d = if b.is_vec_inside(start) { Some(0.0) } else { b.calculate_intercept(start, end).map(|p| dist(start, p)) };
                    if let Some(d) = d {
                        if hit.map_or(true, |h| d < h.0) {
                            hit = Some((d, mi as i32));
                        }
                    }
                }
            }
            if let Some((_, who)) = hit {
                match e.kind {
                    ProjKind::Arrow => {
                        let f = (e.motion[0] * e.motion[0] + e.motion[1] * e.motion[1] + e.motion[2] * e.motion[2]).sqrt();
                        let mut dmg = (f * e.dmg).ceil() as f32;
                        if e.crit {
                            dmg += (self.rng.f64() * (dmg as f64 / 2.0 + 2.0)) as f32;
                        }
                        let ok = if who == -1 {
                            let from = if e.owner == -1 { None } else { Some(self.mobs[e.owner as usize].l.body.pos) };
                            let mult = if e.owner == -1 { self.cfg.arrow_self_dmg_mult } else { 1.0 };
                            let dd = if e.owner == -1 { dmg * mult } else { self.mobs[e.owner as usize].st.proj_dmg };
                            let ok = self.hurt_player(dd.max(0.01), from);
                            if ok && e.owner != -1 {
                                let st = self.mobs[e.owner as usize].st;
                                self.apply_hit_effects(st);
                            }
                            ok
                        } else {
                            let from = [e.pos[0] - e.motion[0], e.pos[1], e.pos[2] - e.motion[2]];
                            self.hurt_mob(who as usize, dmg, from)
                        };
                        if ok {
                            let f7 = (e.motion[0] * e.motion[0] + e.motion[2] * e.motion[2]).sqrt();
                            if e.punch > 0.0 && f7 > 0.0 {
                                let add = [e.motion[0] * e.punch * 0.6000000238418579 / f7, 0.1, e.motion[2] * e.punch * 0.6000000238418579 / f7];
                                if who == -1 {
                                    for a in 0..3 {
                                        self.sp.motion[a] += add[a];
                                    }
                                    self.sp.velocity_changed = true;
                                    if e.owner == -1 {
                                        self.ev.self_punch += 1;
                                        let m = self.sp.motion;
                                        self.ev.punch_boost += (m[0] * m[0] + m[2] * m[2]).sqrt();
                                    }
                                } else {
                                    let m = &mut self.mobs[who as usize];
                                    if !m.mounted {
                                        for a in 0..3 {
                                            m.l.body.motion[a] += add[a];
                                        }
                                    }
                                }
                            }
                            e.alive = false;
                        } else {
                            // Bounced off.
                            for a in 0..3 {
                                e.motion[a] *= -0.10000000149011612;
                            }
                            e.ticks_in_air = 0;
                        }
                    }
                    ProjKind::Snowball => {
                        if who == -1 && e.owner >= 0 {
                            let st = self.mobs[e.owner as usize].st;
                            let from = self.mobs[e.owner as usize].l.body.pos;
                            if self.hurt_player(st.proj_dmg.max(0.01), Some(from)) {
                                let f7 = (e.motion[0] * e.motion[0] + e.motion[2] * e.motion[2]).sqrt();
                                if f7 > 0.0 && st.proj_punch > 0.0 {
                                    self.sp.motion[0] += e.motion[0] * st.proj_punch * 0.6 / f7;
                                    self.sp.motion[2] += e.motion[2] * st.proj_punch * 0.6 / f7;
                                    self.sp.velocity_changed = true;
                                }
                                self.apply_hit_effects(st);
                            }
                        }
                        e.alive = false;
                    }
                    ProjKind::Pearl => {
                        self.pearl_land(e.pos);
                        e.alive = false;
                    }
                    ProjKind::Potion => {
                        self.splash(end, who == -1);
                        e.alive = false;
                    }
                }
                self.projs[i] = e;
                continue;
            }
            if let Some(p) = block {
                match e.kind {
                    ProjKind::Pearl => self.pearl_land(p),
                    ProjKind::Potion => self.splash(p, false),
                    _ => {}
                }
                e.alive = false;
                self.projs[i] = e;
                continue;
            }
            for a in 0..3 {
                e.pos[a] += e.motion[a];
            }
            let in_water = self.world.water(e.pos[0].floor() as i32, e.pos[1].floor() as i32, e.pos[2].floor() as i32);
            let (drag, grav) = match e.kind {
                ProjKind::Arrow => (if in_water { 0.6 } else { 0.99 }, 0.05),
                ProjKind::Potion => (if in_water { 0.8 } else { 0.99 }, 0.05),
                _ => (if in_water { 0.8 } else { 0.99 }, 0.03),
            };
            for a in 0..3 {
                e.motion[a] *= drag;
            }
            e.motion[1] -= grav;
            if e.pos[1] < -10.0 || e.ticks_in_air > 200 {
                e.alive = false;
            }
            self.projs[i] = e;
        }
        self.projs.retain(|p| p.alive);
    }

    fn pearl_land(&mut self, at: V3) {
        if self.dead {
            return;
        }
        let mut p = at;
        // Keep the landing out of blocks.
        for _ in 0..4 {
            if !self.world.collides(&Aabb::player(p)) {
                break;
            }
            p[1] += 0.5;
        }
        self.sp.pos = p;
        self.send_down(S2C::Teleport(p));
        if self.cfg.pearl_dmg > 0.0 {
            self.hurt_player(self.cfg.pearl_dmg, Some(p));
        }
    }

    /// `EntityPotion.onImpact` for Instant Health II.
    fn splash(&mut self, at: V3, direct: bool) {
        let bb = self.sp.bb();
        let grown = bb.expand(4.0, 2.0, 4.0);
        if !grown.is_vec_inside(at) && !direct {
            return;
        }
        let c = [self.sp.pos[0], self.sp.pos[1] + EYE_HEIGHT, self.sp.pos[2]];
        let d2 = (c[0] - at[0]).powi(2) + (c[1] - at[1]).powi(2) + (c[2] - at[2]).powi(2);
        if d2 >= 16.0 && !direct {
            return;
        }
        let k = if direct { 1.0 } else { 1.0 - d2.sqrt() / 4.0 };
        let heal = (8.0 * k + 0.5) as i32 as f32;
        let before = self.sp.hp;
        self.sp.hp = (self.sp.hp + heal).min(20.0);
        self.ev.potion_heal += self.sp.hp - before;
    }

    fn tick_mobs(&mut self) {
        let agent = self.sp.pos;
        let n = self.mobs.len();
        for i in 0..n {
            if !self.mobs[i].alive {
                continue;
            }
            let ci = self.mobs[i].chest;
            let c = self.chests[ci].center();
            // The mobs of another player's wave go for that player, unless
            // this one is right there.
            let mp0 = self.mobs[i].l.body.pos;
            let agent_d2 = (agent[0] - mp0[0]).powi(2) + (agent[2] - mp0[2]).powi(2);
            let rival_t = if self.chests[ci].rival_wave { self.rivals.iter().find(|r| r.chest == ci).map(|r| r.pos) } else { None };
            let (target, at_agent) = match rival_t {
                Some(rp) if agent_d2 > 16.0 && !self.mobs[i].mounted => (rp, false),
                _ => (agent, true),
            };
            let tbb = Aabb::player(target);
            let m = &mut self.mobs[i];
            if m.hurt_resistant > 0 {
                m.hurt_resistant -= 1;
            }
            if m.hurt_time > 0 {
                m.hurt_time -= 1;
            }
            if m.attack_cd > 0 {
                m.attack_cd -= 1;
            }
            if m.ranged_cd > 0 {
                m.ranged_cd -= 1;
            }
            if m.mounted {
                m.l.body.set_position([target[0], target[1] + 1.8, target[2]]);
                m.l.body.motion = [0.0; 3];
                if m.attack_cd == 0 {
                    m.attack_cd = m.st.attack_cd;
                    let st = m.st;
                    if self.hurt_player(st.dmg, None) {
                        self.apply_hit_effects(st);
                    }
                }
                continue;
            }
            let p = m.l.body.pos;
            let (dx, dz) = (target[0] - p[0], target[2] - p[2]);
            let dh = (dx * dx + dz * dz).sqrt();
            let want = (dz.atan2(dx) * 180.0 / std::f64::consts::PI) as f32 - 90.0;
            let turn = wrap(want - m.l.yaw).clamp(-30.0, 30.0);
            m.l.yaw = wrap(m.l.yaw + turn);
            let mut forward = m.l.attr.value() as f32;
            let mut strafe = 0.0f32;
            match m.kind {
                MobKind::Horseman => {
                    if dh < 4.0 {
                        forward = -forward;
                    } else if dh < 9.0 {
                        strafe = forward * m.strafe_dir;
                        forward *= 0.2;
                        if self.rng.chance(0.02) {
                            m.strafe_dir = -m.strafe_dir;
                        }
                    }
                }
                MobKind::Golem => {
                    if dh < 6.0 {
                        forward = 0.0;
                    }
                }
                _ => {}
            }
            let in_water = self.world.in_water(&m.l.body.bb);
            let jump = (m.l.body.collided_h && m.l.body.on_ground) || (in_water && self.rng.chance(0.8));
            m.l.living_update(&self.world, strafe / 0.98, forward / 0.98, jump, false);
            m.l.body.fall_distance = 0.0;
            let p = m.l.body.pos;
            let mb = m.l.body.bb;
            // Melee.
            let reach = (m.l.body.hw * 2.0).powi(2) + 0.6;
            let dy = target[1] - p[1];
            let d2 = dx * dx + dz * dz + dy * dy;
            let st = m.st;
            match m.kind {
                MobKind::Zombie => {
                    if d2 <= reach + 1.0 && m.attack_cd == 0 && !self.dead && at_agent {
                        m.attack_cd = st.attack_cd;
                        if self.hurt_player(st.dmg, Some(p)) {
                            let f = (dx * dx + dz * dz).sqrt().max(1e-4);
                            self.sp.motion[0] += dx / f * st.kb_extra * 0.5;
                            self.sp.motion[1] += 0.1 * (st.kb_extra > 0.0) as i32 as f64;
                            self.sp.motion[2] += dz / f * st.kb_extra * 0.5;
                            self.apply_hit_effects(st);
                        }
                    }
                }
                MobKind::Rider => {
                    if mb.expand(0.5, 0.5, 0.5).intersects(&tbb) && !self.dead && at_agent {
                        m.mounted = true;
                        m.attack_cd = st.attack_cd;
                    }
                }
                MobKind::Horseman | MobKind::Golem => {
                    let eye = [p[0], p[1] + mb.max[1] - mb.min[1] - 0.3, p[2]];
                    let teye = [target[0], target[1] + EYE_HEIGHT, target[2]];
                    if m.ranged_cd == 0 && d2 < 15.0 * 15.0 && !self.world.blocked(eye, teye) && !self.dead && at_agent {
                        m.ranged_cd = self.rng.range(st.ranged_cd.0 as f64, st.ranged_cd.1 as f64 + 1.0) as i32;
                        let kind = if m.kind == MobKind::Horseman { ProjKind::Arrow } else { ProjKind::Snowball };
                        let d0 = target[0] - eye[0];
                        let d2_ = target[2] - eye[2];
                        let d1 = target[1] + EYE_HEIGHT - 1.100000023841858 - eye[1];
                        let f4 = (d0 * d0 + d2_ * d2_).sqrt() * 0.2;
                        let inacc = if kind == ProjKind::Arrow { 2.0 } else { 12.0 };
                        let v = heading(&mut self.rng, [d0, d1 + f4, d2_], 1.6, inacc, kind == ProjKind::Arrow);
                        let start = [eye[0] + v[0] / 1.6 * 0.8, eye[1], eye[2] + v[2] / 1.6 * 0.8];
                        self.projs.push(Proj { kind, owner: i as i32, pos: start, motion: v, ticks_in_air: 0, dmg: 2.0, punch: st.proj_punch, crit: false, alive: true });
                    }
                }
            }
            // The leash: a mob too far from its chest is put back on it.
            let m = &mut self.mobs[i];
            if self.t % self.cfg.leash_every == 0 {
                let p = m.l.body.pos;
                let d = ((p[0] - c[0]).powi(2) + (p[2] - c[2]).powi(2)).sqrt();
                if d > self.cfg.leash_r {
                    m.l.body.set_position([c[0], c[1] + 0.5, c[2]]);
                    m.l.body.motion = [0.0; 3];
                    self.ev.leash_tps += 1;
                }
            }
        }
        // A rider whose player wandered off its chest is put back too.
        if self.t % self.cfg.leash_every == 0 {
            for i in 0..n {
                let m = &self.mobs[i];
                if m.alive && m.mounted {
                    let c = self.chests[m.chest].center();
                    let d = ((agent[0] - c[0]).powi(2) + (agent[2] - c[2]).powi(2)).sqrt();
                    if d > self.cfg.leash_r + 1.5 {
                        let m = &mut self.mobs[i];
                        m.mounted = false;
                        m.l.body.set_position([c[0], c[1] + 0.5, c[2]]);
                        m.attack_cd = 20;
                        self.ev.leash_tps += 1;
                    }
                }
            }
        }
        // Mobs push each other apart (`applyEntityCollision`).
        for i in 0..n {
            if !self.mobs[i].alive || self.mobs[i].mounted {
                continue;
            }
            for j in (i + 1)..n {
                if !self.mobs[j].alive || self.mobs[j].mounted {
                    continue;
                }
                if !self.mobs[i].l.body.bb.intersects(&self.mobs[j].l.body.bb) {
                    continue;
                }
                let (pi, pj) = (self.mobs[i].l.body.pos, self.mobs[j].l.body.pos);
                let (mut dx, mut dz) = (pj[0] - pi[0], pj[2] - pi[2]);
                let d = dx.abs().max(dz.abs());
                if d >= 0.01 {
                    let s = d.sqrt();
                    dx /= s;
                    dz /= s;
                    let k = (1.0 / s).min(1.0) * 0.05;
                    self.mobs[i].l.body.motion[0] -= dx * k;
                    self.mobs[i].l.body.motion[2] -= dz * k;
                    self.mobs[j].l.body.motion[0] += dx * k;
                    self.mobs[j].l.body.motion[2] += dz * k;
                }
            }
        }
    }

    fn tick_chests(&mut self) {
        let p = self.sp.pos;
        let mut active = 0;
        for ci in 0..self.chests.len() {
            let c = self.chests[ci].center();
            match self.chests[ci].state {
                ChestState::Cooldown => {
                    if self.t >= self.chests[ci].ready_at {
                        self.chests[ci].state = ChestState::Ready;
                    }
                }
                ChestState::Ready => {
                    let d = dist([p[0], p[1] + 0.9, p[2]], c);
                    let rival = self.rivals.iter().any(|r| dist([r.pos[0], r.pos[1] + 0.9, r.pos[2]], c) <= SPAWN_RADIUS);
                    if (d <= SPAWN_RADIUS && !self.dead) || rival {
                        self.chests[ci].state = ChestState::Active;
                        self.chests[ci].active_since = self.t;
                        self.chests[ci].mobs.clear();
                        self.chests[ci].rival_wave = rival;
                        self.spawn_wave(ci);
                        if !rival {
                            self.ev.waves += 1;
                        }
                    }
                }
                ChestState::Active => {
                    let alive = self.chests[ci].mobs.iter().any(|&m| self.mobs[m].alive);
                    if !alive && self.chests[ci].rival_wave {
                        // The other player takes it at once.
                        self.chests[ci].state = ChestState::Cooldown;
                        self.chests[ci].ready_at = self.t + CLAIM_CD_TICKS;
                    } else if !alive {
                        self.chests[ci].state = ChestState::Claimable;
                    } else if self.t - self.chests[ci].active_since >= WAVE_TICKS {
                        for k in 0..self.chests[ci].mobs.len() {
                            let m = self.chests[ci].mobs[k];
                            self.mobs[m].alive = false;
                            self.mobs[m].mounted = false;
                        }
                        let cd = self.rng.range(self.cfg.reset_cd.0 as f64, self.cfg.reset_cd.1 as f64) as i64;
                        let ch = &mut self.chests[ci];
                        ch.state = ChestState::Cooldown;
                        ch.ready_at = self.t + cd;
                        ch.resets += 1;
                        if !ch.rival_wave {
                            self.ev.resets += 1;
                        }
                    } else {
                        active += 1;
                    }
                }
                ChestState::Claimable => {}
            }
        }
        self.ev.max_simul = self.ev.max_simul.max(active);
        // Drop the dead mobs of chests that are no longer active.
        if self.mobs.len() > 64 && self.mobs.iter().filter(|m| m.alive).count() * 2 < self.mobs.len() {
            let mut remap = vec![usize::MAX; self.mobs.len()];
            let mut kept = Vec::new();
            for (i, m) in self.mobs.drain(..).enumerate() {
                if m.alive {
                    remap[i] = kept.len();
                    kept.push(m);
                }
            }
            self.mobs = kept;
            for ch in &mut self.chests {
                ch.mobs = ch.mobs.iter().filter_map(|&i| if remap[i] != usize::MAX { Some(remap[i]) } else { None }).collect();
            }
            for pr in &mut self.projs {
                if pr.owner >= 0 {
                    let r = remap[pr.owner as usize];
                    if r == usize::MAX {
                        pr.alive = false;
                    } else {
                        pr.owner = r as i32;
                    }
                }
            }
            self.projs.retain(|p| p.alive);
        }
    }

    fn tick_player(&mut self) {
        let t = self.t;
        let sp = &mut self.sp;
        if sp.hurt_resistant > 0 {
            sp.hurt_resistant -= 1;
        }
        // Regeneration II heals one point every 25 ticks.
        if t < sp.regen_until && (sp.regen_until - t) % 25 == 0 && sp.hp > 0.0 {
            sp.hp = (sp.hp + 1.0).min(20.0);
        }
        if t >= sp.abs_until {
            sp.absorption = 0.0;
        }
        if let Some(item) = sp.using {
            sp.use_ticks += 1;
            if item == 2 && sp.use_ticks >= EAT_TICKS {
                sp.using = None;
                if sp.gapples > 0 {
                    sp.gapples -= 1;
                    self.ev.gapples += 1;
                    if sp.hp >= 20.0 && sp.absorption >= 4.0 && t < sp.regen_until {
                        self.ev.gapple_waste += 1;
                    }
                    sp.absorption = 4.0;
                    sp.abs_until = t + 2400;
                    sp.regen_until = t + 100;
                }
            }
        }
        // Residual push on the server's copy of the player.
        let m = &mut sp.motion;
        m[0] *= 0.546;
        m[2] *= 0.546;
        m[1] = (m[1] - 0.08) * 0.98;
        if m[1] < 0.0 {
            m[1] = 0.0;
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let t = self.t;
        let sp = &self.sp;
        Snapshot {
            tick: t,
            mobs: self
                .mobs
                .iter()
                .filter(|m| m.alive)
                .map(|m| MobView {
                    id: m.id,
                    kind: m.kind,
                    pos: m.l.body.pos,
                    motion: m.l.body.motion,
                    hp: m.hp,
                    max_hp: m.st.hp,
                    hurt_time: m.hurt_time,
                    mounted: m.mounted,
                    hw: m.l.body.hw,
                    h: m.l.body.h,
                })
                .collect(),
            projs: self.projs.iter().map(|p| (p.kind, p.owner, p.pos, p.motion)).collect(),
            holo: self.chests.iter().map(|c| (c.state, c.shown_secs(t))).collect(),
            hp: sp.hp,
            absorption: sp.absorption,
            regen_left: (sp.regen_until - t).max(0) as i32,
            slow_left: (sp.slow_until - t).max(0) as i32,
            blind_left: (sp.blind_until - t).max(0) as i32,
            gapples: sp.gapples,
            pearls: sp.pearls,
            potions: sp.potions,
            arrows: sp.arrows,
            pearl_cd_left: (sp.pearl_cd_until - t).max(0) as i32,
            rivals: self.rivals.iter().map(|r| r.pos).collect(),
        }
    }

    /// Put `n` other players at chests of the map.
    pub fn add_rivals(&mut self, n: usize) {
        for _ in 0..n {
            if self.chests.is_empty() {
                return;
            }
            let ci = (self.rng.f64() * self.chests.len() as f64) as usize % self.chests.len();
            let c = self.chests[ci].center();
            let a = self.rng.range(0.0, std::f64::consts::TAU);
            let (x, z) = (c[0] + a.cos() * 2.0, c[2] + a.sin() * 2.0);
            let y = self.world.ground(x.floor() as i32, z.floor() as i32) as f64;
            let stay = self.t + self.rng.range(400.0, 3000.0) as i64;
            self.rivals.push(Rival { pos: [x, y, z], chest: ci, moving: false, stay_until: stay, next_kill: 0 });
        }
    }

    fn tick_rivals(&mut self) {
        let n = self.chests.len();
        for k in 0..self.rivals.len() {
            let mut r = self.rivals[k].clone();
            let c = self.chests[r.chest].center();
            let d = ((c[0] - r.pos[0]).powi(2) + (c[2] - r.pos[2]).powi(2)).sqrt();
            if r.moving {
                if d > 2.5 {
                    let step = 0.28 / d;
                    let x = r.pos[0] + (c[0] - r.pos[0]) * step;
                    let z = r.pos[2] + (c[2] - r.pos[2]) * step;
                    r.pos = [x, self.world.ground(x.floor() as i32, z.floor() as i32) as f64, z];
                } else {
                    r.moving = false;
                    r.stay_until = self.t + self.rng.range(600.0, 3000.0) as i64;
                }
            } else if self.t >= r.stay_until && self.chests[r.chest].state != ChestState::Active {
                r.chest = (self.rng.f64() * n as f64) as usize % n;
                r.moving = true;
            }
            // It kills the mobs of its own wave, one every second or so.
            let ch = &self.chests[r.chest];
            if ch.state == ChestState::Active && ch.rival_wave && d < 10.0 && self.t >= r.next_kill {
                if let Some(&mi) = ch.mobs.iter().find(|&&m| self.mobs[m].alive) {
                    self.mobs[mi].alive = false;
                    self.mobs[mi].mounted = false;
                }
                r.next_kill = self.t + self.rng.range(12.0, 40.0) as i64;
            }
            self.rivals[k] = r;
        }
        // Being near a chest someone else is farming.
        let p = self.sp.pos;
        for ch in &self.chests {
            let c = ch.center();
            let near = self.rivals.iter().any(|r| ((r.pos[0] - c[0]).powi(2) + (r.pos[2] - c[2]).powi(2)).sqrt() <= SPAWN_RADIUS);
            if near && ((p[0] - c[0]).powi(2) + (p[2] - c[2]).powi(2)).sqrt() <= SPAWN_RADIUS {
                self.ev.rival_near += 1;
                break;
            }
        }
    }

    pub fn server_tick(&mut self) {
        while let Some(&(at, p)) = self.up_q.front() {
            if at > self.t {
                break;
            }
            self.up_q.pop_front();
            self.handle(p);
        }
        self.tick_player();
        self.tick_rivals();
        self.tick_mobs();
        self.tick_projectiles();
        self.tick_chests();
        if self.sp.velocity_changed {
            self.sp.velocity_changed = false;
            let m = self.sp.motion;
            // `S12PacketEntityVelocity` carries motion in 1/8000 steps,
            // clamped to 3.9.
            let q = |v: f64| ((v.clamp(-3.9, 3.9) * 8000.0) as i32) as f64 / 8000.0;
            self.send_down(S2C::Velocity([q(m[0]), q(m[1]), q(m[2])]));
        }
        let s = self.snapshot();
        self.snaps.push_back(s);
    }

    /// One tick of the game: the client acts, then the server runs.
    pub fn step(&mut self, inp: &Input) -> Events {
        self.ev = Events::default();
        self.client_tick(inp);
        self.server_tick();
        self.t += 1;
        self.ev
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub forward: f32,
    pub strafe: f32,
    pub jump: bool,
    pub sprint: bool,
    pub dyaw: f32,
    pub dpitch: f32,
    pub attack: bool,
    pub use_: bool,
    /// 255 keeps the slot.
    pub slot: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct PlayerStart {
    pub yaw: f32,
    pub hp: f32,
    pub gapples: u32,
    pub pearls: u32,
    pub potions: u32,
    pub arrows: u32,
}

#[allow(dead_code)]
fn _unused(_: &Rng) {}
