//! The training environment: a new map, chests and server settings every
//! episode; what the agent is told each tick; what it is paid.
use crate::aabb::{Aabb, V3, EYE_HEIGHT};
use crate::game::{ChestState, Cfg, Events, Game, Input, MobKind, PlayerStart, ProjKind, MOB_KINDS, SPAWN_RADIUS, WAVE_TICKS};
use crate::mth::Rng;
use crate::terrain::{generate, TerrainCfg};

/// Scale of the observation's "wave time left": (this - ticks the wave has
/// run) / this, as `Obs.java` (WAVE_TICKS = 200) builds it in the real
/// client, which cannot know the server's timeout. With 15-20 s waves it goes
/// below 0 (to -0.5 .. -1) before the wave resets. The critic's private part
/// has the true time left (`Cfg::wave_ticks`).
pub const WAVE_OBS_TICKS: i64 = WAVE_TICKS;

pub const K_MOBS: usize = 8;
pub const K_CHESTS: usize = 8;
pub const K_PROJ: usize = 4;
pub const GRID: i32 = 4; // the terrain patch is (2*GRID+1)^2 columns
pub const GRID_N: usize = ((2 * GRID + 1) * (2 * GRID + 1)) as usize;

pub const SELF_DIM: usize = 46;
pub const MOB_DIM: usize = 25;
pub const CHEST_DIM: usize = 28;
pub const PROJ_DIM: usize = 9;
pub const OWN_DIM: usize = 8;
pub const TERRAIN_DIM: usize = 2 * GRID_N;
pub const OBS_DIM: usize = SELF_DIM + TERRAIN_DIM + K_MOBS * MOB_DIM + K_CHESTS * CHEST_DIM + K_PROJ * PROJ_DIM + OWN_DIM;
pub const PRIV_DIM: usize = K_CHESTS * 7 + 6;

/// Most the view turns in one tick (degrees), as a player's hand would: the
/// larger bins are cut to this. The mod (Pilot) cuts them the same way.
pub const MAX_DYAW: f32 = 10.0;
pub const MAX_DPITCH: f32 = 7.0;
pub const YAW_BINS: [f32; 15] = [-60.0, -30.0, -15.0, -8.0, -4.0, -2.0, -0.7, 0.0, 0.7, 2.0, 4.0, 8.0, 15.0, 30.0, 60.0];
pub const PITCH_BINS: [f32; 11] = [-30.0, -15.0, -7.0, -3.0, -1.0, 0.0, 1.0, 3.0, 7.0, 15.0, 30.0];
/// forward, strafe, jump, sprint, yaw, pitch, attack, use, slot.
pub const ACT_HEADS: usize = 9;
pub const ACT_DIMS: [i32; ACT_HEADS] = [3, 3, 2, 2, 15, 11, 2, 2, 6];

pub const STAT_NAMES: [&str; 21] = [
    "episodes", "ticks", "claims", "kills", "waves", "resets", "deaths", "dmg_dealt", "dmg_taken", "leash_tps",
    "hits", "clicks", "self_punch", "punch_boost", "gapples", "gapple_waste", "potions", "potion_waste", "pearls", "max_simul",
    "punch_paid",
];
pub const STATS_DIM: usize = STAT_NAMES.len();

/// Every knob of the episodes. Ranges are drawn per episode; a share of
/// the episodes is "extreme" and draws from the hard ends.
#[derive(Clone, Debug)]
pub struct Params {
    pub ep_ticks_min: f64,
    pub ep_ticks_max: f64,
    pub map_size: f64,
    pub relief: f64,
    pub water_prob: f64,
    pub clutter: f64,
    pub gank_spot_prob: f64,
    pub chests_min: f64,
    pub chests_max: f64,
    pub cluster_gap_min: f64,
    pub cluster_gap_max: f64,
    pub ready_share: f64,
    pub init_timer_max_s: f64,
    pub ping_min_ms: f64,
    pub ping_max_ms: f64,
    pub jitter_ms: f64,
    pub speed_min: f64,
    pub speed_max: f64,
    pub cps_min: f64,
    pub cps_max: f64,
    pub sword_dmg_min: f64,
    pub sword_dmg_max: f64,
    pub punch_min: f64,
    pub punch_max: f64,
    pub leash_min: f64,
    pub leash_max: f64,
    pub leash_every_max: f64,
    pub reset_cd_min_s: f64,
    pub reset_cd_max_s: f64,
    pub pearl_cd_max_s: f64,
    pub holo_min: f64,
    pub holo_max: f64,
    pub w_zombie: f64,
    pub w_horseman: f64,
    pub w_golem: f64,
    pub w_rider: f64,
    pub mob_hp_min: f64,
    pub mob_hp_max: f64,
    pub mob_dmg_min: f64,
    pub mob_dmg_max: f64,
    pub mob_kbr_max: f64,
    pub mob_kb_extra_max: f64,
    pub mob_speed_min: f64,
    pub mob_speed_max: f64,
    pub slow_prob: f64,
    pub blind_prob: f64,
    pub proj_dmg_max: f64,
    pub proj_punch_max: f64,
    pub extreme_share: f64,
    pub gapples_min: f64,
    pub gapples_max: f64,
    pub pearls_max: f64,
    pub potions_max: f64,
    pub start_hurt: f64,
    // Pay.
    pub w_claim: f64,
    pub w_kill: f64,
    pub w_wave: f64,
    pub w_reset: f64,
    pub w_dmg: f64,
    pub w_hurt: f64,
    pub w_death: f64,
    pub w_tp: f64,
    pub w_waste: f64,
    pub w_shape: f64,
    /// Pay of a Punch shot at oneself. With `punch_mode` 1 it is paid only when,
    /// within `punch_window` ticks of the hit, the player has come at least
    /// `punch_gain` blocks closer to the chest that mattered at the hit than
    /// sprinting would have brought it, to a distance from that chest it had
    /// not reached before (so going away and boosting back pays nothing), at
    /// most once every `punch_every_s` seconds and not with a pearl thrown in
    /// between. With `punch_mode` 0, run 3e's rule: paid for any self-hit with
    /// no wave running close by and the nearest chest to work on past 12 blocks.
    pub w_punch: f64,
    pub punch_mode: f64,
    pub punch_gain: f64,
    pub punch_window: f64,
    pub punch_every_s: f64,
    pub gamma: f64,
    pub arrow_self_dmg: f64,
    pub rival_prob: f64,
    pub rivals_max: f64,
    pub w_rival: f64,
    /// Cost of turning the view, per tick: w_turn * ((|dyaw| + |dpitch|) / 15)^2, so that
    /// the agent aims like a player and not like a turret.
    pub w_turn: f64,
    /// Cost of turning back the other way right after a turn (jitter).
    pub w_flip: f64,
    /// Cost per tick of being more than 8 blocks from a chest whose wave runs
    /// (leaving it makes the wave reset).
    pub w_away: f64,
    /// Cost per tick of standing about: less than 1.5 blocks moved in the last 2
    /// seconds and no blow landed in them.
    pub w_idle: f64,
    /// Cost of spinning: the signed turn averaged over about a second, past 3
    /// degrees a tick, so that the view does not go round and round.
    pub w_spin: f64,
    /// 1: hits land as the SilentAim aimbot of the real client lands them (the
    /// nearest own mob within reach and in sight, at the click rate, with the
    /// sword in hand, wherever the view points); the attack head then only
    /// claims a chest under the crosshair. 0: the agent aims and clicks itself.
    pub silent_aim: f64,
    /// Seconds a wave has before its chest resets, drawn per episode.
    pub wave_s_min: f64,
    pub wave_s_max: f64,
    /// Pay per point of damage to the mobs of another player's wave (w_dmg is
    /// for the player's own mobs only).
    pub w_dmg_rival: f64,
    /// A golden apple eaten with this much absorption still left counts as a
    /// waste (as well as one eaten at full health with all of it and regeneration on).
    pub gapple_waste_abs: f64,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            ep_ticks_min: 2400.0,
            ep_ticks_max: 6000.0,
            map_size: 96.0,
            relief: 14.0,
            water_prob: 0.5,
            clutter: 6.0,
            gank_spot_prob: 0.5,
            chests_min: 3.0,
            chests_max: 10.0,
            cluster_gap_min: 4.0,
            cluster_gap_max: 22.0,
            ready_share: 0.3,
            init_timer_max_s: 90.0,
            ping_min_ms: 150.0,
            ping_max_ms: 250.0,
            jitter_ms: 20.0,
            speed_min: 1.0,
            speed_max: 4.0,
            cps_min: 6.0,
            cps_max: 9.0,
            sword_dmg_min: 7.0,
            sword_dmg_max: 13.25,
            punch_min: 1.0,
            punch_max: 2.0,
            leash_min: 4.5,
            leash_max: 7.0,
            leash_every_max: 20.0,
            reset_cd_min_s: 5.0,
            reset_cd_max_s: 60.0,
            pearl_cd_max_s: 15.0,
            holo_min: 24.0,
            holo_max: 96.0,
            w_zombie: 1.0,
            w_horseman: 0.6,
            w_golem: 0.6,
            w_rider: 0.4,
            mob_hp_min: 20.0,
            mob_hp_max: 80.0,
            mob_dmg_min: 0.5,
            mob_dmg_max: 3.0,
            mob_kbr_max: 0.6,
            mob_kb_extra_max: 0.8,
            mob_speed_min: 0.2,
            mob_speed_max: 0.35,
            slow_prob: 0.15,
            blind_prob: 0.1,
            proj_dmg_max: 2.5,
            proj_punch_max: 1.0,
            extreme_share: 0.25,
            gapples_min: 8.0,
            gapples_max: 64.0,
            pearls_max: 16.0,
            potions_max: 8.0,
            start_hurt: 0.2,
            w_claim: 10.0,
            w_kill: 1.0,
            w_wave: 1.0,
            w_reset: 6.0,
            w_dmg: 0.02,
            w_hurt: 0.06,
            w_death: 15.0,
            w_tp: 0.2,
            w_waste: 0.5,
            w_shape: 0.08,
            // Run 3e paid 1.5 for any self-hit (punch_mode 0) and it was farmed;
            // now only for measured travel. A perfect shot gains 2-5 blocks over
            // sprinting in the 30 ticks after the hit (Punch I-II, Speed I-IV), so
            // the bar is 3 blocks, not 8.
            w_punch: 1.0,
            punch_mode: 1.0,
            punch_gain: 3.0,
            punch_window: 30.0,
            punch_every_s: 5.0,
            gamma: 0.998,
            arrow_self_dmg: 0.35,
            rival_prob: 0.5,
            rivals_max: 3.0,
            w_rival: 0.01,
            w_turn: 0.004,
            w_flip: 0.025,
            w_away: 0.01,
            w_idle: 0.03,
            w_spin: 0.08,
            silent_aim: 1.0,
            wave_s_min: 15.0,
            wave_s_max: 20.0,
            w_dmg_rival: 0.0,
            gapple_waste_abs: 2.0,
        }
    }
}

macro_rules! params_table {
    ($($name:ident),* $(,)?) => {
        pub const PARAM_NAMES: &[&str] = &[$(stringify!($name)),*];
        pub const PARAM_CNAMES: &[&str] = &[$(concat!(stringify!($name), "\0")),*];
        impl Params {
            pub fn set(&mut self, id: i32, v: f64) -> bool {
                let mut i = 0;
                $(
                    if id == i { self.$name = v; return true; }
                    i += 1;
                )*
                let _ = i;
                false
            }
        }
    };
}

params_table!(
    ep_ticks_min, ep_ticks_max, map_size, relief, water_prob, clutter, gank_spot_prob, chests_min, chests_max,
    cluster_gap_min, cluster_gap_max, ready_share, init_timer_max_s, ping_min_ms, ping_max_ms, jitter_ms,
    speed_min, speed_max, cps_min, cps_max, sword_dmg_min, sword_dmg_max, punch_min, punch_max, leash_min,
    leash_max, leash_every_max, reset_cd_min_s, reset_cd_max_s, pearl_cd_max_s, holo_min, holo_max, w_zombie,
    w_horseman, w_golem, w_rider, mob_hp_min, mob_hp_max, mob_dmg_min, mob_dmg_max, mob_kbr_max,
    mob_kb_extra_max, mob_speed_min, mob_speed_max, slow_prob, blind_prob, proj_dmg_max, proj_punch_max,
    extreme_share, gapples_min, gapples_max, pearls_max, potions_max, start_hurt, w_claim, w_kill, w_wave,
    w_reset, w_dmg, w_hurt, w_death, w_tp, w_waste, w_shape, w_punch, gamma, arrow_self_dmg, rival_prob, rivals_max, w_rival, w_turn, w_flip, w_away, w_idle, w_spin,
    silent_aim, wave_s_min, wave_s_max, w_dmg_rival, gapple_waste_abs, punch_mode, punch_gain, punch_window,
    punch_every_s,
);

/// What the client remembers of a chest it has seen: the hologram's last
/// state and number, and when.
#[derive(Clone, Copy, Debug, Default)]
pub struct Known {
    pub seen: bool,
    pub tick: i64,
    pub state: u8,
    pub secs: i32,
    pub active_seen_at: i64,
}

pub struct Env {
    pub p: Params,
    pub rng: Rng,
    pub g: Game,
    pub known: Vec<Known>,
    pub ticks: i64,
    pub ep_ticks: i64,
    pub ping_ms: f64,
    pub holo_range: f64,
    pub extreme: bool,
    pub last_hp: f32,
    pub since_hurt: i32,
    pub phi: f64,
    pub stats: [f64; STATS_DIM],
    /// When the teacher last finished a golden apple.
    pub teacher_ate: i64,
    /// Seed of the next episode's level, if the trainer picked one.
    pub next_seed: Option<u64>,
    last_dyaw: f32,
    spin: f32,
    idle_from: [f64; 3],
    idle_tick: u64,
    last_hit: u64,
    punch: PunchWatch,
}

/// What the Punch pay of `punch_mode` 1 keeps track of.
#[derive(Clone, Debug)]
struct PunchWatch {
    /// Per chest: the closest the player has come to it since its state (as
    /// the client knows it) last changed, and that state.
    best: Vec<f64>,
    state: Vec<u8>,
    /// The self-hit being watched: its tick, its chest, the distance then and
    /// the closest the player had come to that chest before it.
    at: i64,
    chest: usize,
    d0: f64,
    best0: f64,
    paid_at: i64,
}

impl Default for PunchWatch {
    fn default() -> Self {
        PunchWatch { best: Vec::new(), state: Vec::new(), at: -1, chest: 0, d0: 0.0, best0: 0.0, paid_at: -1_000_000 }
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Ticks of a one-way delay of `ms`, with the fraction drawn.
fn ticks_of(rng: &mut Rng, ms: f64) -> i64 {
    let t = ms / 50.0;
    let base = t.floor();
    (base + if rng.f64() < t - base { 1.0 } else { 0.0 }) as i64
}

pub fn new_game(p: &Params, rng: &mut Rng) -> (Game, f64, f64, bool, i64) {
    let extreme = rng.chance(p.extreme_share);
    // Hard ends of the ranges in extreme episodes.
    let hard = |lo: f64, hi: f64, rng: &mut Rng| if extreme { lerp(lo, hi, rng.range(0.6, 1.0)) } else { rng.range(lo, hi) };
    let tc = TerrainCfg {
        size: p.map_size as i32,
        height: 48,
        base: 6,
        relief: if extreme { p.relief * 1.5 } else { p.relief },
        water_prob: p.water_prob,
        clutter: if extreme { p.clutter * 1.5 } else { p.clutter },
        gank_spot_prob: p.gank_spot_prob,
        chests_min: p.chests_min as usize,
        chests_max: if extreme { (p.chests_max * 1.3) as usize } else { p.chests_max as usize },
        cluster_gap_min: p.cluster_gap_min,
        cluster_gap_max: p.cluster_gap_max,
        cluster_max: 4,
    };
    let map = generate(&tc, rng);
    let timers: Vec<i64> = map
        .chests
        .iter()
        .map(|_| if rng.chance(p.ready_share) { 0 } else { (rng.range(0.0, p.init_timer_max_s) * 20.0) as i64 })
        .collect();
    let ping = if extreme { rng.range(p.ping_max_ms, p.ping_max_ms + 60.0) } else { rng.range(p.ping_min_ms, p.ping_max_ms) };
    let up = ticks_of(rng, ping / 2.0);
    let down = ticks_of(rng, ping / 2.0);
    let speed = if extreme { 4 } else { rng.range(p.speed_min, p.speed_max + 1.0).floor().min(p.speed_max) as i8 };
    let w = [p.w_zombie, p.w_horseman, p.w_golem, p.w_rider];
    let mut kind_weights = [0.0; MOB_KINDS];
    for k in 0..MOB_KINDS {
        kind_weights[k] = w[k] * if extreme && k > 0 { 1.8 } else { 1.0 };
    }
    let hp_hi = hard(p.mob_hp_min, p.mob_hp_max, rng);
    let cfg = Cfg {
        up,
        down,
        jitter: { let j = rng.range(0.0, p.jitter_ms); ticks_of(rng, j) },
        speed_amp: speed,
        cps: rng.range(p.cps_min, p.cps_max),
        sword_dmg: rng.range(p.sword_dmg_min, p.sword_dmg_max) as f32,
        punch: rng.range(p.punch_min, p.punch_max + 1.0).floor().min(p.punch_max),
        arrow_self_dmg_mult: p.arrow_self_dmg as f32,
        leash_r: rng.range(p.leash_min, p.leash_max),
        leash_every: rng.range(1.0, p.leash_every_max + 1.0) as i64,
        reset_cd: ((p.reset_cd_min_s * 20.0) as i64, (p.reset_cd_max_s * 20.0) as i64),
        pearl_cd: (rng.range(0.0, p.pearl_cd_max_s) * 20.0) as i64,
        pearl_dmg: 0.0,
        holo_range: rng.range(p.holo_min, p.holo_max),
        kind_weights,
        mob_hp: (p.mob_hp_min as f32, hp_hi as f32),
        mob_dmg: (p.mob_dmg_min as f32, hard(p.mob_dmg_min, p.mob_dmg_max, rng) as f32),
        mob_kb_resist: (0.0, hard(0.0, p.mob_kbr_max, rng)),
        mob_kb_extra: (0.0, hard(0.0, p.mob_kb_extra_max, rng)),
        mob_speed: (p.mob_speed_min, hard(p.mob_speed_min, p.mob_speed_max, rng)),
        mob_attack_cd: if extreme { (8, 14) } else { (10, 20) },
        ranged_cd: if extreme { (15, 30) } else { (20, 60) },
        proj_dmg: (0.5, hard(0.5, p.proj_dmg_max, rng) as f32),
        proj_punch: (0.0, hard(0.0, p.proj_punch_max, rng)),
        slow_prob: if extreme { (p.slow_prob * 2.5).min(1.0) } else { p.slow_prob },
        blind_prob: if extreme { (p.blind_prob * 2.5).min(1.0) } else { p.blind_prob },
        effect_ticks: (40, 120),
        // Set below, after every other draw, so that the rest of an episode is
        // drawn as before.
        wave_ticks: WAVE_TICKS,
        silent_aim: p.silent_aim >= 0.5,
        gapple_waste_abs: p.gapple_waste_abs as f32,
    };
    let holo = cfg.holo_range;
    let start = PlayerStart {
        yaw: rng.range(-180.0, 180.0) as f32,
        hp: if rng.chance(p.start_hurt) { rng.range(6.0, 20.0) as f32 } else { 20.0 },
        gapples: rng.range(p.gapples_min, p.gapples_max) as u32,
        pearls: rng.range(0.0, p.pearls_max + 1.0) as u32,
        potions: rng.range(0.0, p.potions_max + 1.0) as u32,
        arrows: 64,
    };
    let seed = rng.next_u64();
    let ep = rng.range(p.ep_ticks_min, p.ep_ticks_max) as i64;
    let mut cfg = cfg;
    let wave_s = if p.wave_s_max > p.wave_s_min { rng.range(p.wave_s_min, p.wave_s_max) } else { p.wave_s_min };
    cfg.wave_ticks = ((wave_s * 20.0).round() as i64).max(1);
    let mut g = Game::new(map.world, &map.chests, map.spawn, cfg, Rng::new(seed), &timers, start);
    if rng.chance(p.rival_prob) {
        let k = 1 + (rng.f64() * p.rivals_max) as usize;
        g.add_rivals(k.min(p.rivals_max.max(1.0) as usize));
    }
    (g, ping, holo, extreme, ep)
}

/// Turn a point into the player's frame: right, up, forward.
#[inline]
fn local(yaw: f32, d: V3) -> V3 {
    let y = yaw as f64 * std::f64::consts::PI / 180.0;
    let (s, c) = (y.sin(), y.cos());
    // Forward is (-sin, cos) in the game's x/z.
    let fwd = -d[0] * s + d[2] * c;
    let right = -d[0] * c - d[2] * s;
    [right, d[1], fwd]
}

fn angle_err(yaw: f32, pitch: f32, eye: V3, at: V3) -> (f64, f64) {
    let (dx, dy, dz) = (at[0] - eye[0], at[1] - eye[1], at[2] - eye[2]);
    let want_yaw = (dz.atan2(dx) * 180.0 / std::f64::consts::PI) - 90.0;
    let mut e = (want_yaw - yaw as f64) % 360.0;
    if e > 180.0 {
        e -= 360.0;
    }
    if e < -180.0 {
        e += 360.0;
    }
    let h = (dx * dx + dz * dz).sqrt();
    let want_pitch = -(dy.atan2(h) * 180.0 / std::f64::consts::PI);
    (e, want_pitch - pitch as f64)
}

impl Env {
    pub fn new(p: Params, seed: u64) -> Env {
        let mut rng = Rng::new(seed);
        let (g, ping, holo, extreme, ep) = new_game(&p, &mut rng);
        let n = g.chests.len();
        let mut e = Env { p, rng, g, known: vec![Known::default(); n], ticks: 0, ep_ticks: ep, ping_ms: ping, holo_range: holo, extreme, last_hp: 20.0, since_hurt: 100, phi: 0.0, stats: [0.0; STATS_DIM], teacher_ate: -1000, next_seed: None, last_dyaw: 0.0, spin: 0.0, idle_from: [0.0; 3], idle_tick: 0, last_hit: 0, punch: PunchWatch::default() };
        e.last_hp = e.g.sp.hp;
        e.update_known();
        e.phi = e.potential();
        e
    }

    pub fn reset(&mut self) {
        if let Some(seed) = self.next_seed.take() {
            self.rng = Rng::new(seed);
        }
        let (g, ping, holo, extreme, ep) = new_game(&self.p, &mut self.rng);
        self.known = vec![Known::default(); g.chests.len()];
        self.g = g;
        self.ticks = 0;
        self.ep_ticks = ep;
        self.ping_ms = ping;
        self.holo_range = holo;
        self.extreme = extreme;
        self.last_hp = self.g.sp.hp;
        self.last_dyaw = 0.0;
        self.spin = 0.0;
        self.idle_from = self.g.cp.l.body.pos;
        self.idle_tick = 0;
        self.last_hit = 0;
        self.punch = PunchWatch::default();
        self.since_hurt = 100;
        self.teacher_ate = -1000;
        self.update_known();
        self.phi = self.potential();
    }

    /// The client's memory of the holograms it can see now.
    fn update_known(&mut self) {
        let pos = self.g.cp.l.body.pos;
        let now = self.g.view.tick;
        for (ci, c) in self.g.chests.iter().enumerate() {
            let cc = c.center();
            let d = ((cc[0] - pos[0]).powi(2) + (cc[1] - pos[1]).powi(2) + (cc[2] - pos[2]).powi(2)).sqrt();
            if d > self.holo_range {
                continue;
            }
            let Some(&(st, secs)) = self.g.view.holo.get(ci) else { continue };
            let k = &mut self.known[ci];
            let st = st as u8;
            if st == ChestState::Active as u8 && (!k.seen || k.state != st) {
                k.active_seen_at = now;
            }
            k.seen = true;
            k.tick = now;
            k.state = st;
            k.secs = secs;
        }
    }

    /// Seconds to the next wave as the client works it out from memory.
    fn est_secs(&self, ci: usize) -> f64 {
        let k = &self.known[ci];
        if !k.seen {
            return 999.0;
        }
        match k.state {
            0 => (k.secs as f64 - (self.g.view.tick - k.tick) as f64 / 20.0).max(0.0),
            _ => 0.0,
        }
    }

    /// Which chest matters most now, by what the client knows: one to
    /// claim, then a wave running out of time, then one ready, then the one
    /// that will be ready soonest.
    fn focus(&self) -> Option<usize> {
        let pos = self.g.cp.l.body.pos;
        let mut best: Option<(f64, usize)> = None;
        for ci in 0..self.g.chests.len() {
            let k = &self.known[ci];
            if !k.seen {
                continue;
            }
            let c = self.g.chests[ci].center();
            let d = ((c[0] - pos[0]).powi(2) + (c[2] - pos[2]).powi(2)).sqrt();
            let score = match k.state {
                3 => 0.0 + d * 0.01,
                2 => 100.0 + (self.g.view.tick - k.active_seen_at) as f64 * -0.01,
                1 => 200.0 + d * 0.1,
                _ => 300.0 + self.est_secs(ci) + d * 0.05,
            };
            if best.map_or(true, |b| score < b.0) {
                best = Some((score, ci));
            }
        }
        best.map(|b| b.1)
    }

    /// Horizontal distance from the player to chest `ci`.
    fn chest_dist(&self, ci: usize) -> f64 {
        let c = self.g.chests[ci].center();
        let p = self.g.cp.l.body.pos;
        ((c[0] - p[0]).powi(2) + (c[2] - p[2]).powi(2)).sqrt()
    }

    /// The Punch pay of `punch_mode` 1 this tick: 1 when a watched self-hit
    /// has now carried the player `punch_gain` blocks closer to its chest than
    /// sprinting would have, and closer than the player had come before.
    fn punch_pay(&mut self, ev: &Events) -> u32 {
        let t = self.ticks;
        let n = self.g.chests.len();
        let dist: Vec<f64> = (0..n).map(|ci| self.chest_dist(ci)).collect();
        let focus = self.focus();
        // Ground speed of a sprint (with jumps or not) at this episode's Speed
        // level, measured on flat ground: 0.34 blocks a tick with Speed I, 0.56 with Speed V
        // (Cfg::speed_amp is the amplifier: 0 is Speed I, -1 none).
        let walk = 0.28 + 0.056 * (self.g.cfg.speed_amp.max(-1) + 1) as f64;
        let (gain, window, every) = (self.p.punch_gain, self.p.punch_window, self.p.punch_every_s * 20.0);
        let w = &mut self.punch;
        if w.best.len() != n {
            w.best = vec![f64::INFINITY; n];
            w.state = vec![255; n];
        }
        let mut paid = 0;
        if ev.pearls > 0 {
            w.at = -1;
        }
        if w.at >= 0 {
            let d = dist[w.chest];
            let dt = (t - w.at) as f64;
            if dt > window {
                w.at = -1;
            } else if (w.d0 - d) - walk * dt >= gain && d <= w.best0 - gain && (t - w.paid_at) as f64 >= every {
                paid = 1;
                w.paid_at = t;
                w.at = -1;
            }
        }
        if ev.self_punch > 0 && w.at < 0 && ev.pearls == 0 {
            if let Some(ci) = focus {
                w.at = t;
                w.chest = ci;
                w.d0 = dist[ci];
                w.best0 = w.best[ci].min(dist[ci]);
            }
        }
        for ci in 0..n {
            let st = if self.known[ci].seen { self.known[ci].state } else { 254 };
            if st != w.state[ci] {
                w.state[ci] = st;
                w.best[ci] = dist[ci];
            } else {
                w.best[ci] = w.best[ci].min(dist[ci]);
            }
        }
        paid
    }

    fn potential(&self) -> f64 {
        match self.focus() {
            Some(ci) => {
                let c = self.g.chests[ci].center();
                let p = self.g.cp.l.body.pos;
                let d = ((c[0] - p[0]).powi(2) + (c[2] - p[2]).powi(2)).sqrt();
                // Close enough to the chest is as good as on it.
                -self.p.w_shape * (d - 3.0).max(0.0)
            }
            None => 0.0,
        }
    }

    /// Chests in the order the agent is told of them: nearest first.
    fn chest_order(&self) -> Vec<usize> {
        let pos = self.g.cp.l.body.pos;
        let mut v: Vec<(f64, usize)> = (0..self.g.chests.len())
            .map(|ci| {
                let c = self.g.chests[ci].center();
                (((c[0] - pos[0]).powi(2) + (c[2] - pos[2]).powi(2)).sqrt(), ci)
            })
            .collect();
        v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        v.into_iter().take(K_CHESTS).map(|x| x.1).collect()
    }

    pub fn write_obs(&self, o: &mut [f32], pv: &mut [f32]) {
        let g = &self.g;
        let l = &g.cp.l;
        let v = &g.view;
        let pos = l.body.pos;
        let eye = [pos[0], pos[1] + EYE_HEIGHT, pos[2]];
        let yaw = l.yaw;
        let mut ww = W { o, w: 0 };
        // Self.
        ww.put(v.hp as f64 / 20.0);
        ww.put(v.absorption as f64 / 4.0);
        ww.put((v.regen_left as f64 / 100.0).min(1.0));
        ww.put((v.slow_left as f64 / 100.0).min(1.5));
        ww.put((v.blind_left as f64 / 100.0).min(1.5));
        ww.put((v.gapples as f64 / 32.0).min(2.0));
        ww.put((v.pearls as f64 / 16.0).min(1.0));
        ww.put((v.potions as f64 / 8.0).min(1.0));
        ww.put((v.arrows as f64 / 64.0).min(1.0));
        ww.put((v.pearl_cd_left as f64 / 300.0).min(1.0));
        let mv = local(yaw, l.body.motion);
        ww.put(mv[0] * 3.0);
        ww.put(mv[1] * 3.0);
        ww.put(mv[2] * 3.0);
        ww.put(l.body.on_ground as i32 as f64);
        ww.put(g.world.in_water(&l.body.bb) as i32 as f64);
        ww.put(l.body.collided_h as i32 as f64);
        ww.put(l.pitch as f64 / 90.0);
        ww.put(l.sprinting as i32 as f64);
        for s in 0..5u8 {
            ww.put((g.cp.slot == s) as i32 as f64);
        }
        ww.put(g.cp.using as i32 as f64);
        ww.put((g.cp.use_ticks as f64 / 32.0).min(1.0));
        let f = g.cp.use_ticks as f64 / 20.0;
        ww.put(if g.cp.slot == 1 && g.cp.using { ((f * f + 2.0 * f) / 3.0).min(1.0) } else { 0.0 });
        ww.put(g.cp.tokens);
        ww.put(g.cfg.speed_amp as f64 / 4.0);
        ww.put(self.ping_ms / 250.0);
        ww.put(g.cfg.cps / 9.0);
        ww.put((self.since_hurt as f64 / 20.0).min(2.0));
        ww.put(v.mobs.iter().filter(|m| m.mounted).count() as f64);
        ww.put(0.0);
        ww.put(g.cfg.punch / 2.0);
        ww.put(g.cfg.leash_r / 6.0);
        ww.put(self.extreme as i32 as f64);
        let rd = v.rivals.iter().map(|r| ((r[0] - pos[0]).powi(2) + (r[2] - pos[2]).powi(2)).sqrt()).fold(64.0, f64::min);
        ww.put(rd / 64.0);
        // Eight level rays at knee height: how far to the first wall.
        for r in 0..8 {
            let a = yaw as f64 + r as f64 * 45.0;
            let ar = a * std::f64::consts::PI / 180.0;
            let dir = [-ar.sin(), 0.0, ar.cos()];
            let from = [pos[0], pos[1] + 0.6, pos[2]];
            let to = [from[0] + dir[0] * 8.0, from[1], from[2] + dir[2] * 8.0];
            let d = match g.world.ray_trace_blocks(from, to, false) {
                crate::world::RayHit::Block(h, _) => ((h[0] - from[0]).powi(2) + (h[2] - from[2]).powi(2)).sqrt(),
                _ => 8.0,
            };
            ww.put(d / 8.0);
        }
        ww.put(1.0);
        debug_assert_eq!(ww.w, SELF_DIM);

        // Terrain around the feet, turned with the view.
        let fy = pos[1].floor() as i32;
        let yr = yaw as f64 * std::f64::consts::PI / 180.0;
        let (s, c) = (yr.sin(), yr.cos());
        for i in -GRID..=GRID {
            for j in -GRID..=GRID {
                // i: right, j: forward.
                let (rx, fz) = (i as f64, j as f64);
                let x = pos[0] + (-rx * c - fz * s);
                let z = pos[2] + (-rx * s + fz * c);
                let (bx, bz) = (x.floor() as i32, z.floor() as i32);
                let mut h = -5;
                for y in (fy - 5..=fy + 4).rev() {
                    if g.world.solid(bx, y, bz) {
                        h = y + 1 - fy;
                        break;
                    }
                }
                ww.put(h as f64 / 5.0);
                ww.put(g.world.water(bx, fy.max(fy + h - 1), bz) as i32 as f64 + g.world.water(bx, fy, bz) as i32 as f64 * 0.5);
            }
        }

        // Mobs, nearest first.
        let (pm, pc) = g.pick();
        let mut mobs: Vec<(f64, usize)> = v.mobs.iter().enumerate().map(|(i, m)| (((m.pos[0] - pos[0]).powi(2) + (m.pos[1] - pos[1]).powi(2) + (m.pos[2] - pos[2]).powi(2)).sqrt(), i)).collect();
        mobs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        for k in 0..K_MOBS {
            if let Some(&(d, i)) = mobs.get(k) {
                let m = &v.mobs[i];
                let rel = local(yaw, [m.pos[0] - pos[0], m.pos[1] - pos[1], m.pos[2] - pos[2]]);
                ww.put(1.0);
                ww.put(rel[0] / 16.0);
                ww.put(rel[1] / 16.0);
                ww.put(rel[2] / 16.0);
                ww.put(d / 16.0);
                let mv = local(yaw, m.motion);
                ww.put(mv[0] * 4.0);
                ww.put(mv[1] * 4.0);
                ww.put(mv[2] * 4.0);
                ww.put((m.hp / m.max_hp.max(1.0)) as f64);
                ww.put(m.hp as f64 / 40.0);
                for kk in 0..MOB_KINDS {
                    ww.put((m.kind as usize == kk) as i32 as f64);
                }
                ww.put(m.hurt_time as f64 / 10.0);
                ww.put(m.mounted as i32 as f64);
                let center = [m.pos[0], m.pos[1] + m.h * 0.5, m.pos[2]];
                let bb = Aabb::new([m.pos[0] - m.hw, m.pos[1], m.pos[2] - m.hw], [m.pos[0] + m.hw, m.pos[1] + m.h, m.pos[2] + m.hw]);
                let cp = bb.closest_point(eye);
                let reach = ((cp[0] - eye[0]).powi(2) + (cp[1] - eye[1]).powi(2) + (cp[2] - eye[2]).powi(2)).sqrt();
                ww.put((reach <= 3.0) as i32 as f64);
                let (ey, ep) = angle_err(yaw, l.pitch, eye, center);
                ww.put(ey / 180.0);
                ww.put(ep / 90.0);
                ww.put((pm == Some(m.id)) as i32 as f64);
                // Its chest is the one it is nearest to; the leash pulls it back
                // past `leash_r`.
                let mut cd = 99.0;
                let mut cpos = m.pos;
                let mut cidx = 0usize;
                for (ci, ch) in g.chests.iter().enumerate() {
                    let cc = ch.center();
                    let dd = ((cc[0] - m.pos[0]).powi(2) + (cc[2] - m.pos[2]).powi(2)).sqrt();
                    if dd < cd {
                        cd = dd;
                        cpos = cc;
                        cidx = ci;
                    }
                }
                ww.put(cd / g.cfg.leash_r);
                ww.put(((g.cfg.leash_r - cd) / 3.0).clamp(-1.0, 2.0));
                // A wall behind the mob, as seen from the player: a place to pin it.
                let (bx, bz) = (m.pos[0] - pos[0], m.pos[2] - pos[2]);
                let bl = (bx * bx + bz * bz).sqrt().max(1e-3);
                let back = [m.pos[0] + bx / bl * (m.hw + 0.6), m.pos[1] + 0.5, m.pos[2] + bz / bl * (m.hw + 0.6)];
                let _ = cpos;
                ww.put(g.world.solid(back[0].floor() as i32, back[1].floor() as i32, back[2].floor() as i32) as i32 as f64);
                // Time its wave has left, and whether that wave is the one
                // closest to resetting.
                let kn = &self.known[cidx];
                let left = |k: &Known| if k.seen && k.state == 2 { (WAVE_OBS_TICKS - (v.tick - k.active_seen_at)) as f64 / WAVE_OBS_TICKS as f64 } else { 1.0 };
                let mine = left(kn);
                ww.put(mine);
                let oldest = self.known.iter().map(|k| left(k)).fold(1.0, f64::min);
                ww.put((kn.state == 2 && mine <= oldest + 1e-9) as i32 as f64);
            } else {
                for _ in 0..MOB_DIM {
                    ww.put(0.0);
                }
            }
        }

        // Chests, nearest first, from the client's memory.
        let order = self.chest_order();
        for k in 0..K_CHESTS {
            if let Some(&ci) = order.get(k) {
                let ch = &g.chests[ci];
                let cc = ch.center();
                let kn = &self.known[ci];
                let rel = local(yaw, [cc[0] - pos[0], cc[1] - pos[1], cc[2] - pos[2]]);
                let d = ((cc[0] - pos[0]).powi(2) + (cc[1] - pos[1] - 0.9).powi(2) + (cc[2] - pos[2]).powi(2)).sqrt();
                ww.put(1.0);
                ww.put(rel[0] / 32.0);
                ww.put(rel[1] / 32.0);
                ww.put(rel[2] / 32.0);
                ww.put(d / 32.0);
                for s in 0..4u8 {
                    ww.put((kn.seen && kn.state == s) as i32 as f64);
                }
                ww.put((!kn.seen) as i32 as f64);
                ww.put(if kn.seen { kn.secs as f64 / 180.0 } else { 0.0 });
                let est = self.est_secs(ci);
                ww.put(if kn.seen { est / 180.0 } else { 1.0 });
                ww.put(if kn.seen { (1.0 + est).ln() / 5.2 } else { 1.0 });
                let window = if kn.seen && kn.state == 2 { (WAVE_OBS_TICKS - (v.tick - kn.active_seen_at)) as f64 / WAVE_OBS_TICKS as f64 } else { 0.0 };
                ww.put(window);
                let near = v.mobs.iter().filter(|m| ((m.pos[0] - cc[0]).powi(2) + (m.pos[2] - cc[2]).powi(2)).sqrt() < g.cfg.leash_r + 2.0).count();
                ww.put(near as f64 / 5.0);
                ww.put(if kn.seen { ((v.tick - kn.tick) as f64 / 100.0).min(3.0) } else { 3.0 });
                ww.put((d <= SPAWN_RADIUS) as i32 as f64);
                ww.put(((SPAWN_RADIUS - d) / SPAWN_RADIUS).clamp(-2.0, 1.0));
                // The nearest other chest, and the point between the two.
                let mut od = 999.0;
                let mut oi = ci;
                for (cj, c2) in g.chests.iter().enumerate() {
                    if cj == ci {
                        continue;
                    }
                    let c2 = c2.center();
                    let dd = ((c2[0] - cc[0]).powi(2) + (c2[1] - cc[1]).powi(2) + (c2[2] - cc[2]).powi(2)).sqrt();
                    if dd < od {
                        od = dd;
                        oi = cj;
                    }
                }
                ww.put((od / 24.0).min(2.0));
                if oi != ci {
                    let c2 = g.chests[oi].center();
                    let mid = [(cc[0] + c2[0]) / 2.0, (cc[1] + c2[1]) / 2.0, (cc[2] + c2[2]) / 2.0];
                    let rm = local(yaw, [mid[0] - pos[0], mid[1] - pos[1], mid[2] - pos[2]]);
                    ww.put(rm[0] / 32.0);
                    ww.put(rm[1] / 32.0);
                    ww.put(rm[2] / 32.0);
                    ww.put((od <= 2.0 * SPAWN_RADIUS - 1.0) as i32 as f64);
                    ww.put(if self.known[oi].seen { self.est_secs(oi) / 180.0 } else { 1.0 });
                } else {
                    for _ in 0..5 {
                        ww.put(0.0);
                    }
                }
                ww.put((pc == Some(ci)) as i32 as f64);
                // Other players at this chest: it is theirs, leave it.
                let rivals = v.rivals.iter().filter(|r| ((r[0] - cc[0]).powi(2) + (r[2] - cc[2]).powi(2)).sqrt() <= SPAWN_RADIUS).count();
                ww.put(rivals as f64 / 2.0);
                let (ey, ep) = angle_err(yaw, l.pitch, eye, cc);
                ww.put(ey / 180.0);
                ww.put(ep / 90.0);
            } else {
                for _ in 0..CHEST_DIM {
                    ww.put(0.0);
                }
            }
        }

        // Mob projectiles coming in, nearest first.
        let mut pr: Vec<(f64, usize)> = v.projs.iter().enumerate().filter(|(_, p)| p.1 >= 0).map(|(i, p)| (((p.2[0] - pos[0]).powi(2) + (p.2[1] - pos[1]).powi(2) + (p.2[2] - pos[2]).powi(2)).sqrt(), i)).collect();
        pr.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        for k in 0..K_PROJ {
            if let Some(&(_, i)) = pr.get(k) {
                let (_, _, pp, pm_) = v.projs[i];
                let rel = [pp[0] - pos[0], pp[1] - (pos[1] + 0.9), pp[2] - pos[2]];
                let lr = local(yaw, rel);
                let lv = local(yaw, pm_);
                ww.put(1.0);
                ww.put(lr[0] / 16.0);
                ww.put(lr[1] / 16.0);
                ww.put(lr[2] / 16.0);
                ww.put(lv[0]);
                ww.put(lv[1]);
                ww.put(lv[2]);
                // Closest approach if it flew straight.
                let vv = pm_[0] * pm_[0] + pm_[1] * pm_[1] + pm_[2] * pm_[2];
                let t = if vv > 1e-6 { (-(rel[0] * pm_[0] + rel[1] * pm_[1] + rel[2] * pm_[2]) / vv).max(0.0) } else { 0.0 };
                let cpos = [rel[0] + pm_[0] * t, rel[1] + pm_[1] * t, rel[2] + pm_[2] * t];
                ww.put((t / 20.0).min(2.0));
                ww.put(((cpos[0] * cpos[0] + cpos[1] * cpos[1] + cpos[2] * cpos[2]).sqrt() / 4.0).min(2.0));
            } else {
                for _ in 0..PROJ_DIM {
                    ww.put(0.0);
                }
            }
        }
        // The player's own arrow in flight (for the Punch boost).
        let own = v.projs.iter().filter(|p| p.1 == -1 && p.0 == ProjKind::Arrow).min_by(|a, b| {
            let da = (a.2[0] - pos[0]).powi(2) + (a.2[2] - pos[2]).powi(2);
            let db = (b.2[0] - pos[0]).powi(2) + (b.2[2] - pos[2]).powi(2);
            da.partial_cmp(&db).unwrap()
        });
        if let Some(&(_, _, pp, pm_)) = own {
            let lr = local(yaw, [pp[0] - pos[0], pp[1] - (pos[1] + 0.9), pp[2] - pos[2]]);
            let lv = local(yaw, pm_);
            ww.put(1.0);
            ww.put(lr[0] / 8.0);
            ww.put(lr[1] / 8.0);
            ww.put(lr[2] / 8.0);
            ww.put(lv[0]);
            ww.put(lv[1]);
            ww.put(lv[2]);
            ww.put(0.0);
        } else {
            for _ in 0..OWN_DIM {
                ww.put(0.0);
            }
        }
        assert_eq!(ww.w, OBS_DIM);

        // What only the critic sees: the true state of the same chests.
        let mut w2 = 0usize;
        let mut putp = |x: f64| {
            pv[w2] = x as f32;
            w2 += 1;
        };
        for k in 0..K_CHESTS {
            if let Some(&ci) = order.get(k) {
                let ch = &g.chests[ci];
                for s in 0..4u8 {
                    putp((ch.state as u8 == s) as i32 as f64);
                }
                putp(((ch.ready_at - g.t).max(0) as f64 / 3600.0).min(1.0));
                putp(if ch.state == ChestState::Active { (g.cfg.wave_ticks - (g.t - ch.active_since)) as f64 / g.cfg.wave_ticks as f64 } else { 0.0 });
                putp(ch.mobs.iter().filter(|&&m| g.mobs[m].alive).count() as f64 / 5.0);
            } else {
                for _ in 0..7 {
                    putp(0.0);
                }
            }
        }
        putp(g.sp.hp as f64 / 20.0);
        putp(g.sp.absorption as f64 / 4.0);
        putp(g.mobs.iter().filter(|m| m.alive).count() as f64 / 10.0);
        putp(g.cfg.up as f64 / 5.0);
        putp(g.cfg.down as f64 / 5.0);
        putp(1.0 - self.ticks as f64 / self.ep_ticks.max(1) as f64);
        assert_eq!(w2, PRIV_DIM);
    }

    pub fn input_of(&self, a: &[i32]) -> Input {
        Input {
            forward: (a[0] - 1) as f32,
            strafe: (1 - a[1]) as f32,
            jump: a[2] == 1,
            sprint: a[3] == 1,
            dyaw: YAW_BINS[a[4] as usize].clamp(-MAX_DYAW, MAX_DYAW),
            dpitch: PITCH_BINS[a[5] as usize].clamp(-MAX_DPITCH, MAX_DPITCH),
            attack: a[6] == 1,
            use_: a[7] == 1,
            slot: if a[8] == 0 { 255 } else { (a[8] - 1) as u8 },
        }
    }

    /// One tick. Returns the reward and 0 (going on), 1 (died) or 2 (out of
    /// time).
    pub fn step(&mut self, a: &[i32]) -> (f32, u8) {
        let inp = self.input_of(a);
        let ev: Events = self.g.step(&inp);
        self.ticks += 1;
        self.update_known();
        let hp = self.g.view.hp + self.g.view.absorption;
        if hp < self.last_hp {
            self.since_hurt = 0;
        } else {
            self.since_hurt = (self.since_hurt + 1).min(1000);
        }
        self.last_hp = hp;
        let punch_paid = if self.p.punch_mode >= 0.5 { self.punch_pay(&ev) } else { 0 };
        let p = &self.p;
        let mut r = p.w_claim * ev.claims as f64 + p.w_kill * ev.kills as f64 + p.w_wave * ev.waves as f64 - p.w_reset * ev.resets as f64
            + p.w_dmg * (ev.dmg_dealt - ev.dmg_rival) as f64
            + p.w_dmg_rival * ev.dmg_rival as f64
            - p.w_hurt * ev.dmg_taken as f64
            - p.w_tp * ev.leash_tps as f64
            - p.w_waste * (ev.gapple_waste + ev.potion_waste) as f64
            - p.w_rival * ev.rival_near as f64;
        let turn = (inp.dyaw.abs() + inp.dpitch.abs()) as f64 / 15.0;
        r -= p.w_turn * turn * turn;
        if inp.dyaw.abs() >= 2.0 && self.last_dyaw.abs() >= 2.0 && inp.dyaw.signum() != self.last_dyaw.signum() {
            r -= p.w_flip;
        }
        self.last_dyaw = inp.dyaw;
        self.spin += (inp.dyaw - self.spin) / 20.0;
        let over = (self.spin.abs() - 3.0).max(0.0) as f64 / 5.0;
        r -= p.w_spin * over * over;
        {
            let pos = self.g.cp.l.body.pos;
            let mut nearest_active = f64::INFINITY;
            for ci in 0..self.g.chests.len() {
                if self.known[ci].seen && self.known[ci].state == 2 {
                    let c = self.g.chests[ci].center();
                    nearest_active = nearest_active.min(((c[0] - pos[0]).powi(2) + (c[2] - pos[2]).powi(2)).sqrt());
                }
            }
            if nearest_active.is_finite() && nearest_active > 8.0 {
                r -= p.w_away;
            }
            if p.punch_mode >= 0.5 {
                r += p.w_punch * punch_paid as f64;
            } else if ev.self_punch > 0 && !(nearest_active <= 8.0) {
                // Run 3e: a Punch shot at oneself pays when it carries the player toward a far chest:
                // no wave running close by and the nearest chest to work on more than 12 blocks off.
                let mut nearest_goal = f64::INFINITY;
                for ci in 0..self.g.chests.len() {
                    let k = &self.known[ci];
                    if k.seen && (k.state == 1 || k.state == 3) {
                        let c = self.g.chests[ci].center();
                        nearest_goal = nearest_goal.min(((c[0] - pos[0]).powi(2) + (c[2] - pos[2]).powi(2)).sqrt());
                    }
                }
                if nearest_goal.is_finite() && nearest_goal > 12.0 {
                    r += p.w_punch * ev.self_punch as f64;
                }
            }
            // Standing about.
            let t = self.ticks as u64;
            if ev.hits > 0 {
                self.last_hit = t;
            }
            let moved = ((pos[0] - self.idle_from[0]).powi(2) + (pos[2] - self.idle_from[2]).powi(2)).sqrt();
            if moved >= 1.5 {
                self.idle_from = pos;
                self.idle_tick = t;
            } else if t >= self.idle_tick + 40 && t >= self.last_hit + 40 {
                r -= p.w_idle;
            }
        }
        // Shaping by potential; the potential of a dead player is zero.
        let phi = if ev.died { 0.0 } else { self.potential() };
        r += p.gamma * phi - self.phi;
        self.phi = phi;
        let mut done = 0u8;
        if ev.died {
            r -= p.w_death;
            done = 1;
        } else if self.ticks >= self.ep_ticks {
            done = 2;
        }
        let s = &mut self.stats;
        s[1] += 1.0;
        s[2] += ev.claims as f64;
        s[3] += ev.kills as f64;
        s[4] += ev.waves as f64;
        s[5] += ev.resets as f64;
        s[6] += ev.died as i32 as f64;
        s[7] += ev.dmg_dealt as f64;
        s[8] += ev.dmg_taken as f64;
        s[9] += ev.leash_tps as f64;
        s[10] += ev.hits as f64;
        s[12] += ev.self_punch as f64;
        s[13] += ev.punch_boost;
        s[14] += ev.gapples as f64;
        s[15] += ev.gapple_waste as f64;
        s[16] += ev.potions as f64;
        s[17] += ev.potion_waste as f64;
        s[18] += ev.pearls as f64;
        s[19] = s[19].max(ev.max_simul as f64);
        s[20] += punch_paid as f64;
        if done != 0 {
            s[0] += 1.0;
            s[11] += self.g.cp.clicks as f64;
        }
        (r as f32, done)
    }
}

struct W<'a> {
    o: &'a mut [f32],
    w: usize,
}

impl W<'_> {
    #[inline]
    fn put(&mut self, x: f64) {
        self.o[self.w] = x as f32;
        self.w += 1;
    }
}

pub struct VecEnv {
    pub envs: Vec<Env>,
}

impl VecEnv {
    /// The teacher's controls for every environment.
    pub fn teacher(&mut self, out: &mut [i32]) {
        for (i, e) in self.envs.iter_mut().enumerate() {
            out[i * ACT_HEADS..(i + 1) * ACT_HEADS].copy_from_slice(&e.teacher());
        }
    }

    pub fn new(n: usize, seed: u64) -> VecEnv {
        let p = Params::default();
        VecEnv { envs: (0..n).map(|i| Env::new(p.clone(), seed.wrapping_mul(6364136223846793005).wrapping_add(i as u64 * 1442695040888963407 + 1))).collect() }
    }

    pub fn set_param(&mut self, id: i32, v: f64) -> bool {
        let mut ok = true;
        for e in &mut self.envs {
            ok &= e.p.set(id, v);
        }
        ok
    }

    pub fn reset(&mut self, obs: &mut [f32], privs: &mut [f32]) {
        for (i, e) in self.envs.iter_mut().enumerate() {
            e.reset();
            e.write_obs(&mut obs[i * OBS_DIM..(i + 1) * OBS_DIM], &mut privs[i * PRIV_DIM..(i + 1) * PRIV_DIM]);
        }
    }

    /// Step every environment on its own thread share. `fobs`/`fpriv` get
    /// the last observation of an episode that ended, before the reset.
    #[allow(clippy::too_many_arguments)]
    pub fn step(&mut self, actions: &[i32], obs: &mut [f32], privs: &mut [f32], rew: &mut [f32], done: &mut [u8], fobs: &mut [f32], fpriv: &mut [f32]) {
        let n = self.envs.len();
        let threads = std::thread::available_parallelism().map(|x| x.get()).unwrap_or(1).min(n).max(1);
        let chunk = n.div_ceil(threads);
        std::thread::scope(|sc| {
            let mut envs: &mut [Env] = &mut self.envs;
            let mut acts = actions;
            let mut o = &mut obs[..];
            let mut pv = &mut privs[..];
            let mut r = &mut rew[..];
            let mut d = &mut done[..];
            let mut fo = &mut fobs[..];
            let mut fp = &mut fpriv[..];
            while !envs.is_empty() {
                let k = chunk.min(envs.len());
                let (e1, e2) = std::mem::take(&mut envs).split_at_mut(k);
                envs = e2;
                let (a1, a2) = acts.split_at(k * ACT_HEADS);
                acts = a2;
                let (o1, o2) = std::mem::take(&mut o).split_at_mut(k * OBS_DIM);
                o = o2;
                let (p1, p2) = std::mem::take(&mut pv).split_at_mut(k * PRIV_DIM);
                pv = p2;
                let (r1, r2) = std::mem::take(&mut r).split_at_mut(k);
                r = r2;
                let (d1, d2) = std::mem::take(&mut d).split_at_mut(k);
                d = d2;
                let (f1, f2) = std::mem::take(&mut fo).split_at_mut(k * OBS_DIM);
                fo = f2;
                let (q1, q2) = std::mem::take(&mut fp).split_at_mut(k * PRIV_DIM);
                fp = q2;
                sc.spawn(move || {
                    for (i, e) in e1.iter_mut().enumerate() {
                        let (rr, dd) = e.step(&a1[i * ACT_HEADS..(i + 1) * ACT_HEADS]);
                        r1[i] = rr;
                        d1[i] = dd;
                        let ob = &mut o1[i * OBS_DIM..(i + 1) * OBS_DIM];
                        let pb = &mut p1[i * PRIV_DIM..(i + 1) * PRIV_DIM];
                        if dd != 0 {
                            e.write_obs(&mut f1[i * OBS_DIM..(i + 1) * OBS_DIM], &mut q1[i * PRIV_DIM..(i + 1) * PRIV_DIM]);
                            e.reset();
                        }
                        e.write_obs(ob, pb);
                    }
                });
            }
        });
    }

    pub fn take_stats(&mut self, out: &mut [f64]) {
        for x in out.iter_mut() {
            *x = 0.0;
        }
        for e in &mut self.envs {
            for i in 0..STATS_DIM {
                if i == 19 {
                    out[i] = out[i].max(e.stats[i]);
                } else {
                    out[i] += e.stats[i];
                }
            }
            e.stats = [0.0; STATS_DIM];
        }
    }
}

#[allow(dead_code)]
fn _kinds(_: MobKind) {}

fn bin_of(bins: &[f32], want: f64) -> i32 {
    let mut b = 0;
    for (i, &x) in bins.iter().enumerate() {
        if (x as f64 - want).abs() < (bins[b] as f64 - want).abs() {
            b = i;
        }
    }
    b as i32
}

impl Env {
    /// Yaw and pitch error from the crosshair to a point.
    fn aim_err(&self, at: V3) -> (f64, f64) {
        let l = &self.g.cp.l;
        let eye = [l.body.pos[0], l.body.pos[1] + EYE_HEIGHT, l.body.pos[2]];
        angle_err(l.yaw, l.pitch, eye, at)
    }

    /// A hand-written player that knows the true state of the game. Its
    /// choices start the network off (imitation) before it learns on its
    /// own; it is not meant to be good, only to show what the game is.
    pub fn teacher(&mut self) -> [i32; ACT_HEADS] {
        let g = &self.g;
        let pos = g.cp.l.body.pos;
        let v = &g.view;
        let mut a = [1, 1, 0, 1, bin_of(&YAW_BINS, 0.0), bin_of(&PITCH_BINS, 0.0), 0, 0, 0];
        let in_water = g.world.in_water(&g.cp.l.body.bb);
        let jump_need = g.cp.l.body.collided_h || in_water || v.slow_left > 0;
        // Heal first when it is safe enough.
        let near_mob = v.mobs.iter().map(|m| ((m.pos[0] - pos[0]).powi(2) + (m.pos[2] - pos[2]).powi(2)).sqrt()).fold(99.0, f64::min);
        if g.cp.using && g.cp.slot == 2 {
            a[7] = 1;
            a[0] = 1;
            if g.cp.use_ticks + 1 >= crate::game::EAT_TICKS {
                self.teacher_ate = g.t;
            }
            return a;
        }
        if v.hp < 8.0 && v.potions > 0 && g.cp.l.pitch > 70.0 {
            a[8] = 5;
            return a;
        }
        if v.hp < 8.0 && v.potions > 0 {
            a[5] = bin_of(&PITCH_BINS, (85.0 - g.cp.l.pitch) as f64);
            return a;
        }
        if v.absorption <= 0.0 && v.gapples > 0 && g.t - self.teacher_ate > 80 && (v.hp < 14.0 || near_mob > 8.0) && v.regen_left == 0 && (near_mob > 4.0 || v.hp < 10.0) {
            if g.cp.slot != 2 {
                a[8] = 3;
            } else {
                a[7] = 1;
            }
            return a;
        }
        // Fight: the mob of the wave that has been running longest, near.
        let mut target: Option<(f64, usize)> = None;
        for (i, m) in v.mobs.iter().enumerate() {
            let d = ((m.pos[0] - pos[0]).powi(2) + (m.pos[1] - pos[1]).powi(2) + (m.pos[2] - pos[2]).powi(2)).sqrt();
            // With the aimbot, another player's mobs cannot be hit: leave them.
            if d > 12.0 || (g.cfg.silent_aim && m.rival) {
                continue;
            }
            // Its chest, and how long that wave has been on.
            let mut age = 0i64;
            let mut best_d = 99.0;
            for (ci, ch) in g.chests.iter().enumerate() {
                let cc = ch.center();
                let dd = ((cc[0] - m.pos[0]).powi(2) + (cc[2] - m.pos[2]).powi(2)).sqrt();
                if dd < best_d && self.known[ci].state == 2 {
                    best_d = dd;
                    age = v.tick - self.known[ci].active_seen_at;
                }
            }
            let score = d - age as f64 * 0.05 - if m.mounted { 5.0 } else { 0.0 };
            if target.map_or(true, |t| score < t.0) {
                target = Some((score, i));
            }
        }
        if let Some((_, i)) = target {
            let m = v.mobs[i];
            let at = [m.pos[0], m.pos[1] + m.h * 0.55, m.pos[2]];
            let (ey, ep) = self.aim_err(at);
            let d = ((m.pos[0] - pos[0]).powi(2) + (m.pos[2] - pos[2]).powi(2)).sqrt();
            a[4] = bin_of(&YAW_BINS, ey);
            a[5] = bin_of(&PITCH_BINS, ep);
            a[8] = if g.cp.slot != 0 { 1 } else { 0 };
            a[0] = if d > 2.6 || m.mounted { 2 } else { 1 };
            // Sprint only to close in: a sprint hit pushes the mob away.
            a[3] = (d > 4.0) as i32;
            a[2] = jump_need as i32;
            a[6] = (ey.abs() < 12.0 && ep.abs() < 20.0 && d < 3.6 && m.hurt_time <= 1) as i32;
            return a;
        }
        // Travel: claim, else stand where the most chests close to ready
        // spawn together, else go toward the soonest.
        let mut goal: Option<(V3, usize, bool)> = None;
        let mut best = f64::MAX;
        for ci in 0..g.chests.len() {
            let k = self.known[ci];
            let cc = g.chests[ci].center();
            if v.rivals.iter().any(|r| ((r[0] - cc[0]).powi(2) + (r[2] - cc[2]).powi(2)).sqrt() <= SPAWN_RADIUS + 2.0) {
                continue;
            }
            let d = ((cc[0] - pos[0]).powi(2) + (cc[2] - pos[2]).powi(2)).sqrt();
            let eta = d / 6.0;
            let score = if !k.seen {
                1e6
            } else {
                match k.state {
                    3 => d * 0.1,
                    1 => 50.0 + d * 0.3,
                    2 => 1e5,
                    _ => 80.0 + (self.est_secs(ci) - eta).max(0.0) + d * 0.2,
                }
            };
            if score < best {
                best = score;
                goal = Some((cc, ci, k.state == 3));
            }
        }
        if let Some((mut gp, ci, claim)) = goal {
            if !claim {
                // A second chest within reach of the same spot: stand between.
                let cc = g.chests[ci].center();
                let mut pair: Option<V3> = None;
                for cj in 0..g.chests.len() {
                    if cj == ci || !self.known[cj].seen {
                        continue;
                    }
                    let c2 = g.chests[cj].center();
                    let dd = ((c2[0] - cc[0]).powi(2) + (c2[2] - cc[2]).powi(2)).sqrt();
                    let st = self.known[cj].state;
                    if dd < 22.0 && (st == 1 || (st == 0 && (self.est_secs(cj) - self.est_secs(ci)).abs() < 6.0)) {
                        pair = Some([(cc[0] + c2[0]) / 2.0, cc[1], (cc[2] + c2[2]) / 2.0]);
                    }
                }
                if let Some(p) = pair {
                    gp = p;
                }
            }
            let d = ((gp[0] - pos[0]).powi(2) + (gp[2] - pos[2]).powi(2)).sqrt();
            let (ey, ep) = self.aim_err(gp);
            a[4] = bin_of(&YAW_BINS, ey);
            if claim {
                a[5] = bin_of(&PITCH_BINS, ep);
                a[0] = if d > 2.2 { 2 } else { 1 };
                a[7] = (d < 4.0 && ey.abs() < 8.0 && ep.abs() < 15.0) as i32;
            } else {
                a[5] = bin_of(&PITCH_BINS, -(g.cp.l.pitch as f64) * 0.5);
                a[0] = if d > 2.0 { 2 } else { 1 };
                // Far to go on open ground: a punch shot.
                if d > 25.0 && ey.abs() < 10.0 && v.arrows > 0 && !in_water {
                    if g.cp.slot != 1 {
                        a[8] = 2;
                    } else if g.cp.using {
                        if g.cp.use_ticks < 5 {
                            a[7] = 1;
                        }
                    } else if g.cp.l.pitch > -40.0 {
                        a[5] = bin_of(&PITCH_BINS, (-45.0 - g.cp.l.pitch) as f64);
                    } else {
                        a[7] = 1;
                    }
                } else if g.cp.slot != 0 && !g.cp.using {
                    a[8] = 1;
                }
            }
            a[2] = jump_need as i32;
        }
        a
    }
}

impl Env {
    /// The map from above, for the viewer: per column the height of the
    /// top solid block and whether water lies over it, as JSON.
    pub fn map_json(&self) -> String {
        let w = &self.g.world;
        let mut h = String::new();
        let mut wa = String::new();
        for z in 0..w.sz {
            for x in 0..w.sx {
                let g = w.ground(x, z);
                let mut top = g;
                while top < w.sy && w.water(x, top, z) {
                    top += 1;
                }
                if !h.is_empty() {
                    h.push(',');
                    wa.push(',');
                }
                h.push_str(&g.to_string());
                wa.push(if top > g { '1' } else { '0' });
            }
        }
        format!("{{\"sx\":{},\"sz\":{},\"h\":[{}],\"water\":[{}]}}", w.sx, w.sz, h, wa)
    }

    /// The true state of this tick, for the viewer, as JSON.
    pub fn frame_json(&self) -> String {
        let g = &self.g;
        let p = &g.sp;
        let mut s = format!(
            "{{\"t\":{},\"p\":[{:.2},{:.2},{:.2},{:.1}],\"hp\":{:.1},\"abs\":{:.1},\"slot\":{},\"using\":{}",
            g.t, p.pos[0], p.pos[1], p.pos[2], g.cp.l.yaw, p.hp, p.absorption, g.cp.slot, g.cp.using as i32
        );
        s.push_str(",\"mobs\":[");
        let mut first = true;
        for m in g.mobs.iter().filter(|m| m.alive) {
            if !first {
                s.push(',');
            }
            first = false;
            s.push_str(&format!("[{:.2},{:.2},{:.2},{},{:.0},{:.0},{},{}]", m.l.body.pos[0], m.l.body.pos[1], m.l.body.pos[2], m.kind as i32, m.hp, m.st.hp, m.chest, m.hurt_time));
        }
        s.push_str("],\"chests\":[");
        for (i, c) in g.chests.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("[{},{},{},{},{},{}]", c.block[0], c.block[1], c.block[2], c.state as i32, c.shown_secs(g.t), if c.state == ChestState::Active { g.cfg.wave_ticks - (g.t - c.active_since) } else { 0 }));
        }
        s.push_str("],\"projs\":[");
        let mut first = true;
        for q in g.projs.iter().filter(|q| q.alive) {
            if !first {
                s.push(',');
            }
            first = false;
            s.push_str(&format!("[{:.2},{:.2},{:.2},{},{}]", q.pos[0], q.pos[1], q.pos[2], q.kind as i32, q.owner));
        }
        s.push_str("],\"rivals\":[");
        for (i, r) in g.rivals.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("[{:.2},{:.2},{:.2}]", r.pos[0], r.pos[1], r.pos[2]));
        }
        s.push_str(&format!("],\"claims\":{},\"kills\":{},\"resets\":{}}}", self.stats[2], self.stats[3], self.stats[5]));
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;

    /// A player on flat ground sprinting along +z (or -z) with a chest 70
    /// blocks up +z; with `shoot`, a Punch II shot at itself on the way.
    fn run(shoot: bool, away: bool, p: Params) -> (f64, f64) {
        let mut e = Env::new(p, 5);
        let mut cfg = e.g.cfg.clone();
        cfg.speed_amp = 1;
        cfg.punch = 2.0;
        cfg.up = 4;
        cfg.down = 4;
        cfg.jitter = 0;
        let start = PlayerStart { yaw: if away { 180.0 } else { 0.0 }, hp: 20.0, gapples: 0, pearls: 0, potions: 0, arrows: 64 };
        e.g = Game::new(World::flat(64, 32, 400, 8), &[[30, 8, 150]], [30.5, 8.0, 80.5], cfg, Rng::new(5), &[99999], start);
        e.known = vec![Known::default(); 1];
        e.holo_range = 500.0;
        e.punch = PunchWatch::default();
        e.stats = [0.0; STATS_DIM];
        let (y0, p0, p_dn, p_up) = (bin_of(&YAW_BINS, 0.0), bin_of(&PITCH_BINS, 0.0), bin_of(&PITCH_BINS, -7.0), bin_of(&PITCH_BINS, 7.0));
        for t in 0..120 {
            let mut a = [2, 1, 0, 1, y0, p0, 0, 0, if t == 0 { 2 } else { 0 }];
            if shoot && (20..25).contains(&t) {
                a[7] = 1;
                a[5] = p_dn;
            } else if t >= 25 && e.g.cp.l.pitch < 0.0 {
                a[5] = p_up;
            }
            e.step(&a);
        }
        (e.stats[12], e.stats[20])
    }

    #[test]
    fn punch_pays_only_for_travel_toward_the_chest() {
        let (hits, paid) = run(true, false, Params::default());
        assert_eq!((hits, paid), (1.0, 1.0), "a boost toward the chest");
        assert_eq!(run(false, false, Params::default()), (0.0, 0.0), "sprinting alone");
        let (hits, paid) = run(true, true, Params::default());
        assert_eq!((hits, paid), (1.0, 0.0), "a boost away from the chest");
        let mut p = Params::default();
        p.punch_gain = 20.0;
        assert_eq!(run(true, false, p).1, 0.0, "a boost that gains less than the bar");
    }
}
