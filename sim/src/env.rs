//! The training environment around a duel: action decoding, observations,
//! rewards, episode resets with randomised conditions, and a batch of
//! environments stepped together.
//!
//! Stepping is per client tick. In self-play the two agents take turns
//! (agent 0 ticks, then agent 1, 50 ms per round), because their clocks are
//! out of phase and each must see what reached it before its own tick. Each
//! call returns the observation of the agent that acts next, together with
//! the reward that agent earned since its previous action.
use crate::aabb::{dist, Aabb, V3};
use crate::bot::{self, BotParams, BotState};
use crate::client::{pearl_landing, pearl_preview, ClientCfg, Edge, Input, TickOut, MAX_BACKTRACK};
use crate::mth::{self, Rng};
use crate::net::{Item, SLOT_PEARL, SLOT_SWORD};
use crate::server::{KbPlugin, KbProfile, ServerCfg, PEARL_DAMAGE};
use crate::sim::{Match, NetCfg, Spawn, TICK_US};
use crate::world::World;

pub const ACT_HEADS: usize = 9;
/// forward, strafe, jump, sprint key, attack, yaw bin, pitch bin, right
/// mouse button (block with the sword), throw a pearl.
///
/// The last one stands for three key presses a player makes in a row: the
/// number key of the pearls with a right click, then the number key of the
/// sword on the next tick (see `Env::pearl_keys`). It does nothing while
/// the server's cooldown runs or when no pearl is left.
pub const ACT_DIMS: [i32; ACT_HEADS] = [3, 3, 2, 2, 2, 27, 21, 2, 2];
/// Mouse counts per tick. One count is 0.15 degrees at sensitivity 0.5.
pub const YAW_BINS: [i32; 27] = [
    -377, -233, -144, -89, -55, -34, -21, -13, -8, -5, -3, -2, -1, 0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377,
];
pub const PITCH_BINS: [i32; 21] = [-89, -55, -34, -21, -13, -8, -5, -3, -2, -1, 0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89];

pub const OBS_DIM: usize = 96;
pub const PRIV_DIM: usize = 54;
pub const STATS_DIM: usize = 38;
/// Floats per player and in total written by `Env::write_view`.
pub const VIEW_PLAYER: usize = 30;
pub const VIEW_DIM: usize = 2 * VIEW_PLAYER + 22;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// Both players are driven from outside, alternating.
    SelfPlay,
    /// Player 0 is driven from outside, player 1 is a scripted bot.
    Bot,
}

#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub floor_y: f64,
    pub arena_half: f64,
    pub walls: bool,
    pub speed_amp: i8,
    pub ping_min_ms: f64,
    pub ping_max_ms: f64,
    /// Ping range of player 0, the side that stands for the agent as it
    /// will be used. A negative minimum means the common range above. In
    /// self-play player 1 keeps the common range and plays the opponent
    /// with an ordinary connection.
    pub agent_ping_min_ms: f64,
    pub agent_ping_max_ms: f64,
    /// Clicks per second player 0 can make, drawn per duel from this
    /// range. Below twenty it is a budget: a click that would go over it is
    /// not made, and no second of play holds more than the rate rounded up.
    /// Twenty or more means a click every tick is allowed.
    pub cps_min: f64,
    pub cps_max: f64,
    /// The same for player 1, the opponent. Above twenty the opponent also
    /// clicks twice in some ticks, and scripted bots click on every tick
    /// they are on target. A negative minimum means: a self-play copy
    /// follows the range above and bots click as their level says.
    pub opp_cps_min: f64,
    pub opp_cps_max: f64,
    /// Chance that a duel gives player 1, the opponent, an edge over the
    /// plain game, drawn from the three ranges below: a longer attack
    /// reach in blocks, a smaller share of the horizontal knockback taken,
    /// and hits that still land where the agent was shown up to so many
    /// milliseconds ago (backtrack). The agent is not told; it has to
    /// notice from how the duel goes.
    pub opp_edge_prob: f64,
    pub opp_reach: [f64; 2],
    pub opp_kb_taken: [f64; 2],
    pub opp_backtrack_ms: [f64; 2],
    /// Among the duels that `kb_random` gives a knockback other than the
    /// game's, the share that get it the way server plugins set it
    /// (`KbPlugin`: separate numbers on the ground and in the air, an
    /// optional height limit, sprint knockback with or without a sprint
    /// reset). The rest get the game's formula with other numbers. All of
    /// them may also get a shorter time between hits.
    pub kb_plugin: f64,
    /// 0 draws the knockback as described above. 1 always uses the profile
    /// the KnockbackManager plugin ships with.
    pub kb_preset: i32,
    pub jitter_max_ms: f64,
    pub start_dist_min: f64,
    pub start_dist_max: f64,
    pub start_yaw_noise: f64,
    pub ep_ticks_min: f64,
    pub ep_ticks_max: f64,
    pub w_hit: f32,
    pub w_taken: f32,
    pub w_aim: f32,
    pub w_ontarget: f32,
    pub w_range: f32,
    pub w_swing_miss: f32,
    /// Extra reward for a hit that carried sprint knockback. Zero leaves
    /// the reward as hits dealt minus hits taken; a small value early in
    /// training points the agent at sprint resets, which pay off through
    /// the hits the opponent then cannot return.
    pub w_sprint_hit: f32,
    /// Reward per own tick for having the opponent within reach (as this
    /// client sees it) while being out of the opponent's reach (as that
    /// client sees it), and the same amount taken away in the opposite
    /// case. It is the spacing that makes a combo, paid directly. Zero
    /// turns it off.
    pub w_reach_adv: f32,
    /// Extra reward for each hit after the first in an unanswered run, per
    /// hit already in the run (at most five), and the same amount taken
    /// from the player on the receiving end. Zero leaves every hit worth
    /// the same.
    pub w_combo: f32,
    /// Cost per own tick of standing closer to the opponent than
    /// `close_dist`, growing to the full amount at zero distance. Fights
    /// at arm's length are where nobody can be kept out of reach; this
    /// points early training away from them. Zero turns it off.
    pub w_close: f32,
    pub close_dist: f64,
    pub mouse_delay_fix: bool,
    pub infinite_health: bool,
    pub fall_damage: bool,
    pub kb_random: f64,
    pub kb_h: [f64; 2],
    pub kb_v: [f64; 2],
    pub kb_friction: [f64; 2],
    pub kb_extra_h: [f64; 2],
    pub kb_extra_v: [f64; 2],
    pub bot_level: [f64; 2],
    pub sensitivity: f32,
    pub damage: f32,
    pub kb_ench: i32,
    /// How much the damage of a hit counts in its reward. 0: every hit is
    /// worth the same. 1: a hit is worth its damage relative to a plain
    /// one, so a critical hit counts one and a half times and a hit on a
    /// blocking player a little over half.
    pub w_damage: f32,
    /// Among the opponents with an edge, the share that also block around
    /// every hit they take (the server always has them blocking, and they
    /// are shown blocking) without slowing down or losing clicks.
    pub opp_autoblock: f64,
    /// Chance that a hit costs the sword durability, which makes the
    /// server send the sword again (see `ServerCfg::sword_wear`). Negative
    /// draws it per duel: a plain sword, Unbreaking III or unbreakable.
    pub sword_wear: f64,
    /// Scales how often scripted bots block. 0 turns their blocking off.
    pub bot_block: f64,
    /// Ender pearls each player starts a duel with, in the share
    /// `pearl_prob` of the duels. The others are fought without pearls.
    pub pearls: f64,
    pub pearl_prob: f64,
    /// Seconds a server makes a player wait between two pearls, drawn per
    /// duel from this range. Zero is the plain game, which has no wait.
    pub pearl_cd_s: [f64; 2],
    /// Scales how often scripted bots throw pearls. 0 turns it off.
    pub bot_pearl: f64,
    /// Ticks the bots aim ahead of their opponent on top of the flight of
    /// the pearl.
    pub bot_pearl_lead: f64,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            floor_y: 4.0,
            arena_half: 24.0,
            walls: true,
            speed_amp: 1,
            ping_min_ms: 10.0,
            ping_max_ms: 120.0,
            agent_ping_min_ms: 160.0,
            agent_ping_max_ms: 230.0,
            cps_min: 14.0,
            cps_max: 16.9,
            opp_cps_min: 20.0,
            opp_cps_max: 24.0,
            opp_edge_prob: 0.0,
            opp_reach: [3.0, 3.5],
            opp_kb_taken: [0.6, 1.0],
            opp_backtrack_ms: [0.0, 200.0],
            kb_plugin: 0.5,
            kb_preset: 0,
            jitter_max_ms: 2.0,
            start_dist_min: 2.0,
            start_dist_max: 10.0,
            start_yaw_noise: 180.0,
            ep_ticks_min: 600.0,
            ep_ticks_max: 1200.0,
            w_hit: 1.0,
            w_taken: 1.0,
            w_aim: 1.0,
            w_ontarget: 0.005,
            w_range: 0.5,
            w_swing_miss: 0.0,
            w_sprint_hit: 0.0,
            w_reach_adv: 0.0,
            w_combo: 0.0,
            w_close: 0.0,
            close_dist: 2.0,
            mouse_delay_fix: true,
            infinite_health: true,
            fall_damage: true,
            kb_random: 0.0,
            kb_h: [0.3, 0.5],
            kb_v: [0.3, 0.45],
            kb_friction: [1.6, 2.4],
            kb_extra_h: [0.3, 0.6],
            kb_extra_v: [0.05, 0.12],
            bot_level: [0.0, 4.0],
            sensitivity: 0.5,
            damage: 8.0,
            kb_ench: 0,
            w_damage: 0.0,
            opp_autoblock: 0.3,
            sword_wear: -1.0,
            bot_block: 1.0,
            pearls: 16.0,
            pearl_prob: 0.8,
            pearl_cd_s: [10.0, 16.0],
            bot_pearl: 1.0,
            bot_pearl_lead: 3.0,
        }
    }
}

impl Params {
    /// Numbered access for the C interface. The numbering is part of the
    /// interface: add at the end, never reorder.
    pub fn set(&mut self, id: i32, v: f64) -> bool {
        match id {
            0 => self.arena_half = v,
            1 => self.walls = v != 0.0,
            2 => self.speed_amp = v as i8,
            3 => self.ping_min_ms = v,
            4 => self.ping_max_ms = v,
            5 => self.jitter_max_ms = v,
            6 => self.start_dist_min = v,
            7 => self.start_dist_max = v,
            8 => self.start_yaw_noise = v,
            9 => self.ep_ticks_min = v,
            10 => self.ep_ticks_max = v,
            11 => self.w_hit = v as f32,
            12 => self.w_taken = v as f32,
            13 => self.w_aim = v as f32,
            14 => self.w_ontarget = v as f32,
            15 => self.w_range = v as f32,
            16 => self.w_swing_miss = v as f32,
            17 => self.mouse_delay_fix = v != 0.0,
            18 => self.infinite_health = v != 0.0,
            19 => self.fall_damage = v != 0.0,
            20 => self.kb_random = v,
            21 => self.kb_h[0] = v,
            22 => self.kb_h[1] = v,
            23 => self.kb_v[0] = v,
            24 => self.kb_v[1] = v,
            25 => self.kb_friction[0] = v,
            26 => self.kb_friction[1] = v,
            27 => self.kb_extra_h[0] = v,
            28 => self.kb_extra_h[1] = v,
            29 => self.kb_extra_v[0] = v,
            30 => self.kb_extra_v[1] = v,
            31 => self.bot_level[0] = v,
            32 => self.bot_level[1] = v,
            33 => self.sensitivity = v as f32,
            34 => self.damage = v as f32,
            35 => self.kb_ench = v as i32,
            36 => self.floor_y = v,
            37 => self.w_sprint_hit = v as f32,
            38 => self.w_reach_adv = v as f32,
            39 => self.w_combo = v as f32,
            40 => self.w_close = v as f32,
            41 => self.close_dist = v,
            42 => self.agent_ping_min_ms = v,
            43 => self.agent_ping_max_ms = v,
            44 => self.cps_min = v,
            45 => self.cps_max = v,
            46 => self.opp_cps_min = v,
            47 => self.opp_cps_max = v,
            48 => self.opp_edge_prob = v,
            49 => self.opp_reach[0] = v,
            50 => self.opp_reach[1] = v,
            51 => self.opp_kb_taken[0] = v,
            52 => self.opp_kb_taken[1] = v,
            53 => self.opp_backtrack_ms[0] = v,
            54 => self.opp_backtrack_ms[1] = v,
            55 => self.kb_plugin = v,
            56 => self.kb_preset = v as i32,
            57 => self.w_damage = v as f32,
            58 => self.opp_autoblock = v,
            59 => self.sword_wear = v,
            60 => self.bot_block = v,
            61 => self.pearls = v,
            62 => self.pearl_prob = v,
            63 => self.pearl_cd_s[0] = v,
            64 => self.pearl_cd_s[1] = v,
            65 => self.bot_pearl = v,
            66 => self.bot_pearl_lead = v,
            _ => return false,
        }
        true
    }
}

pub fn decode_action(a: &[i32]) -> Input {
    let pick = |v: i32, n: usize| (v.max(0) as usize).min(n - 1);
    Input {
        forward: [0i8, 1, -1][pick(a[0], 3)],
        strafe: [0i8, 1, -1][pick(a[1], 3)],
        jump: a[2] != 0,
        sprint_key: a[3] != 0,
        attack: a[4] != 0,
        extra_clicks: 0,
        dx: YAW_BINS[pick(a[5], 27)],
        dy: PITCH_BINS[pick(a[6], 21)],
        use_item: a.len() > 7 && a[7] != 0,
        slot_key: 0,
    }
}

#[inline]
fn local(yaw: f32, x: f64, z: f64) -> (f64, f64) {
    let (s, c) = (yaw as f64).to_radians().sin_cos();
    // Forward is (-sin, cos); right is (-cos, -sin).
    (-s * x + c * z, -c * x - s * z)
}

#[inline]
fn clip(v: f64, lim: f64) -> f32 {
    v.clamp(-lim, lim) as f32
}

/// Yaw error, pitch error (degrees) and horizontal distance from an eye
/// with the given rotation to the nearest height of a box.
fn aim_error(eye: V3, yaw: f32, pitch: f32, bb: &Aabb) -> (f64, f64, f64) {
    let c = bb.center();
    let ty = eye[1].clamp(bb.min[1] + 0.2, bb.max[1] - 0.2);
    let (yaw_to, pitch_to) = bot::angles_to(eye, [c[0], ty, c[2]]);
    let dx = c[0] - eye[0];
    let dz = c[2] - eye[2];
    let h = (dx * dx + dz * dz).sqrt();
    if h < 1e-6 {
        return (0.0, 0.0, h);
    }
    (mth::wrap_deg(yaw_to - yaw as f64), pitch_to - pitch as f64, h)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub episodes: f64,
    pub agent_ticks: [f64; 2],
    pub hits: [f64; 2],
    pub sprint_hits: [f64; 2],
    pub swings: [f64; 2],
    pub attacks: [f64; 2],
    pub on_target: [f64; 2],
    pub aim_err_sum: [f64; 2],
    pub max_combo_sum: [f64; 2],
    pub reward_sum: [f64; 2],
    /// Own ticks spent blocking.
    pub use_ticks: [f64; 2],
    /// Hits this player took while the server had it blocking.
    pub blocked: [f64; 2],
    /// Damage dealt, before armour.
    pub damage: [f64; 2],
    pub crits: [f64; 2],
    /// Pearls thrown, and pearls that landed and moved their thrower.
    pub pearls: [f64; 2],
    pub landings: [f64; 2],
    /// After a pearl of this player landed: the times it got the next hit
    /// in, and the times the other player did, within two seconds.
    pub pearl_won: [f64; 2],
    pub pearl_lost: [f64; 2],
    /// Blocks started. `use_ticks` over this is how long one is held.
    pub blocks: [f64; 2],
    /// Sum over the hits dealt of the horizontal speed each gave the
    /// victim, blocks per tick.
    pub kb: [f64; 2],
    /// Hits dealt with sprint within six ticks of the end of a block: the
    /// block used as a sprint reset.
    pub tap_hits: [f64; 2],
}

impl Stats {
    fn add(&mut self, o: &Stats) {
        self.episodes += o.episodes;
        for a in 0..2 {
            self.agent_ticks[a] += o.agent_ticks[a];
            self.hits[a] += o.hits[a];
            self.sprint_hits[a] += o.sprint_hits[a];
            self.swings[a] += o.swings[a];
            self.attacks[a] += o.attacks[a];
            self.on_target[a] += o.on_target[a];
            self.aim_err_sum[a] += o.aim_err_sum[a];
            self.max_combo_sum[a] += o.max_combo_sum[a];
            self.reward_sum[a] += o.reward_sum[a];
            self.use_ticks[a] += o.use_ticks[a];
            self.blocked[a] += o.blocked[a];
            self.damage[a] += o.damage[a];
            self.crits[a] += o.crits[a];
            self.pearls[a] += o.pearls[a];
            self.landings[a] += o.landings[a];
            self.pearl_won[a] += o.pearl_won[a];
            self.pearl_lost[a] += o.pearl_lost[a];
            self.blocks[a] += o.blocks[a];
            self.kb[a] += o.kb[a];
            self.tap_hits[a] += o.tap_hits[a];
        }
    }
    pub fn write(&self, out: &mut [f64]) {
        let v = [
            self.episodes,
            self.agent_ticks[0],
            self.agent_ticks[1],
            self.hits[0],
            self.hits[1],
            self.sprint_hits[0],
            self.sprint_hits[1],
            self.swings[0],
            self.swings[1],
            self.attacks[0],
            self.attacks[1],
            self.on_target[0],
            self.on_target[1],
            self.aim_err_sum[0],
            self.max_combo_sum[0],
            self.reward_sum[0],
            self.use_ticks[0],
            self.use_ticks[1],
            self.blocked[0],
            self.blocked[1],
            self.damage[0],
            self.damage[1],
            self.crits[0],
            self.crits[1],
            self.pearls[0],
            self.pearls[1],
            self.landings[0],
            self.landings[1],
            self.pearl_won[0],
            self.pearl_won[1],
            self.pearl_lost[0],
            self.pearl_lost[1],
            self.blocks[0],
            self.blocks[1],
            self.kb[0],
            self.kb[1],
            self.tap_hits[0],
            self.tap_hits[1],
        ];
        for (o, x) in out.iter_mut().zip(v.iter()) {
            *o = *x;
        }
    }
}

pub struct Env {
    pub m: Match,
    pub params: Params,
    pub mode: Mode,
    pub rng: Rng,
    pub pending: usize,
    pub bot: BotParams,
    bot_state: BotState,
    rew_acc: [f32; 2],
    /// Shaping potential after each agent's last tick.
    phi: [f32; 2],
    prev_inp: [Input; 2],
    ticks_left: i32,
    ep_len: i32,
    combo: [u32; 2],
    max_combo: [u32; 2],
    ep: Stats,
    pub total: Stats,
    done_flag: [u8; 2],
    stash_rew: [f32; 2],
    stash_obs: [[f32; OBS_DIM]; 2],
    stash_priv: [[f32; PRIV_DIM]; 2],
    ping_ms: [f64; 2],
    /// Click rate of each player in this duel and what is left of its
    /// budget.
    cps: [f64; 2],
    click_tokens: [f64; 2],
    /// Where each player is in the key presses of a pearl throw: 0 not
    /// throwing, 1 the right click comes next, 2 the sword comes next.
    pearl_stage: [u8; 2],
    /// Own ticks until each player may throw again, as it counts them
    /// itself from its last throw.
    pearl_wait: [i32; 2],
    asked_pearl: [bool; 2],
    /// The thrower of the last pearl that landed, and the server ticks left
    /// in which the next hit is counted for or against that pearl.
    landed: Option<(usize, i32)>,
    /// Own ticks since each player last had its sword up; 0 while it is.
    since_block: [i32; 2],
}

pub fn new_match(p: &Params, rng: &mut Rng, mode: Mode) -> (Match, [f64; 2], BotParams) {
    let world = World::arena(p.floor_y, p.arena_half, p.walls);
    let lim = (p.arena_half - 2.0).max(1.0);
    let mut a;
    let mut b;
    let mut tries = 0;
    loop {
        a = [rng.range(-lim, lim), p.floor_y, rng.range(-lim, lim)];
        let d = rng.range(p.start_dist_min, p.start_dist_max.max(p.start_dist_min));
        let ang = rng.range(0.0, std::f64::consts::TAU);
        b = [a[0] + d * ang.cos(), p.floor_y, a[2] + d * ang.sin()];
        tries += 1;
        if (b[0].abs() <= lim && b[2].abs() <= lim) || tries > 50 {
            b[0] = b[0].clamp(-lim, lim);
            b[2] = b[2].clamp(-lim, lim);
            break;
        }
    }
    let face = |from: V3, to: V3, rng: &mut Rng| -> f32 {
        let (yaw, _) = bot::angles_to(from, to);
        (yaw + rng.range(-p.start_yaw_noise, p.start_yaw_noise)) as f32
    };
    let spawns = [
        Spawn { pos: a, yaw: face(a, b, rng), pitch: rng.range(-8.0, 8.0) as f32 },
        Spawn { pos: b, yaw: face(b, a, rng), pitch: rng.range(-8.0, 8.0) as f32 },
    ];
    let mut ping = [0.0; 2];
    let mut net = NetCfg { up_us: [0; 2], down_us: [0; 2], jitter_us: [0; 2] };
    for i in 0..2 {
        let (lo, hi) = if i == 0 && p.agent_ping_min_ms >= 0.0 {
            (p.agent_ping_min_ms, p.agent_ping_max_ms)
        } else {
            (p.ping_min_ms, p.ping_max_ms)
        };
        ping[i] = rng.range(lo, hi.max(lo));
        let half = (ping[i] * 500.0) as i64;
        net.up_us[i] = half.max(1);
        net.down_us[i] = half.max(1);
        net.jitter_us[i] = (rng.range(0.0, p.jitter_max_ms.max(0.0)) * 1000.0) as i64;
    }
    // Client 0 ticks first; the other two clocks get a random phase.
    let c1 = 1 + (rng.next_u64() % (TICK_US as u64 - 1)) as i64;
    let mut sv = 1 + (rng.next_u64() % (TICK_US as u64 - 1)) as i64;
    if sv == c1 {
        sv = if c1 > 1 { c1 - 1 } else { c1 + 1 };
    }
    let mut kb = KbProfile::VANILLA;
    let mut plugin = None;
    let mut max_no_damage = 20;
    if p.kb_preset == 1 {
        plugin = Some(KbPlugin::DEFAULT);
    } else if rng.chance(p.kb_random) {
        if rng.chance(p.kb_plugin) {
            let hg = rng.range(0.28, 0.46);
            let vg = rng.range(0.30, 0.40);
            plugin = Some(KbPlugin {
                h_ground: hg,
                h_air: hg * rng.range(0.8, 1.1),
                h_sprint: rng.range(0.3, 0.6),
                v_ground: vg,
                v_air: vg * rng.range(0.6, 1.0),
                v_sprint: rng.range(0.0, 0.12),
                stop_sprint: rng.chance(0.7),
                y_limit: if rng.chance(0.6) { rng.range(0.5, 1.2) } else { -1.0 },
                v_after_limit: rng.range(-0.15, 0.0),
            });
        } else {
            let v = rng.range(p.kb_v[0], p.kb_v[1]);
            kb = KbProfile {
                friction: rng.range(p.kb_friction[0], p.kb_friction[1]),
                horizontal: rng.range(p.kb_h[0], p.kb_h[1]),
                vertical: v,
                vertical_limit: v.max(KbProfile::VANILLA.vertical_limit),
                extra_horizontal: rng.range(p.kb_extra_h[0], p.kb_extra_h[1]) as f32,
                extra_vertical: rng.range(p.kb_extra_v[0], p.kb_extra_v[1]),
            };
        }
        if rng.chance(0.25) {
            max_no_damage = 16 + (rng.next_u64() % 4) as i32;
        }
    }
    let mut ccfg = ClientCfg::with_sensitivity(p.sensitivity);
    ccfg.mouse_delay_fix = p.mouse_delay_fix;
    ccfg.kb_ench = p.kb_ench;
    let scfg = ServerCfg {
        damage: p.damage,
        ench_damage: 0.0,
        kb_ench: p.kb_ench,
        infinite_health: p.infinite_health,
        fall_damage: p.fall_damage,
        max_no_damage,
        sword_wear: if p.sword_wear >= 0.0 { p.sword_wear as f32 } else { [1.0f32, 0.25, 0.25, 0.0][(rng.next_u64() % 4) as usize] },
        pearl_cooldown: (rng.range(p.pearl_cd_s[0], p.pearl_cd_s[1].max(p.pearl_cd_s[0])) * 20.0).round().max(0.0) as i32,
    };
    let pearls = if rng.chance(p.pearl_prob) { p.pearls.clamp(0.0, 64.0) as u8 } else { 0 };
    let seed = rng.next_u64();
    let mut m = Match::new(world, spawns, p.speed_amp, kb, scfg, ccfg, net, [0, c1, sv], seed);
    m.s.plugin = plugin;
    m.s.pearl_guard = true;
    for i in 0..2 {
        m.s.p[i].pearls = pearls;
        m.c[i].lp.pearls = pearls;
    }
    if rng.chance(p.opp_edge_prob) {
        let ms = rng.range(p.opp_backtrack_ms[0], p.opp_backtrack_ms[1].max(p.opp_backtrack_ms[0]));
        m.c[1].edge = Edge {
            reach: rng.range(p.opp_reach[0], p.opp_reach[1].max(p.opp_reach[0])),
            kb_taken: rng.range(p.opp_kb_taken[0], p.opp_kb_taken[1].max(p.opp_kb_taken[0])),
            backtrack: ((ms / 50.0).round().max(0.0) as usize).min(MAX_BACKTRACK) as u8,
        };
        if rng.chance(p.opp_autoblock) {
            m.s.p[1].autoblock = true;
            m.s.p[1].set_using(true);
        }
    }
    let mut bot = if mode == Mode::Bot {
        let lo = p.bot_level[0].round() as i64;
        let hi = (p.bot_level[1].round() as i64).max(lo);
        BotParams::level((lo + (rng.next_u64() % (hi - lo + 1) as u64) as i64) as u8)
    } else {
        BotParams::level(0)
    };
    if p.opp_cps_min >= 20.0 && bot.click_prob > 0.0 {
        bot.click_prob = 1.0;
    }
    bot.blockhit *= p.bot_block;
    bot.block_react *= p.bot_block;
    bot.pearl_attack *= p.bot_pearl;
    bot.pearl_escape *= p.bot_pearl;
    bot.pearl_cd = m.s.cfg.pearl_cooldown;
    bot.pearl_lead = p.bot_pearl_lead;
    (m, ping, bot)
}

impl Env {
    pub fn new(mode: Mode, params: Params, seed: u64) -> Env {
        let mut rng = Rng::new(seed);
        let (m, ping, bot) = new_match(&params, &mut rng, mode);
        let mut e = Env {
            m,
            params,
            mode,
            rng,
            pending: 0,
            bot,
            bot_state: BotState::default(),
            rew_acc: [0.0; 2],
            phi: [0.0; 2],
            prev_inp: [Input::default(); 2],
            ticks_left: 0,
            ep_len: 0,
            combo: [0; 2],
            max_combo: [0; 2],
            ep: Stats::default(),
            total: Stats::default(),
            done_flag: [0; 2],
            stash_rew: [0.0; 2],
            stash_obs: [[0.0; OBS_DIM]; 2],
            stash_priv: [[0.0; PRIV_DIM]; 2],
            ping_ms: ping,
            cps: [20.0; 2],
            click_tokens: [2.0; 2],
            pearl_stage: [0; 2],
            pearl_wait: [0; 2],
            asked_pearl: [false; 2],
            landed: None,
            since_block: [1000; 2],
        };
        e.begin();
        e
    }

    fn begin(&mut self) {
        let p = &self.params;
        self.ep_len = self.rng.range(p.ep_ticks_min, p.ep_ticks_max.max(p.ep_ticks_min)).round() as i32;
        self.ticks_left = self.ep_len;
        self.rew_acc = [0.0; 2];
        self.prev_inp = [Input::default(); 2];
        self.combo = [0; 2];
        self.max_combo = [0; 2];
        self.ep = Stats::default();
        self.bot_state = BotState::default();
        self.pearl_stage = [0; 2];
        self.pearl_wait = [0; 2];
        self.asked_pearl = [false; 2];
        self.landed = None;
        self.since_block = [1000; 2];
        let own = self.rng.range(p.cps_min, p.cps_max.max(p.cps_min));
        let (lo, hi) = if p.opp_cps_min < 0.0 { (p.cps_min, p.cps_max) } else { (p.opp_cps_min, p.opp_cps_max) };
        let other = self.rng.range(lo, hi.max(lo));
        self.cps = [own.max(1.0), other.max(1.0)];
        for a in 0..2 {
            self.click_tokens[a] = 1.0 + (self.cps[a] / 20.0).min(1.0);
        }
        self.pending = self.m.advance();
        debug_assert_eq!(self.pending, 0);
        for a in 0..2 {
            self.phi[a] = self.potential(a).0;
        }
    }

    /// Start a fresh episode with newly drawn conditions.
    pub fn reset(&mut self) {
        let (m, ping, bot) = new_match(&self.params, &mut self.rng, self.mode);
        self.m = m;
        self.ping_ms = ping;
        self.bot = bot;
        self.done_flag = [0; 2];
        self.begin();
    }

    /// Pearls that landed: the thrower pays for the damage it took the way
    /// it pays for a hit taken, and the next hit is watched.
    fn collect_landings(&mut self) {
        for k in 0..self.m.s.landings.len() {
            let l = self.m.s.landings[k];
            let p = &self.params;
            self.rew_acc[l.thrower] -= p.w_taken * p.w_damage * (PEARL_DAMAGE / p.damage.max(0.5));
            self.ep.landings[l.thrower] += 1.0;
            self.landed = Some((l.thrower, 40));
        }
        self.m.s.landings.clear();
    }

    fn collect_hits(&mut self) {
        self.collect_landings();
        if self.m.s.hits.is_empty() {
            return;
        }
        if let Some((thrower, _)) = self.landed.take() {
            let first = self.m.s.hits[0];
            if first.attacker == thrower {
                self.ep.pearl_won[thrower] += 1.0;
            } else {
                self.ep.pearl_lost[thrower] += 1.0;
            }
        }
        let (wh, wt) = (self.params.w_hit, self.params.w_taken);
        for k in 0..self.m.s.hits.len() {
            let h = self.m.s.hits[k];
            let worth = 1.0 + self.params.w_damage * (h.damage / self.params.damage.max(0.5) - 1.0);
            self.rew_acc[h.attacker] += wh * worth;
            self.rew_acc[h.victim] -= wt * worth;
            self.ep.hits[h.attacker] += 1.0;
            self.ep.damage[h.attacker] += h.damage as f64;
            if h.blocked {
                self.ep.blocked[h.victim] += 1.0;
            }
            if h.crit {
                self.ep.crits[h.attacker] += 1.0;
            }
            self.ep.kb[h.attacker] += h.kb[0].hypot(h.kb[2]);
            if h.sprint {
                self.ep.sprint_hits[h.attacker] += 1.0;
                self.rew_acc[h.attacker] += self.params.w_sprint_hit;
                if (1..=6).contains(&self.since_block[h.attacker]) {
                    self.ep.tap_hits[h.attacker] += 1.0;
                }
            }
            let run = self.combo[h.attacker].min(5) as f32 * self.params.w_combo;
            self.rew_acc[h.attacker] += run;
            self.rew_acc[h.victim] -= run;
            self.combo[h.attacker] += 1;
            self.combo[h.victim] = 0;
            if self.combo[h.attacker] > self.max_combo[h.attacker] {
                self.max_combo[h.attacker] = self.combo[h.attacker];
            }
        }
        self.m.s.hits.clear();
    }

    /// Shaping potential of agent `id`: high when it faces the opponent and
    /// is within reach. Rewards pay the change of this value from one own
    /// tick to the next, which guides early learning without changing which
    /// behaviour is best. Also returns the aim error in degrees.
    fn potential(&self, id: usize) -> (f32, f64) {
        let p = &self.params;
        let c = &self.m.c[id];
        let eye = c.lp.eye();
        let (ey, ep, _) = aim_error(eye, c.lp.l.yaw, c.lp.l.pitch, &c.rp.bb);
        let ang = (ey * ey + ep * ep).sqrt().min(180.0);
        let ebb = c.rp.bb.expand(0.1, 0.1, 0.1);
        let reach = dist(eye, ebb.closest_point(eye));
        let aim = 1.0 - ang / 180.0;
        let range = 1.0 - (reach - 3.0).clamp(0.0, 12.0) / 12.0;
        (p.w_aim * aim as f32 + p.w_range * range as f32, ang)
    }

    /// Is the other player within attack reach of `id`, in `id`'s own view?
    fn in_reach(&self, id: usize) -> bool {
        let c = &self.m.c[id];
        let eye = c.lp.eye();
        let ebb = c.rp.bb.expand(0.1, 0.1, 0.1);
        dist(eye, ebb.closest_point(eye)) <= 3.0
    }

    /// Clicks player `id` can still make on its next tick.
    fn clicks_available(&self, id: usize) -> f64 {
        let r = (self.cps[id] / 20.0).min(1.0);
        (self.click_tokens[id] + r).min(1.0 + r)
    }

    /// Apply the click rate of player `id` to what it asked for: drop a
    /// click the budget does not cover, or add the second click of a tick
    /// for a rate above twenty.
    fn limit_clicks(&mut self, id: usize, inp: &mut Input) {
        let c = self.cps[id];
        if c >= 20.0 {
            if inp.attack && c > 20.0 && self.rng.chance((c - 20.0) / 20.0) {
                inp.extra_clicks = 1;
            }
            return;
        }
        let t = self.clicks_available(id);
        if inp.attack && t >= 1.0 - 1e-9 {
            self.click_tokens[id] = t - 1.0;
        } else {
            inp.attack = false;
            self.click_tokens[id] = t;
        }
    }

    /// Statistics of the episode under way.
    pub fn episode(&self) -> &Stats {
        &self.ep
    }

    /// Can player `id` throw a pearl now, as far as it can tell itself?
    fn pearl_ready(&self, id: usize) -> bool {
        let lp = &self.m.c[id].lp;
        lp.pearls > 0 && self.pearl_wait[id] <= 0 && self.pearl_stage[id] == 0 && lp.held == SLOT_SWORD
    }

    /// The key presses of a pearl throw, laid over what player `id` asked
    /// for on this tick. Tick one: the number key of the pearls and a right
    /// click, which throws along the view the server already has. Tick two:
    /// the number key of the sword. If the right button was down, it is let
    /// go on tick one and the click comes a tick later. No left click is
    /// made on the tick of the throw, because one that lands on a block
    /// would swallow the right click.
    fn pearl_keys(&mut self, id: usize, want: bool, inp: &mut Input) {
        if self.pearl_wait[id] > 0 {
            self.pearl_wait[id] -= 1;
        }
        self.asked_pearl[id] = want;
        match self.pearl_stage[id] {
            0 => {
                if want && self.pearl_ready(id) {
                    inp.slot_key = SLOT_PEARL + 1;
                    inp.attack = false;
                    if self.prev_inp[id].use_item {
                        inp.use_item = false;
                        self.pearl_stage[id] = 1;
                    } else {
                        inp.use_item = true;
                        self.pearl_stage[id] = 2;
                    }
                }
            }
            1 => {
                inp.use_item = true;
                inp.attack = false;
                self.pearl_stage[id] = 2;
            }
            _ => {
                inp.slot_key = SLOT_SWORD + 1;
                inp.use_item = false;
                self.pearl_stage[id] = 0;
            }
        }
    }

    /// Shaping terms and statistics for the tick `id` just made.
    fn account(&mut self, id: usize, inp: &Input, out: &TickOut) {
        let (phi, ang) = self.potential(id);
        let p = &self.params;
        let mut r = phi - self.phi[id];
        self.phi[id] = phi;
        if out.pick.entity {
            r += p.w_ontarget;
            self.ep.on_target[id] += 1.0;
        }
        if out.swung && !out.attacked {
            r -= p.w_swing_miss;
        }
        if p.w_reach_adv != 0.0 {
            let mine = self.in_reach(id);
            let theirs = self.in_reach(1 - id);
            if mine && !theirs {
                r += p.w_reach_adv;
            } else if theirs && !mine {
                r -= p.w_reach_adv;
            }
        }
        if p.w_close != 0.0 {
            let a = &self.m.c[0].lp.l.body.pos;
            let b = &self.m.c[1].lp.l.body.pos;
            let d = ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
            if d < p.close_dist {
                r -= p.w_close * ((p.close_dist - d) / p.close_dist) as f32;
            }
        }
        self.rew_acc[id] += r;
        self.ep.agent_ticks[id] += 1.0;
        self.ep.aim_err_sum[id] += ang;
        if out.swung {
            self.ep.swings[id] += 1.0;
        }
        if out.attacked {
            self.ep.attacks[id] += 1.0;
        }
        if self.m.c[id].lp.using_item {
            self.ep.use_ticks[id] += 1.0;
            if self.since_block[id] != 0 {
                self.ep.blocks[id] += 1.0;
            }
            self.since_block[id] = 0;
        } else if self.since_block[id] < 1000 {
            self.since_block[id] += 1;
        }
        if out.threw {
            self.ep.pearls[id] += 1.0;
            // The server starts its wait when the throw reaches it, a
            // little later than this; two ticks more cover the difference.
            self.pearl_wait[id] = self.m.s.cfg.pearl_cooldown + 2;
        }
        self.prev_inp[id] = *inp;
    }

    fn finish_episode(&mut self) {
        for a in 0..2 {
            self.ep.max_combo_sum[a] = self.max_combo[a] as f64;
            let mut o = [0.0f32; OBS_DIM];
            let mut pv = [0.0f32; PRIV_DIM];
            self.write_obs(a, &mut o);
            self.write_priv(a, &mut pv);
            self.stash_obs[a] = o;
            self.stash_priv[a] = pv;
            self.stash_rew[a] = self.rew_acc[a];
            self.ep.reward_sum[a] += self.rew_acc[a] as f64;
            self.done_flag[a] = 2;
        }
        self.ep.episodes = 1.0;
        let ep = self.ep;
        self.total.add(&ep);
        let (m, ping, bot) = new_match(&self.params, &mut self.rng, self.mode);
        self.m = m;
        self.ping_ms = ping;
        self.bot = bot;
        self.begin();
    }

    /// Apply the pending agent's action and run until an agent has to act
    /// again. Returns that agent's index.
    pub fn step(&mut self, action: &[i32]) -> usize {
        let id = self.pending;
        let mut inp = decode_action(action);
        self.pearl_keys(id, action.len() > 8 && action[8] != 0, &mut inp);
        self.limit_clicks(id, &mut inp);
        let out = self.m.client_tick(id, &inp);
        self.account(id, &inp, &out);
        let mut round_done = id == 1;
        let mut next = self.m.advance();
        self.collect_hits();
        if self.mode == Mode::Bot {
            while next == 1 {
                let mut b = bot::act(&self.m.c[1], &self.m.world, &self.m.ccfg, &self.bot, &mut self.bot_state, &mut self.rng);
                if self.params.opp_cps_min >= 0.0 {
                    self.limit_clicks(1, &mut b);
                }
                let o = self.m.client_tick(1, &b);
                self.account(1, &b, &o);
                next = self.m.advance();
                self.collect_hits();
            }
            round_done = true;
        }
        self.pending = next;
        if round_done {
            if let Some((who, left)) = self.landed {
                self.landed = if left > 1 { Some((who, left - 1)) } else { None };
            }
            self.ticks_left -= 1;
            if self.ticks_left <= 0 {
                self.finish_episode();
            }
        }
        self.pending
    }

    /// Reward and done flag for the pending agent's previous action. When
    /// done is non-zero the final observation of the finished episode is
    /// copied out as well. Clears what it returns.
    pub fn take_outcome(&mut self, fobs: &mut [f32], fpriv: &mut [f32]) -> (f32, u8) {
        let a = self.pending;
        if self.done_flag[a] != 0 {
            let d = self.done_flag[a];
            self.done_flag[a] = 0;
            fobs.copy_from_slice(&self.stash_obs[a]);
            fpriv.copy_from_slice(&self.stash_priv[a]);
            self.rew_acc[a] = 0.0;
            (self.stash_rew[a], d)
        } else {
            let r = self.rew_acc[a];
            self.rew_acc[a] = 0.0;
            self.ep.reward_sum[a] += r as f64;
            (r, 0)
        }
    }

    /// A snapshot for drawing the duel: for each player its own position
    /// and view angles, state flags, the keys it held on its last tick (for
    /// the attack key, how many times it clicked), its running hit counts, and where it currently sees the other player;
    /// then the episode clock, arena size, the two pings, the two click
    /// rates of this duel, the opponent's edge (reach, share of knockback
    /// taken, backtrack in milliseconds) and the server's knockback (0 the
    /// game's, 1 the game's formula with other numbers, 2 plugin style;
    /// horizontal, vertical, friction; ticks of `maxHurtResistantTime`).
    pub fn write_view(&self, o: &mut [f32]) {
        for id in 0..2 {
            let c = &self.m.c[id];
            let lp = &c.lp;
            let inp = &self.prev_inp[id];
            let b = id * VIEW_PLAYER;
            o[b] = lp.l.body.pos[0] as f32;
            o[b + 1] = lp.l.body.pos[1] as f32;
            o[b + 2] = lp.l.body.pos[2] as f32;
            o[b + 3] = lp.l.yaw;
            o[b + 4] = lp.l.pitch;
            o[b + 5] = lp.l.body.on_ground as i32 as f32;
            o[b + 6] = lp.l.sprinting as i32 as f32;
            o[b + 7] = lp.hurt_time as f32;
            o[b + 8] = lp.ticks_since_swing as f32;
            o[b + 9] = inp.forward as f32;
            o[b + 10] = inp.strafe as f32;
            o[b + 11] = inp.jump as i32 as f32;
            o[b + 12] = inp.sprint_key as i32 as f32;
            o[b + 13] = if inp.attack { 1.0 + inp.extra_clicks as f32 } else { 0.0 };
            o[b + 14] = self.ep.hits[id] as f32;
            o[b + 15] = self.combo[id] as f32;
            o[b + 16] = self.ep.sprint_hits[id] as f32;
            o[b + 17] = c.rp.pos[0] as f32;
            o[b + 18] = c.rp.pos[1] as f32;
            o[b + 19] = c.rp.pos[2] as f32;
            o[b + 20] = lp.using_item as i32 as f32;
            o[b + 21] = self.m.s.p[id].using_item as i32 as f32;
            o[b + 22] = self.ep.blocked[id] as f32;
            o[b + 23] = self.ep.damage[id] as f32;
            o[b + 24] = lp.held as f32;
            o[b + 25] = lp.pearls as f32;
            // The newest pearl of this player still in flight, as the
            // server has it.
            let e = self.m.s.pearls.iter().rev().find(|e| e.thrower == id);
            o[b + 26] = e.is_some() as u8 as f32;
            for a in 0..3 {
                o[b + 27 + a] = e.map(|e| e.pos[a] as f32).unwrap_or(0.0);
            }
        }
        let g = 2 * VIEW_PLAYER;
        o[g] = self.ticks_left as f32;
        o[g + 1] = self.ep_len as f32;
        o[g + 2] = self.params.arena_half as f32;
        o[g + 3] = self.params.floor_y as f32;
        o[g + 4] = self.ping_ms[0] as f32;
        o[g + 5] = self.ping_ms[1] as f32;
        o[g + 6] = self.cps[0] as f32;
        o[g + 7] = self.cps[1] as f32;
        let e = &self.m.c[1].edge;
        o[g + 8] = e.reach as f32;
        o[g + 9] = e.kb_taken as f32;
        o[g + 10] = e.backtrack as f32 * 50.0;
        let s = &self.m.s;
        match s.plugin {
            Some(k) => {
                o[g + 11] = 2.0;
                o[g + 12] = k.h_ground as f32;
                o[g + 13] = k.v_ground as f32;
                o[g + 14] = 0.0;
            }
            None => {
                let vanilla = s.kb.horizontal == KbProfile::VANILLA.horizontal && s.kb.friction == 2.0;
                o[g + 11] = if vanilla { 0.0 } else { 1.0 };
                o[g + 12] = s.kb.horizontal as f32;
                o[g + 13] = s.kb.vertical as f32;
                o[g + 14] = s.kb.friction as f32;
            }
        }
        o[g + 15] = s.cfg.max_no_damage as f32;
        o[g + 16] = s.p[1].autoblock as i32 as f32;
        o[g + 17] = s.cfg.sword_wear;
        o[g + 18] = s.cfg.pearl_cooldown as f32 / 20.0;
        // The last pearl of the scripted bot: why it was thrown and where
        // it was meant to land.
        o[g + 19] = self.bot_state.pearl_kind as f32;
        o[g + 20] = self.bot_state.pearl_to[0] as f32;
        o[g + 21] = self.bot_state.pearl_to[2] as f32;
    }

    /// What agent `id` can know from its own client.
    pub fn write_obs(&self, id: usize, o: &mut [f32]) {
        let p = &self.params;
        let c = &self.m.c[id];
        let lp = &c.lp;
        let rp = &c.rp;
        let b = &lp.l.body;
        let yaw = lp.l.yaw;
        let eye = lp.eye();
        let (vf, vr) = local(yaw, b.motion[0], b.motion[2]);
        o[0] = clip(vf / 0.5, 3.0);
        o[1] = clip(vr / 0.5, 3.0);
        o[2] = clip(b.motion[1] / 0.5, 3.0);
        o[3] = b.on_ground as u8 as f32;
        o[4] = b.collided_h as u8 as f32;
        o[5] = lp.l.sprinting as u8 as f32;
        o[6] = lp.server_sprint_state as u8 as f32;
        o[7] = lp.hurt_time as f32 / 10.0;
        o[8] = lp.l.jump_ticks as f32 / 10.0;
        o[9] = (b.fall_distance / 3.0).min(2.0);
        o[10] = lp.l.pitch / 90.0;
        let (sy, cy) = (yaw as f64).to_radians().sin_cos();
        o[11] = sy as f32;
        o[12] = cy as f32;
        o[13] = clip(b.pos[0] / p.arena_half, 1.5);
        o[14] = clip(b.pos[2] / p.arena_half, 1.5);
        o[15] = clip((b.pos[1] - p.floor_y) / 3.0, 2.0);
        o[16] = lp.l.attr.value() as f32 / 0.2;
        let (kf, kr) = local(yaw, lp.last_kb[0], lp.last_kb[2]);
        let kb_on = lp.ticks_since_kb < 40;
        o[17] = if kb_on { clip(kf / 0.5, 3.0) } else { 0.0 };
        o[18] = if kb_on { clip(kr / 0.5, 3.0) } else { 0.0 };
        o[19] = if kb_on { clip(lp.last_kb[1] / 0.5, 3.0) } else { 0.0 };
        o[20] = (lp.ticks_since_kb.min(40)) as f32 / 40.0;
        o[21] = (lp.ticks_since_swing.min(20)) as f32 / 20.0;
        o[22] = (lp.ticks_since_attack.min(20)) as f32 / 20.0;

        let rel = [rp.pos[0] - b.pos[0], rp.pos[1] - b.pos[1], rp.pos[2] - b.pos[2]];
        let (rf, rr) = local(yaw, rel[0], rel[2]);
        o[23] = clip(rf / 8.0, 4.0);
        o[24] = clip(rr / 8.0, 4.0);
        o[25] = clip(rel[1] / 3.0, 2.0);
        let (ey, ep, h) = aim_error(eye, yaw, lp.l.pitch, &rp.bb);
        o[26] = clip(h / 8.0, 4.0);
        o[27] = (ey / 180.0) as f32;
        o[28] = clip(ep / 90.0, 2.0);
        // Fine-grained copies of the aim error for when it is small.
        o[29] = clip(ey / 15.0, 1.0);
        o[30] = clip(ep / 15.0, 1.0);
        let tgt = [rp.target[0] - rp.pos[0], rp.target[1] - rp.pos[1], rp.target[2] - rp.pos[2]];
        let (tf, tr) = local(yaw, tgt[0], tgt[2]);
        let inc = rp.increments > 0;
        o[31] = if inc { clip(tf, 2.0) } else { 0.0 };
        o[32] = if inc { clip(tr, 2.0) } else { 0.0 };
        o[33] = if inc { clip(tgt[1], 2.0) } else { 0.0 };
        o[34] = rp.increments as f32 / 3.0;
        let vel = [rp.pos[0] - rp.prev_pos[0], rp.pos[1] - rp.prev_pos[1], rp.pos[2] - rp.prev_pos[2]];
        let (of, or) = local(yaw, vel[0], vel[2]);
        o[35] = clip(of / 0.5, 3.0);
        o[36] = clip(or / 0.5, 3.0);
        o[37] = clip(vel[1] / 0.5, 3.0);
        o[38] = rp.on_ground as u8 as f32;
        o[39] = rp.sprinting as u8 as f32;
        o[40] = rp.hurt_time as f32 / 10.0;
        o[41] = if rp.swing_in_progress { (rp.swing_progress_int.max(0) as f32 + 1.0) / 6.0 } else { 0.0 };
        let reye = [rp.pos[0], rp.pos[1] + crate::aabb::EYE_HEIGHT, rp.pos[2]];
        let (ry, _) = bot::angles_to(reye, eye);
        o[42] = (mth::wrap_deg(ry - rp.yaw as f64) / 180.0) as f32;
        o[43] = rp.pitch / 90.0;
        let pk = c.pick(&self.m.world, &self.m.ccfg);
        o[44] = pk.entity as u8 as f32;
        let ebb = rp.bb.expand(0.1, 0.1, 0.1);
        let reach = dist(eye, ebb.closest_point(eye));
        o[45] = clip(reach / 6.0, 3.0);
        o[46] = (reach <= 3.0) as u8 as f32;
        o[47] = clip((reach - 3.0) / 1.0, 1.0);
        o[48] = (rp.ticks_since_hurt.min(20)) as f32 / 20.0;
        o[49] = (lp.ticks_since_hurt.min(20)) as f32 / 20.0;
        o[50] = (self.ping_ms[id] / 200.0) as f32;
        o[51] = (self.ping_ms[1 - id] / 200.0) as f32;
        let pi = &self.prev_inp[id];
        let dpc = self.m.ccfg.mouse_f1 as f64 * 0.15;
        o[52] = pi.forward as f32;
        o[53] = pi.strafe as f32;
        o[54] = pi.jump as u8 as f32;
        o[55] = pi.sprint_key as u8 as f32;
        o[56] = pi.attack as u8 as f32;
        o[57] = clip(pi.dx as f64 * dpc / 30.0, 2.0);
        o[58] = clip(pi.dy as f64 * dpc / 15.0, 1.0);
        o[59] = (self.combo[id] as f32 / 10.0).min(2.0) - (self.combo[1 - id] as f32 / 10.0).min(2.0);
        let avail = self.clicks_available(id);
        o[60] = (avail >= 1.0 - 1e-9) as u8 as f32;
        o[61] = (avail / 2.0) as f32;
        o[62] = (self.cps[id] / 20.0) as f32;
        // Blocking: whether this client is blocking, how long until a held
        // button clicks again, whether the opponent is shown blocking, the
        // button on the last tick, and what the server last said about
        // this player's own block.
        o[63] = lp.using_item as u8 as f32;
        o[64] = lp.right_click_delay as f32 / 4.0;
        o[65] = rp.using as u8 as f32;
        o[66] = pi.use_item as u8 as f32;
        o[67] = lp.flag_using as u8 as f32;
        // Ender pearls: what is in hand, how many are left, whether one can
        // be thrown now and how much of the wait is left, where the throw's
        // key presses are, what was asked last tick, whether the opponent
        // holds pearls, and how long ago either player was moved at once.
        let cd = self.m.s.cfg.pearl_cooldown.max(1) as f32;
        o[68] = (lp.held == SLOT_SWORD) as u8 as f32;
        o[69] = lp.pearls as f32 / 16.0;
        o[70] = self.pearl_ready(id) as u8 as f32;
        o[71] = (self.pearl_wait[id].max(0) as f32 / cd).min(1.0);
        o[72] = self.pearl_stage[id] as f32 / 2.0;
        o[73] = self.asked_pearl[id] as u8 as f32;
        o[74] = (rp.item == Item::Pearl) as u8 as f32;
        o[75] = (lp.ticks_since_moved.min(20)) as f32 / 20.0;
        o[76] = (rp.ticks_since_jump.min(20)) as f32 / 20.0;
        // The newest pearl of its own in flight: where it will put this
        // player (relative to where it stands now), in how many ticks, and
        // how far from the opponent that is.
        for v in o[77..96].iter_mut() {
            *v = 0.0;
        }
        if let Some(e) = c.pearls.iter().rev().find(|e| e.mine) {
            let (land, ticks) = pearl_landing(&self.m.world, e);
            let (lf, lr) = local(yaw, land[0] - b.pos[0], land[2] - b.pos[2]);
            o[77] = 1.0;
            o[78] = clip(lf / 8.0, 4.0);
            o[79] = clip(lr / 8.0, 4.0);
            o[80] = clip((land[1] - b.pos[1]) / 3.0, 2.0);
            o[81] = (ticks.min(40)) as f32 / 20.0;
            o[82] = clip(dist(land, rp.pos) / 8.0, 4.0);
        }
        // The newest pearl of the opponent: where it is, where it will put
        // the opponent, in how many ticks, and how far the view has to
        // turn to face that spot.
        if let Some(e) = c.pearls.iter().rev().find(|e| !e.mine) {
            let (land, ticks) = pearl_landing(&self.m.world, e);
            let (pf, pr) = local(yaw, e.pos[0] - b.pos[0], e.pos[2] - b.pos[2]);
            let (lf, lr) = local(yaw, land[0] - b.pos[0], land[2] - b.pos[2]);
            o[83] = 1.0;
            o[84] = clip(pf / 8.0, 4.0);
            o[85] = clip(pr / 8.0, 4.0);
            o[86] = clip((e.pos[1] - b.pos[1]) / 3.0, 2.0);
            o[87] = clip(lf / 8.0, 4.0);
            o[88] = clip(lr / 8.0, 4.0);
            o[89] = clip((land[1] - b.pos[1]) / 3.0, 2.0);
            o[90] = (ticks.min(40)) as f32 / 20.0;
            let (ly, _) = bot::angles_to(eye, [land[0], land[1] + 1.0, land[2]]);
            o[91] = (mth::wrap_deg(ly - yaw as f64) / 180.0) as f32;
        }
        // A pearl thrown on this tick flies along the view the server has,
        // which is the view at the start of the tick: how far it would
        // carry this player, in how many ticks, and where that is seen from
        // the opponent (further along the view, to the right of it).
        if lp.pearls > 0 {
            let (land, ticks) = pearl_preview(&self.m.world, b.pos, lp.l.yaw, lp.l.pitch);
            let (lf, lr) = local(yaw, land[0] - b.pos[0], land[2] - b.pos[2]);
            o[92] = clip(lf.hypot(lr) / 16.0, 4.0);
            o[93] = (ticks.min(60)) as f32 / 30.0;
            o[94] = clip((lf - rf) / 8.0, 4.0);
            o[95] = clip((lr - rr) / 8.0, 4.0);
        }
    }

    /// Extra inputs for the value function only: things a client cannot see.
    pub fn write_priv(&self, id: usize, o: &mut [f32]) {
        let me = &self.m.c[id].lp;
        let other = &self.m.c[1 - id].lp;
        let yaw = me.l.yaw;
        let ob = &other.l.body;
        let mb = &me.l.body;
        let (rf, rr) = local(yaw, ob.pos[0] - mb.pos[0], ob.pos[2] - mb.pos[2]);
        o[0] = clip(rf / 8.0, 4.0);
        o[1] = clip(rr / 8.0, 4.0);
        o[2] = clip((ob.pos[1] - mb.pos[1]) / 3.0, 2.0);
        let (vf, vr) = local(yaw, ob.motion[0], ob.motion[2]);
        o[3] = clip(vf / 0.5, 3.0);
        o[4] = clip(vr / 0.5, 3.0);
        o[5] = clip(ob.motion[1] / 0.5, 3.0);
        let (ey, ep, _) = aim_error(other.eye(), other.l.yaw, other.l.pitch, &mb.bb);
        o[6] = (ey / 180.0) as f32;
        o[7] = clip(ep / 90.0, 2.0);
        let s = &self.m.s;
        o[8] = s.p[1 - id].hurt_resistant_time as f32 / 20.0;
        o[9] = s.p[id].hurt_resistant_time as f32 / 20.0;
        o[10] = s.p[id].l.sprinting as u8 as f32;
        o[11] = s.p[1 - id].l.sprinting as u8 as f32;
        o[12] = other.l.sprinting as u8 as f32;
        o[13] = ob.on_ground as u8 as f32;
        o[14] = (s.kb.friction / 2.0) as f32;
        o[15] = (s.kb.horizontal / 0.4) as f32;
        o[16] = (s.kb.vertical / 0.4) as f32;
        o[17] = (s.kb.vertical_limit / 0.4) as f32;
        o[18] = s.kb.extra_horizontal / 0.5;
        o[19] = (s.kb.extra_vertical / 0.1) as f32;
        let n = &self.m.net;
        o[20] = n.up_us[id] as f32 / 100_000.0;
        o[21] = n.down_us[id] as f32 / 100_000.0;
        o[22] = n.up_us[1 - id] as f32 / 100_000.0;
        o[23] = n.down_us[1 - id] as f32 / 100_000.0;
        let t = self.m.next_client[id];
        o[24] = ((self.m.next_server - t).rem_euclid(TICK_US)) as f32 / TICK_US as f32;
        o[25] = ((self.m.next_client[1 - id] - t).rem_euclid(TICK_US)) as f32 / TICK_US as f32;
        o[26] = (s.p[1 - id].update_counter % 2) as f32;
        o[27] = (s.p[id].update_counter % 2) as f32;
        o[28] = self.ticks_left as f32 / self.ep_len.max(1) as f32;
        o[29] = clip(dist(s.p[0].l.body.pos, s.p[1].l.body.pos) / 8.0, 4.0);
        o[30] = if self.mode == Mode::Bot { self.bot.level as f32 / 4.0 } else { -1.0 };
        o[31] = clip((self.ep.hits[id] - self.ep.hits[1 - id]) / 20.0, 3.0);
        // Blocking as the server has it, each player's edge, and the rest
        // of the server's settings.
        o[32] = s.p[id].using_item as u8 as f32;
        o[33] = s.p[1 - id].using_item as u8 as f32;
        for (k, who) in [1 - id, id].into_iter().enumerate() {
            let e = &self.m.c[who].edge;
            o[34 + 3 * k] = (e.reach - 3.0) as f32 * 2.0;
            o[35 + 3 * k] = e.kb_taken as f32;
            o[36 + 3 * k] = e.backtrack as f32 / 4.0;
        }
        match s.plugin {
            Some(k) => {
                o[40] = 1.0;
                o[41] = (k.h_ground / 0.4) as f32;
                o[42] = (k.v_ground / 0.4) as f32;
                o[45] = (k.h_air / 0.4) as f32;
                o[46] = (k.v_air / 0.4) as f32;
                o[47] = k.stop_sprint as u8 as f32;
            }
            None => {
                for k in [40, 41, 42, 45, 46, 47] {
                    o[k] = 0.0;
                }
            }
        }
        o[43] = s.cfg.max_no_damage as f32 / 20.0;
        o[44] = s.cfg.sword_wear;
        // Pearls as the server has them.
        let cd = s.cfg.pearl_cooldown.max(1) as f32;
        o[48] = s.p[id].pearl_wait as f32 / cd;
        o[49] = s.p[1 - id].pearl_wait as f32 / cd;
        o[50] = s.p[1 - id].pearls as f32 / 16.0;
        o[51] = (s.p[1 - id].held == SLOT_PEARL) as u8 as f32;
        o[52] = s.pearls.iter().any(|e| e.thrower != id) as u8 as f32;
        o[53] = s.cfg.pearl_cooldown as f32 / 320.0;
    }
}

/// A batch of environments with the same mode, stepped with one call.
pub struct VecEnv {
    pub envs: Vec<Env>,
    pub params: Params,
    pub mode: Mode,
}

impl VecEnv {
    pub fn new(n: usize, mode: Mode, seed: u64) -> VecEnv {
        let params = Params::default();
        let mut r = Rng::new(seed ^ 0xA5A5_5A5A_1234_5678);
        let envs = (0..n).map(|_| Env::new(mode, params, r.next_u64())).collect();
        VecEnv { envs, params, mode }
    }

    /// Takes effect at each environment's next episode.
    pub fn set_param(&mut self, id: i32, v: f64) -> bool {
        let ok = self.params.set(id, v);
        for e in self.envs.iter_mut() {
            e.params.set(id, v);
        }
        ok
    }

    pub fn reset(&mut self, obs: &mut [f32], privs: &mut [f32]) {
        for (i, e) in self.envs.iter_mut().enumerate() {
            e.reset();
            e.write_obs(e.pending, &mut obs[i * OBS_DIM..(i + 1) * OBS_DIM]);
            e.write_priv(e.pending, &mut privs[i * PRIV_DIM..(i + 1) * PRIV_DIM]);
        }
    }

    /// One action per environment for its pending agent. All environments
    /// of a batch have the same pending agent, which is returned.
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        actions: &[i32],
        obs: &mut [f32],
        privs: &mut [f32],
        rew: &mut [f32],
        done: &mut [u8],
        fobs: &mut [f32],
        fpriv: &mut [f32],
    ) -> usize {
        let mut pending = 0;
        for (i, e) in self.envs.iter_mut().enumerate() {
            pending = e.step(&actions[i * ACT_HEADS..(i + 1) * ACT_HEADS]);
            let (r, d) = e.take_outcome(&mut fobs[i * OBS_DIM..(i + 1) * OBS_DIM], &mut fpriv[i * PRIV_DIM..(i + 1) * PRIV_DIM]);
            rew[i] = r;
            done[i] = d;
            e.write_obs(pending, &mut obs[i * OBS_DIM..(i + 1) * OBS_DIM]);
            e.write_priv(pending, &mut privs[i * PRIV_DIM..(i + 1) * PRIV_DIM]);
        }
        pending
    }

    /// Totals over finished episodes since the last call.
    pub fn take_stats(&mut self, out: &mut [f64]) {
        let mut s = Stats::default();
        for e in self.envs.iter_mut() {
            s.add(&e.total);
            e.total = Stats::default();
        }
        s.write(out);
    }
}
