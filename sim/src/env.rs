//! The training environment around a duel: action decoding, observations,
//! rewards, episode resets with randomised conditions, and a batch of
//! environments stepped together.
//!
//! Stepping is per client tick. In self-play the two agents take turns
//! (agent 0 ticks, then agent 1, 50 ms per round), because their clocks are
//! out of phase and each must see what reached it before its own tick. Each
//! call returns the observation of the agent that acts next, together with
//! the reward that agent earned since its previous action.
use crate::aabb::{dist, dist_sq, Aabb, V3};
use crate::bot::{self, BotParams, BotState};
use crate::client::{pearl_landing, pearl_preview, ClientCfg, Edge, Input, TickOut, INV_SHUT, MAX_BACKTRACK};
use crate::mth::{self, Rng};
use crate::net::{potion_mask, refill_click, shift_click, Item, Kind, Rows, SLOT_FOOD, SLOT_PEARL, SLOT_SWORD, USE_TICKS, WINDOW_BAR};
use crate::server::{
    feather_points, prot_points, splash_heal, throw_heading, Food, KbPlugin, KbProfile, ServerCfg, DRINK_SHOWN, MAX_HEALTH, PEARL_DAMAGE,
    POTION_HEAL,
};
use crate::sim::{Match, NetCfg, Spawn, TICK_US};
use crate::world::{RayHit, World};

pub const ACT_HEADS: usize = 10;
/// forward, strafe, jump, sprint key, attack, yaw bin, pitch bin, right
/// mouse button (block with the sword), throw a pearl, and what to do with
/// the rest of the hotbar: nothing, throw a healing potion, drink the
/// potion of Speed, eat, stop drinking or eating, or get a healing potion
/// or a potion of Speed down from the inventory.
///
/// A pearl and a potion throw each stand for three key presses a player
/// makes in a row: the number key of the pearls or of a potion with a right
/// click, then the number key of the sword on the next tick (see
/// `Env::pearl_keys` and `Env::potion_keys`). The pearl does nothing while
/// the server's cooldown runs or when no pearl is left, the potion when no
/// potion is left. A drink or a meal is asked for once: the number key and
/// the right button, which then stays down until the server says it is
/// done or the player stops (see `Env::consume_keys`). A potion from the
/// inventory is the inventory key, a shift click on the potion on the next
/// tick, and the key again on the third; asked for on every tick, another
/// potion comes down on each (see `Env::refill_keys`).
pub const ACT_DIMS: [i32; ACT_HEADS] = [3, 3, 2, 2, 2, 27, 29, 2, 2, 7];
/// Mouse counts per tick. One count is 0.15 degrees at sensitivity 0.5.
pub const YAW_BINS: [i32; 27] = [
    -377, -233, -144, -89, -55, -34, -21, -13, -8, -5, -3, -2, -1, 0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377,
];
/// The eight large ones at the end came later, for looking at the ground
/// and back up within one tick: 600 counts are a quarter turn at the
/// sensitivity the agent plays with.
pub const PITCH_BINS: [i32; 29] = [
    -89, -55, -34, -21, -13, -8, -5, -3, -2, -1, 0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, -377, -233, -144, 144, 233, 377, -600, 600,
];

pub const OBS_DIM: usize = 151;
pub const PRIV_DIM: usize = 71;
pub const STATS_DIM: usize = 86;
/// Floats per player and in total written by `Env::write_view`.
pub const VIEW_PLAYER: usize = 45;
pub const VIEW_DIM: usize = 2 * VIEW_PLAYER + 24;

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
    /// Share of the duels fought with real health when `infinite_health`
    /// is set: diamond armour, hits that cost health, and a duel that ends
    /// when a player dies. Without `infinite_health` it is every duel.
    pub health_prob: f64,
    /// Level of Protection on each piece of armour and of Sharpness on the
    /// sword in such a duel, drawn per duel and the same for both players.
    pub prot: [f64; 2],
    pub sharp: [f64; 2],
    /// Level of Feather Falling on the boots.
    pub feather: f64,
    /// Splash potions of Instant Health II a player starts such a duel
    /// with, drawn per player (0 to 7).
    pub potions: [f64; 2],
    /// Chance that a player starts such a duel hurt, and the health it
    /// then has, drawn per player.
    pub start_hurt: f64,
    pub start_health: [f64; 2],
    /// What the player left standing gets when the other dies, and what the
    /// other loses.
    pub w_win: f32,
    /// Reward for health got back from a potion, per the health a plain hit
    /// costs, and the same amount taken from the other player.
    pub w_heal: f32,
    /// Cost of the part of an own potion that gave its thrower nothing, in
    /// the same unit: thrown with little health missing, or too far away.
    pub w_waste: f32,
    /// Scales how readily scripted bots heal themselves. 0 turns it off.
    pub bot_potion: f64,
    /// Running, jumping and fighting use up food.
    pub hunger: bool,
    /// Who may start hurt: 0 either player, 1 only the first (the agent
    /// against a bot), 2 only the second. For looking at single cases.
    pub hurt_who: u8,
    /// Pearls for one player only: 0 both, 1 only the first, 2 only the
    /// second.
    pub pearls_who: u8,
    /// Share of the duels with real health in which the Speed is that of a
    /// potion: it runs out, and a player can carry potions of it to drink.
    pub speed_end_prob: f64,
    /// The seconds of Speed a player has left when such a duel starts,
    /// drawn per player; none at all when the draw is not above zero.
    pub speed_left_s: [f64; 2],
    /// The potions of Speed it carries, drawn per player (0 to 2).
    pub drinks: [f64; 2],
    /// Share of the duels with real health in which running, jumping and
    /// fighting use up food and both players carry steak.
    pub hunger_prob: f64,
    /// The food a player starts such a duel with, drawn per player.
    pub start_food: [f64; 2],
    /// Speed that runs out for one player only: 0 both, 1 only the first,
    /// 2 only the second.
    pub speed_who: u8,
    /// Scales how readily scripted bots drink and eat. 0 turns it off.
    pub bot_drink: f64,
    /// What it is worth to a player, in hits, to have Speed and to have
    /// food enough to sprint. Paid as a difference of state: once when it
    /// gets there and taken back when it loses it, so that neither can be
    /// collected twice and drinking or eating by itself earns nothing.
    pub w_speed: f32,
    pub w_fed: f32,
    /// Share of the duels with real health in which the rows of the
    /// inventory hold more potions, to be got down to the hotbar.
    pub refill_prob: f64,
    /// The healing potions and the potions of Speed in those rows, drawn
    /// per player (the latter only in a duel in which the Speed runs out).
    pub inv_potions: [f64; 2],
    pub inv_drinks: [f64; 2],
    /// Stocked rows for one player only: 0 both, 1 only the first, 2 only
    /// the second.
    pub refill_who: u8,
    /// Scales how readily scripted bots go to their inventory. 0 turns it
    /// off.
    pub bot_refill: f64,
    /// What it costs a player, in hits, to stand with no healing potion in
    /// the hotbar while the inventory has some, and the same for the
    /// potion of Speed once the Speed is over. A difference of state, like
    /// `w_speed`: taken when it gets there and given back when it leaves.
    pub w_stock: f32,
    /// Share of the duels that start as a lesson: a situation in which
    /// one of the things to learn is plainly the thing to do, with the
    /// opponent far away. `lessons` says which, one bit each: 1 the first
    /// player has no Speed and a potion of it at hand, 2 it is too hungry
    /// to sprint and has steak, 4 it is hurt with no healing potion at
    /// hand and some in the inventory, 8 it has no Speed and its potion of
    /// it is in the inventory. The rest of the duel is drawn as always.
    pub lesson_prob: f64,
    pub lessons: u32,
    /// How far apart a lesson starts.
    pub lesson_dist: [f64; 2],
    /// The last seconds of Speed, in which a potion of it is worth
    /// drinking. An agent's drink is taken only with this much Speed left
    /// or less (or none): a potion drunk with a minute of Speed still to
    /// go is a potion thrown away, and an agent that tries the choice at
    /// random would have none left by the time one is needed. And `w_speed`
    /// counts the Speed a player has left up to this much, so it goes
    /// little by little over these seconds and a drink gets back what is
    /// gone. Less than zero: a drink at any time, and all of `w_speed` for
    /// any Speed at all.
    pub drink_window_s: f64,
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
            health_prob: 0.0,
            prot: [1.0, 3.0],
            sharp: [1.0, 3.0],
            feather: 4.0,
            potions: [0.0, 7.0],
            start_hurt: 0.5,
            start_health: [5.0, 18.0],
            w_win: 10.0,
            w_heal: 1.0,
            w_waste: 0.5,
            bot_potion: 1.0,
            hunger: false,
            hurt_who: 0,
            pearls_who: 0,
            speed_end_prob: 0.0,
            speed_left_s: [-10.0, 45.0],
            drinks: [0.0, 2.0],
            hunger_prob: 0.0,
            start_food: [5.0, 20.0],
            speed_who: 0,
            bot_drink: 1.0,
            w_speed: 0.0,
            w_fed: 0.0,
            refill_prob: 0.0,
            inv_potions: [3.0, 12.0],
            inv_drinks: [0.0, 2.0],
            refill_who: 0,
            bot_refill: 1.0,
            w_stock: 0.0,
            lesson_prob: 0.0,
            lessons: 15,
            lesson_dist: [10.0, 20.0],
            drink_window_s: 15.0,
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
            67 => self.health_prob = v,
            68 => self.prot[0] = v,
            69 => self.prot[1] = v,
            70 => self.sharp[0] = v,
            71 => self.sharp[1] = v,
            72 => self.feather = v,
            73 => self.potions[0] = v,
            74 => self.potions[1] = v,
            75 => self.start_hurt = v,
            76 => self.start_health[0] = v,
            77 => self.start_health[1] = v,
            78 => self.w_win = v as f32,
            79 => self.w_heal = v as f32,
            80 => self.w_waste = v as f32,
            81 => self.bot_potion = v,
            82 => self.hunger = v != 0.0,
            83 => self.hurt_who = v as u8,
            84 => self.pearls_who = v as u8,
            85 => self.speed_end_prob = v,
            86 => self.speed_left_s[0] = v,
            87 => self.speed_left_s[1] = v,
            88 => self.drinks[0] = v,
            89 => self.drinks[1] = v,
            90 => self.hunger_prob = v,
            91 => self.start_food[0] = v,
            92 => self.start_food[1] = v,
            93 => self.speed_who = v as u8,
            94 => self.bot_drink = v,
            95 => self.w_speed = v as f32,
            96 => self.w_fed = v as f32,
            97 => self.refill_prob = v,
            98 => self.inv_potions[0] = v,
            99 => self.inv_potions[1] = v,
            100 => self.inv_drinks[0] = v,
            101 => self.inv_drinks[1] = v,
            102 => self.refill_who = v as u8,
            103 => self.bot_refill = v,
            104 => self.w_stock = v as f32,
            105 => self.lesson_prob = v,
            106 => self.lessons = v as u32,
            107 => self.lesson_dist[0] = v,
            108 => self.lesson_dist[1] = v,
            109 => self.drink_window_s = v,
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
        dy: PITCH_BINS[pick(a[6], 29)],
        use_item: a.len() > 7 && a[7] != 0,
        use_again: false,
        slot_key: 0,
        inv: 0,
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

/// How far a player moves per tick if it goes on as it does now, from the
/// motion its last tick left it with.
#[inline]
fn pace(motion: V3, on_ground: bool) -> [f64; 2] {
    let f = if on_ground { 0.546 } else { 0.91 };
    [motion[0] / f, motion[2] / f]
}

/// What a potion at `pos` moving by `m` per tick and `age` ticks old will
/// do if both players keep moving along the ground as they do now: where it
/// breaks (its position at the start of that tick, from which its reach is
/// measured), after how many ticks, and the health it offers each player.
/// `feet` and `vel` are the two players, `thrower` the index of the one who
/// threw it, whom it passes through for its first ticks.
fn potion_outcome(world: &World, mut pos: V3, mut m: V3, mut age: i32, thrower: usize, feet: [V3; 2], vel: [[f64; 2]; 2]) -> (V3, i32, [f32; 2]) {
    let g = Kind::Potion.gravity() as f64;
    for t in 0..60 {
        age += 1;
        let next = [pos[0] + m[0], pos[1] + m[1], pos[2] + m[2]];
        let mut end = next;
        let mut broke = false;
        if !world.clear_path(pos, next) {
            if let RayHit::Block(h) = world.ray_trace_blocks(pos, next, false) {
                end = h;
                broke = true;
            }
        }
        let at = |j: usize| [feet[j][0] + vel[j][0] * t as f64, feet[j][1], feet[j][2] + vel[j][1] * t as f64];
        let mut direct = None;
        let mut best = 0.0;
        for j in 0..2 {
            if j == thrower && age < 5 {
                continue;
            }
            let f = at(j);
            let bb = Aabb::new([f[0] - 0.6, f[1] - 0.3, f[2] - 0.6], [f[0] + 0.6, f[1] + 2.1, f[2] + 0.6]);
            if let Some(h) = bb.calculate_intercept(pos, end) {
                let d = dist_sq(pos, h);
                if direct.is_none() || d < best {
                    direct = Some(j);
                    best = d;
                }
            }
        }
        if broke || direct.is_some() {
            let mut offered = [0.0f32; 2];
            for j in 0..2 {
                let f = at(j);
                if f[1] + 1.8 >= pos[1] - 2.0 && f[1] <= pos[1] + 2.25 {
                    offered[j] = splash_heal(pos, f, direct == Some(j));
                }
            }
            return (pos, t, offered);
        }
        pos = next;
        for v in m.iter_mut() {
            *v *= 0.99f32 as f64;
        }
        m[1] -= g;
    }
    (pos, 60, [0.0; 2])
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
    /// Times this player died.
    pub deaths: [f64; 2],
    /// Potions thrown, and potions that broke.
    pub potions: [f64; 2],
    pub splashes: [f64; 2],
    /// Health this player's potions gave back to itself and to the other
    /// player, and the health they offered it (eight each at best).
    pub heal_self: [f64; 2],
    pub heal_gift: [f64; 2],
    pub pot_offer: [f64; 2],
    /// How well a potion is thrown. Potions that offered their thrower all
    /// eight points; potions thrown within three ticks of the one before;
    /// the thrower's health at the throw, summed; the ticks its crosshair
    /// was more than twelve degrees off the other player around a throw
    /// (up to ten before it and forty after), summed; and the hits it took
    /// in the second after a throw.
    pub pot_perfect: [f64; 2],
    pub pot_double: [f64; 2],
    pub pot_health: [f64; 2],
    pub pot_away: [f64; 2],
    pub pot_hit: [f64; 2],
    /// Drinking and eating: the drinks and the meals a player finished and
    /// the ones it began, the ticks it spent on them and the hits it took
    /// meanwhile, the ticks it had Speed, and the ticks it had food enough
    /// to sprint.
    pub drinks: [f64; 2],
    pub meals: [f64; 2],
    pub sips: [f64; 2],
    pub sip_ticks: [f64; 2],
    pub sip_hit: [f64; 2],
    pub speed_on: [f64; 2],
    pub fed: [f64; 2],
    /// The part of `pot_away` that came after the throw: the ticks until
    /// the crosshair was back on the opponent. And all the ticks a player
    /// had its crosshair that far off the opponent, potion or not, to
    /// compare with.
    pub pot_after: [f64; 2],
    pub off: [f64; 2],
    /// The inventory: potions a player got down from its rows, the times
    /// it opened the screen, the ticks it had it open and the hits it took
    /// meanwhile.
    pub refills: [f64; 2],
    pub inv_opens: [f64; 2],
    pub inv_ticks: [f64; 2],
    pub inv_hit: [f64; 2],
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
            self.deaths[a] += o.deaths[a];
            self.potions[a] += o.potions[a];
            self.splashes[a] += o.splashes[a];
            self.heal_self[a] += o.heal_self[a];
            self.heal_gift[a] += o.heal_gift[a];
            self.pot_offer[a] += o.pot_offer[a];
            self.pot_perfect[a] += o.pot_perfect[a];
            self.pot_double[a] += o.pot_double[a];
            self.pot_health[a] += o.pot_health[a];
            self.pot_away[a] += o.pot_away[a];
            self.pot_hit[a] += o.pot_hit[a];
            self.drinks[a] += o.drinks[a];
            self.meals[a] += o.meals[a];
            self.sips[a] += o.sips[a];
            self.sip_ticks[a] += o.sip_ticks[a];
            self.sip_hit[a] += o.sip_hit[a];
            self.speed_on[a] += o.speed_on[a];
            self.fed[a] += o.fed[a];
            self.pot_after[a] += o.pot_after[a];
            self.off[a] += o.off[a];
            self.refills[a] += o.refills[a];
            self.inv_opens[a] += o.inv_opens[a];
            self.inv_ticks[a] += o.inv_ticks[a];
            self.inv_hit[a] += o.inv_hit[a];
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
            self.deaths[0],
            self.deaths[1],
            self.potions[0],
            self.potions[1],
            self.splashes[0],
            self.splashes[1],
            self.heal_self[0],
            self.heal_self[1],
            self.heal_gift[0],
            self.heal_gift[1],
            self.pot_offer[0],
            self.pot_offer[1],
            self.pot_perfect[0],
            self.pot_perfect[1],
            self.pot_double[0],
            self.pot_double[1],
            self.pot_health[0],
            self.pot_health[1],
            self.pot_away[0],
            self.pot_away[1],
            self.pot_hit[0],
            self.pot_hit[1],
            self.drinks[0],
            self.drinks[1],
            self.meals[0],
            self.meals[1],
            self.sips[0],
            self.sips[1],
            self.sip_ticks[0],
            self.sip_ticks[1],
            self.sip_hit[0],
            self.sip_hit[1],
            self.speed_on[0],
            self.speed_on[1],
            self.fed[0],
            self.fed[1],
            self.pot_after[0],
            self.pot_after[1],
            self.off[0],
            self.off[1],
            self.refills[0],
            self.refills[1],
            self.inv_opens[0],
            self.inv_opens[1],
            self.inv_ticks[0],
            self.inv_ticks[1],
            self.inv_hit[0],
            self.inv_hit[1],
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
    pub bot_state: BotState,
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
    /// Where each player is in the key presses of a potion throw, as
    /// `pearl_stage`, and what it asked for on its last tick.
    potion_stage: [u8; 2],
    asked_potion: [bool; 2],
    /// Ticks since each player's last potion throw, whether its crosshair
    /// has been back on the other player since, and for how many ticks in
    /// a row it has been off it.
    pot_since: [i32; 2],
    pot_back: [bool; 2],
    off_run: [i32; 2],
    /// The drink (1) or the meal (2) each player has its right button down
    /// for, and what it asked of the rest of the hotbar on its last tick.
    consume: [u8; 2],
    asked_bar: [u8; 2],
    /// Whether each client was drinking or eating on its last tick, and
    /// for how many ticks in a row it has seen the other player do so.
    was_consuming: [bool; 2],
    opp_use_ticks: [i32; 2],
    /// Where each player is with its inventory: 0 the screen is shut, 1
    /// it is open and the click that gets the potion it was opened for is
    /// still to come, 2 it is open and that click is made. And the kind of
    /// potion it was opened for, and whether it was open on the last tick.
    refill: [u8; 2],
    refill_kind: [Item; 2],
    was_open: [bool; 2],
    /// The lesson this duel started as (0: none).
    pub lesson: u8,
    /// A player has died: the duel ends when both have made their tick.
    over: bool,
}

pub fn new_match(p: &Params, rng: &mut Rng, mode: Mode) -> (Match, [f64; 2], BotParams) {
    let (m, ping, bot, _) = new_duel(p, rng, mode);
    (m, ping, bot)
}

/// A duel with newly drawn conditions, and the lesson it starts as (0:
/// none; see `Params::lessons`).
pub fn new_duel(p: &Params, rng: &mut Rng, mode: Mode) -> (Match, [f64; 2], BotParams, u8) {
    let world = World::arena(p.floor_y, p.arena_half, p.walls);
    let lim = (p.arena_half - 2.0).max(1.0);
    // Drawn only when lessons are on, so that duels without them come out
    // as they always did.
    let lesson = if p.lesson_prob > 0.0 && p.lessons & 15 != 0 && rng.chance(p.lesson_prob) {
        let on: Vec<u8> = (0..4u8).filter(|k| p.lessons >> k & 1 != 0).collect();
        1 + on[(rng.next_u64() % on.len() as u64) as usize]
    } else {
        0
    };
    let (d_lo, d_hi) = if lesson != 0 { (p.lesson_dist[0], p.lesson_dist[1]) } else { (p.start_dist_min, p.start_dist_max) };
    let mut a;
    let mut b;
    let mut tries = 0;
    loop {
        a = [rng.range(-lim, lim), p.floor_y, rng.range(-lim, lim)];
        let d = rng.range(d_lo, d_hi.max(d_lo));
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
    // Real health: a diamond set with Protection, a sword with Sharpness.
    let real = !p.infinite_health || (p.health_prob > 0.0 && rng.chance(p.health_prob));
    let real = real || lesson != 0;
    let level = |r: [f64; 2], rng: &mut Rng| -> i32 {
        let (lo, hi) = (r[0].round() as i64, (r[1].round() as i64).max(r[0].round() as i64));
        (lo + (rng.next_u64() % (hi - lo + 1) as u64) as i64) as i32
    };
    let (prot, sharp) = if real { (level(p.prot, rng), level(p.sharp, rng)) } else { (0, 0) };
    let scfg = ServerCfg {
        damage: p.damage,
        ench_damage: sharp as f32 * 1.25,
        kb_ench: p.kb_ench,
        infinite_health: !real,
        armor: if real { 20 } else { 0 },
        prot: 4 * prot_points(prot),
        feather: if real { feather_points(p.feather.round() as i32) } else { 0 },
        hunger: p.hunger,
        fall_damage: p.fall_damage,
        max_no_damage,
        sword_wear: if p.sword_wear >= 0.0 { p.sword_wear as f32 } else { [1.0f32, 0.25, 0.25, 0.0][(rng.next_u64() % 4) as usize] },
        pearl_cooldown: (rng.range(p.pearl_cd_s[0], p.pearl_cd_s[1].max(p.pearl_cd_s[0])) * 20.0).round().max(0.0) as i32,
        ..ServerCfg::default()
    };
    let pearls = if rng.chance(p.pearl_prob) { p.pearls.clamp(0.0, 64.0) as u8 } else { 0 };
    // The later draws are made only for what is switched on, so that duels
    // without it come out as they always did.
    let timed = (real && p.speed_end_prob > 0.0 && rng.chance(p.speed_end_prob)) || lesson == 1 || lesson == 4;
    let hungry = (real && p.hunger_prob > 0.0 && rng.chance(p.hunger_prob)) || lesson == 2;
    let stocked = (real && p.refill_prob > 0.0 && rng.chance(p.refill_prob)) || lesson >= 3;
    let mut scfg = scfg;
    scfg.hunger |= hungry;
    let seed = rng.next_u64();
    let mut m = Match::new(world, spawns, p.speed_amp, kb, scfg, ccfg, net, [0, c1, sv], seed);
    m.s.plugin = plugin;
    m.s.pearl_guard = true;
    for i in 0..2 {
        let pearls = if p.pearls_who == 0 || p.pearls_who as usize == i + 1 { pearls } else { 0 };
        m.s.p[i].pearls = pearls;
        m.c[i].lp.pearls = pearls;
        if real {
            let mut n = level(p.potions, rng).clamp(0, 7) as u8;
            // The first player is the one a lesson is for.
            let lesson = if i == 0 { lesson } else { 0 };
            let timed_i = timed && (p.speed_who == 0 || p.speed_who as usize == i + 1 || lesson == 1 || lesson == 4);
            if timed_i {
                // Speed as after a potion drunk some time ago, and potions
                // of it in the slots after the healing ones.
                let mut left = (rng.range(p.speed_left_s[0], p.speed_left_s[1].max(p.speed_left_s[0])) * 20.0).round() as i32;
                let mut cups = level(p.drinks, rng).clamp(0, 2) as u8;
                if lesson == 1 {
                    left = 0;
                    cups = cups.max(1);
                } else if lesson == 4 {
                    left = 0;
                    cups = 0;
                }
                n = n.min(7 - cups - hungry as u8);
                let (sp, lp) = (&mut m.s.p[i], &mut m.c[i].lp);
                sp.drinks = potion_mask(n + cups) & !potion_mask(n);
                lp.drinks = sp.drinks;
                if left > 0 && p.speed_amp >= 0 {
                    sp.speed_ticks = left;
                    sp.speed_shown = DRINK_SHOWN;
                    sp.fx = DRINK_SHOWN;
                    lp.speed_ticks = left;
                    lp.fx = DRINK_SHOWN;
                    m.c[1 - i].rp.fx = DRINK_SHOWN;
                } else {
                    sp.speed_ticks = 0;
                    sp.l.attr.speed_amp = -1;
                    lp.speed_ticks = -1;
                    lp.speed_amp = -1;
                    lp.l.attr.speed_amp = -1;
                }
            }
            if hungry {
                n = n.min(6);
                let mut food = level(p.start_food, rng).clamp(0, 20);
                if lesson == 2 {
                    food = (rng.next_u64() % 7) as i32;
                }
                let sat = if food >= 20 { 5.0f32 } else { 0.0f32 };
                let (sp, lp) = (&mut m.s.p[i], &mut m.c[i].lp);
                sp.steak = 64;
                lp.steak = 64;
                sp.food = Food { level: food, saturation: sat, exhaustion: 0.0, timer: 0 };
                sp.last_food = food;
                sp.was_hungry = sat == 0.0;
                lp.food = food;
                lp.saturation = sat;
            }
            if stocked && (p.refill_who == 0 || p.refill_who as usize == i + 1 || lesson >= 3) {
                // More potions in the rows of the inventory, the healing
                // ones first.
                let mut pots = level(p.inv_potions, rng).clamp(0, 27) as usize;
                let mut cups = if timed_i { level(p.inv_drinks, rng).clamp(0, 2) as usize } else { 0 };
                if lesson == 3 {
                    pots = pots.max(3);
                    n = 0;
                } else if lesson == 4 {
                    cups = cups.max(1);
                }
                let mut rows = Rows::default();
                for k in 0..(pots + cups).min(27) {
                    rows.0[k] = if k < pots { 1 } else { 2 };
                }
                m.s.p[i].rows = rows;
                m.c[i].lp.rows = rows;
            }
            m.s.p[i].potions = potion_mask(n);
            m.c[i].lp.potions = potion_mask(n);
            if lesson == 3 {
                let h = (rng.range(6.0, 12.0) * 2.0).round() as f32 / 2.0;
                m.s.p[i].health = h;
                m.s.p[i].last_health = h;
                m.c[i].lp.health = h;
                m.c[1 - i].rp.health = h;
            } else if (p.hurt_who == 0 || p.hurt_who as usize == i + 1) && rng.chance(p.start_hurt) {
                // Both sides start out knowing it.
                let h = (rng.range(p.start_health[0], p.start_health[1].max(p.start_health[0])) * 2.0).round() as f32 / 2.0;
                let h = h.clamp(1.0, MAX_HEALTH);
                m.s.p[i].health = h;
                m.s.p[i].last_health = h;
                m.c[i].lp.health = h;
                m.c[1 - i].rp.health = h;
            }
        }
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
    bot.potion *= p.bot_potion;
    bot.drink *= p.bot_drink;
    bot.refill *= p.bot_refill;
    (m, ping, bot, lesson)
}

impl Env {
    pub fn new(mode: Mode, params: Params, seed: u64) -> Env {
        let mut rng = Rng::new(seed);
        let (m, ping, bot, lesson) = new_duel(&params, &mut rng, mode);
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
            potion_stage: [0; 2],
            asked_potion: [false; 2],
            pot_since: [1000; 2],
            consume: [0; 2],
            asked_bar: [0; 2],
            was_consuming: [false; 2],
            opp_use_ticks: [0; 2],
            refill: [0; 2],
            refill_kind: [Item::Potion; 2],
            was_open: [false; 2],
            lesson,
            pot_back: [true; 2],
            off_run: [0; 2],
            over: false,
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
        self.potion_stage = [0; 2];
        self.asked_potion = [false; 2];
        self.pot_since = [1000; 2];
        self.consume = [0; 2];
        self.asked_bar = [0; 2];
        self.was_consuming = [false; 2];
        self.opp_use_ticks = [0; 2];
        self.refill = [0; 2];
        self.was_open = [false; 2];
        self.pot_back = [true; 2];
        self.off_run = [0; 2];
        self.over = false;
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
        let (m, ping, bot, lesson) = new_duel(&self.params, &mut self.rng, self.mode);
        self.m = m;
        self.ping_ms = ping;
        self.bot = bot;
        self.lesson = lesson;
        self.done_flag = [0; 2];
        self.begin();
    }

    /// Pearls that landed: the thrower pays for the damage it took the way
    /// it pays for a hit taken, and the next hit is watched.
    fn collect_landings(&mut self) {
        let real = !self.m.s.cfg.infinite_health;
        let hh = self.m.s.cfg.hit_health().max(0.1);
        for k in 0..self.m.s.landings.len() {
            let l = self.m.s.landings[k];
            let p = &self.params;
            // With real health it is what the landing took, counted in
            // plain hits.
            let cost = if real { l.dealt / hh } else { PEARL_DAMAGE / p.damage.max(0.5) };
            self.rew_acc[l.thrower] -= p.w_taken * p.w_damage * cost;
            self.ep.landings[l.thrower] += 1.0;
            self.landed = Some((l.thrower, 40));
        }
        self.m.s.landings.clear();
    }

    /// Potions that broke: health got back is worth what the same health
    /// lost to hits costs, to the player healed and against the other one,
    /// and the thrower pays for the part of its potion it got nothing from.
    fn collect_splashes(&mut self) {
        let hh = self.m.s.cfg.hit_health().max(0.1);
        for k in 0..self.m.s.splashes.len() {
            let s = self.m.s.splashes[k];
            let p = &self.params;
            for j in 0..2 {
                let v = p.w_heal * s.healed[j] / hh;
                self.rew_acc[j] += v;
                self.rew_acc[1 - j] -= v;
            }
            let t = s.thrower;
            self.rew_acc[t] -= p.w_waste * (POTION_HEAL - s.healed[t]).max(0.0) / hh;
            self.ep.splashes[t] += 1.0;
            self.ep.heal_self[t] += s.healed[t] as f64;
            self.ep.heal_gift[t] += s.healed[1 - t] as f64;
            self.ep.pot_offer[t] += s.offered[t] as f64;
            if s.offered[t] >= POTION_HEAL {
                self.ep.pot_perfect[t] += 1.0;
            }
        }
        self.m.s.splashes.clear();
    }

    fn collect_hits(&mut self) {
        self.collect_landings();
        self.collect_splashes();
        if !self.over {
            let dead = [!self.m.s.p[0].alive(), !self.m.s.p[1].alive()];
            if dead[0] || dead[1] {
                self.over = true;
                for a in 0..2 {
                    if dead[a] {
                        self.rew_acc[a] -= self.params.w_win;
                        self.ep.deaths[a] += 1.0;
                    } else {
                        self.rew_acc[a] += self.params.w_win;
                    }
                }
            }
        }
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
            // Relative to a plain hit with this sword.
            let plain = (self.params.damage + self.m.s.cfg.ench_damage).max(0.5);
            let worth = 1.0 + self.params.w_damage * (h.damage / plain - 1.0);
            self.rew_acc[h.attacker] += wh * worth;
            self.rew_acc[h.victim] -= wt * worth;
            self.ep.hits[h.attacker] += 1.0;
            if self.pot_since[h.victim] <= 20 {
                self.ep.pot_hit[h.victim] += 1.0;
            }
            if self.m.c[h.victim].lp.consuming() {
                self.ep.sip_hit[h.victim] += 1.0;
            }
            if self.m.c[h.victim].lp.inv_open {
                self.ep.inv_hit[h.victim] += 1.0;
            }
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
        // Speed and food as the server has them: the Speed left, counted up
        // to the last seconds in which a drink is worth it, and all of it
        // with food enough to sprint; and of what is missing, the part of
        // a drink or a meal under way that is done. So Speed goes little by
        // little as it runs out, a drink pays as it goes, giving it up
        // takes back what it paid, and with plenty of Speed left, or once
        // fed, there is nothing more in it.
        let sp = &self.m.s.p[id];
        let done = |item: Item| -> f32 {
            if sp.using_item && sp.use_slot == sp.held && sp.in_hand().0 == item {
                ((USE_TICKS - sp.use_count) as f32 / USE_TICKS as f32).clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        let have = (sp.speed_ticks.max(0) as f32 / self.drink_window().max(1) as f32).min(1.0);
        let fast = p.w_speed * (have + (1.0 - have) * done(Item::Drink));
        let fed = p.w_fed * if sp.food.level > 6 { 1.0 } else { done(Item::Food) };
        // The hotbar as the player's own client has it: out of healing
        // potions with some in the inventory, and out of Speed and of
        // potions of it with one in the inventory.
        let lp = &c.lp;
        let dry = (lp.potions == 0 && lp.rows.count(Item::Potion) > 0) as u8
            + (lp.drinks == 0 && lp.speed_amp < 0 && lp.rows.count(Item::Drink) > 0) as u8;
        let stock = -p.w_stock * dry as f32;
        (p.w_aim * aim as f32 + p.w_range * range as f32 + fast + fed + stock, ang)
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
        lp.pearls > 0
            && self.pearl_wait[id] <= 0
            && self.pearl_stage[id] == 0
            && self.potion_stage[id] == 0
            && self.consume[id] == 0
            && lp.held == SLOT_SWORD
    }

    /// Can player `id` throw a potion now? With the sword in hand, or on
    /// the tick after another potion.
    fn potion_ready(&self, id: usize) -> bool {
        let lp = &self.m.c[id].lp;
        lp.potions != 0
            && self.pearl_stage[id] == 0
            && self.consume[id] == 0
            && match self.potion_stage[id] {
                0 => lp.held == SLOT_SWORD,
                2 => true,
                _ => false,
            }
    }

    /// The last ticks of Speed in which a drink is taken, see
    /// `Params::drink_window_s`.
    fn drink_window(&self) -> i32 {
        (self.params.drink_window_s * 20.0).round() as i32
    }

    /// Is player `id`, as far as its own client knows, without Speed or in
    /// the last seconds of it?
    fn drink_due(&self, id: usize) -> bool {
        self.params.drink_window_s < 0.0 || self.m.c[id].lp.speed_ticks <= self.drink_window()
    }

    /// The key presses of a drink or a meal, laid over what player `id`
    /// asked for on this tick. `choice` is what it asked of the rest of the
    /// hotbar: 2 to drink (taken only when due, see `drink_due`), 3 to
    /// eat, 4 to stop. Only stopping stops: a
    /// healing potion asked for meanwhile is not thrown and does not end
    /// the drink (an agent that asks for one on a tick in fifty, as it does
    /// at any time, would otherwise never finish half of its drinks). To
    /// heal, stop first. Asked with the sword in hand, the
    /// number key of the potion of Speed or of the steak is pressed with a
    /// right click, and the button stays down on the ticks that follow, on
    /// which nothing else is asked, until the client is no longer using the
    /// item: the server has said that the bottle is empty or the steak
    /// eaten, or the stack was replaced. Then, or when the player stops,
    /// the button is let go and the number key of the sword pressed, so a
    /// meal is one steak. No left click is made meanwhile: the game throws
    /// them away while an item is in use.
    fn consume_keys(&mut self, id: usize, choice: i32, inp: &mut Input) {
        self.asked_bar[id] = choice.clamp(0, 6) as u8;
        let lp = &self.m.c[id].lp;
        if self.consume[id] == 0 {
            let slot = match choice {
                2 if lp.drinks != 0 && self.drink_due(id) => lp.drinks.trailing_zeros() as u8,
                3 if lp.steak > 0 && lp.food < 20 => SLOT_FOOD,
                _ => return,
            };
            if lp.held != SLOT_SWORD || self.pearl_stage[id] != 0 || self.potion_stage[id] != 0 {
                return;
            }
            inp.slot_key = slot + 1;
            inp.attack = false;
            inp.use_item = true;
            inp.use_again = self.prev_inp[id].use_item;
            self.consume[id] = (choice - 1) as u8;
            self.was_consuming[id] = false;
        } else if (self.was_consuming[id] && !lp.consuming()) || choice == 4 || lp.held == SLOT_SWORD {
            inp.slot_key = SLOT_SWORD + 1;
            inp.use_item = false;
            self.consume[id] = 0;
        } else {
            inp.use_item = true;
            inp.attack = false;
            // The click that did not start a use (a full stomach, as the
            // server knows before the client): give up.
            if !lp.consuming() && !self.was_consuming[id] && self.prev_inp[id].slot_key == 0 {
                inp.slot_key = SLOT_SWORD + 1;
                inp.use_item = false;
                self.consume[id] = 0;
            }
        }
    }

    /// One click of the mouse on a slot of the inventory. It comes out of
    /// the same budget as the clicks with the sword.
    fn take_click(&mut self, id: usize) -> bool {
        if self.cps[id] >= 20.0 {
            return true;
        }
        let t = self.clicks_available(id);
        let can = t >= 1.0 - 1e-9;
        self.click_tokens[id] = if can { t - 1.0 } else { t };
        can
    }

    /// The inventory, for player `id`: `choice` 5 asks for a healing potion
    /// from its rows and 6 for a potion of Speed. Asked with the screen
    /// shut, the sword in hand and nothing else under way, the inventory
    /// key is pressed, if there is such a potion and a way to make room
    /// for it. On the next tick the potion is shift-clicked, whatever is
    /// asked then (an empty bottle first, on a tick of its own, when the
    /// hotbar is full). From then on every tick that asks for a potion
    /// clicks another one down, and a tick that asks for anything else
    /// shuts the screen. A click after which nothing more could come down
    /// shuts it in the same tick. A click the click rate does not cover
    /// waits a tick. Returns whether the tick belongs to the screen: the
    /// game then takes no other key or button, and none is passed on.
    fn refill_keys(&mut self, id: usize, choice: i32, inp: &mut Input) -> bool {
        let want = match choice {
            5 => Some(Item::Potion),
            6 => Some(Item::Drink),
            _ => None,
        };
        let lp = &self.m.c[id].lp;
        let (bar, rows) = (lp.bar(), lp.rows);
        let inv;
        if self.refill[id] == 0 {
            let Some(kind) = want else { return false };
            if lp.held != SLOT_SWORD || self.pearl_stage[id] != 0 || self.potion_stage[id] != 0 || self.consume[id] != 0 {
                return false;
            }
            if refill_click(&bar, &rows, kind).is_none() {
                return false;
            }
            self.refill[id] = 1;
            self.refill_kind[id] = kind;
            self.click_tokens[id] = self.clicks_available(id);
            inv = 1;
        } else {
            let kind = if self.refill[id] == 1 { Some(self.refill_kind[id]) } else { want };
            match kind.and_then(|k| refill_click(&bar, &rows, k)) {
                None => {
                    self.refill[id] = 0;
                    self.click_tokens[id] = self.clicks_available(id);
                    inv = 0;
                }
                Some(slot) => {
                    if self.take_click(id) {
                        let (mut b, mut r) = (bar, rows);
                        shift_click(&mut b, &mut r, slot);
                        let more = refill_click(&b, &r, Item::Potion).is_some() || refill_click(&b, &r, Item::Drink).is_some();
                        if !more {
                            self.refill[id] = 0;
                            inv = slot + INV_SHUT;
                        } else {
                            // A bottle put away is not yet the potion.
                            if slot < WINDOW_BAR {
                                self.refill[id] = 2;
                            }
                            inv = slot;
                        }
                    } else {
                        inv = 1;
                    }
                }
            }
        }
        self.asked_bar[id] = choice.clamp(0, 6) as u8;
        self.asked_potion[id] = false;
        self.asked_pearl[id] = false;
        if self.pearl_wait[id] > 0 {
            self.pearl_wait[id] -= 1;
        }
        *inp = Input { inv, ..Input::default() };
        true
    }

    /// The key presses of a potion throw, as `pearl_keys`: the number key
    /// of the first slot that holds a potion and a right click, which
    /// throws along the view the server already has, then the number key of
    /// the sword. Asked for again on the tick after a throw, it takes the
    /// next potion in hand and clicks again at once, the button let go and
    /// pressed between the two ticks, and the sword comes back after the
    /// last one. It comes before the pearl when both are asked for.
    fn potion_keys(&mut self, id: usize, want: bool, inp: &mut Input) {
        self.asked_potion[id] = want;
        match self.potion_stage[id] {
            0 => {
                if want && self.potion_ready(id) {
                    inp.slot_key = self.m.c[id].lp.potions.trailing_zeros() as u8 + 1;
                    inp.attack = false;
                    if self.prev_inp[id].use_item {
                        inp.use_item = false;
                        self.potion_stage[id] = 1;
                    } else {
                        inp.use_item = true;
                        self.potion_stage[id] = 2;
                    }
                }
            }
            1 => {
                inp.use_item = true;
                inp.attack = false;
                self.potion_stage[id] = 2;
            }
            _ => {
                if want && self.potion_ready(id) {
                    inp.slot_key = self.m.c[id].lp.potions.trailing_zeros() as u8 + 1;
                    inp.attack = false;
                    inp.use_item = true;
                    inp.use_again = self.prev_inp[id].use_item;
                } else {
                    inp.slot_key = SLOT_SWORD + 1;
                    inp.use_item = false;
                    self.potion_stage[id] = 0;
                }
            }
        }
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
        let off = ang > 12.0;
        if out.threw_potion {
            self.ep.potions[id] += 1.0;
            if self.pot_since[id] <= 3 {
                self.ep.pot_double[id] += 1.0;
            }
            self.ep.pot_health[id] += self.m.c[id].lp.health as f64;
            // The ticks spent looking away to aim it, unless they were
            // already counted after the potion before.
            if self.pot_back[id] {
                self.ep.pot_away[id] += self.off_run[id].min(10) as f64;
            }
            self.pot_since[id] = 0;
            self.pot_back[id] = false;
        }
        if !self.pot_back[id] {
            if off && self.pot_since[id] < 40 {
                self.ep.pot_away[id] += 1.0;
                self.ep.pot_after[id] += 1.0;
            } else {
                self.pot_back[id] = true;
            }
        }
        self.off_run[id] = if off { self.off_run[id] + 1 } else { 0 };
        if off {
            self.ep.off[id] += 1.0;
        }
        if self.pot_since[id] < 1000 {
            self.pot_since[id] += 1;
        }
        // Drinking and eating.
        let lp = &self.m.c[id].lp;
        let rp = &self.m.c[id].rp;
        let consuming = lp.consuming();
        if consuming {
            self.ep.sip_ticks[id] += 1.0;
            if !self.was_consuming[id] {
                self.ep.sips[id] += 1.0;
            }
        }
        self.was_consuming[id] = consuming;
        let seen = rp.using && matches!(rp.item, Item::Drink | Item::Food);
        self.opp_use_ticks[id] = if seen { self.opp_use_ticks[id] + 1 } else { 0 };
        let sp = &mut self.m.s.p[id];
        self.ep.drinks[id] += sp.drinks_done as f64;
        self.ep.meals[id] += sp.meals_done as f64;
        sp.drinks_done = 0;
        sp.meals_done = 0;
        if sp.speed_ticks > 0 {
            self.ep.speed_on[id] += 1.0;
        }
        if sp.food.level > 6 {
            self.ep.fed[id] += 1.0;
        }
        // The inventory.
        let open = self.m.c[id].lp.inv_open;
        if open || self.was_open[id] {
            self.ep.inv_ticks[id] += 1.0;
        }
        if open && !self.was_open[id] {
            self.ep.inv_opens[id] += 1.0;
        }
        if out.moved.is_some() && (9..36).contains(&(inp.inv % INV_SHUT)) {
            self.ep.refills[id] += 1.0;
        }
        self.was_open[id] = open;
        if out.threw {
            self.ep.pearls[id] += 1.0;
            // The server starts its wait when the throw reaches it, a
            // little later than this; two ticks more cover the difference.
            self.pearl_wait[id] = self.m.s.cfg.pearl_cooldown + 2;
        }
        self.prev_inp[id] = *inp;
    }

    /// `how` is what the trainer is told: 1 the duel is over (a player
    /// died), 2 the clock cut it short.
    fn finish_episode(&mut self, how: u8) {
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
            self.done_flag[a] = how;
        }
        self.ep.episodes = 1.0;
        let ep = self.ep;
        self.total.add(&ep);
        let (m, ping, bot, lesson) = new_duel(&self.params, &mut self.rng, self.mode);
        self.m = m;
        self.ping_ms = ping;
        self.bot = bot;
        self.lesson = lesson;
        self.begin();
    }

    /// Apply the pending agent's action and run until an agent has to act
    /// again. Returns that agent's index.
    pub fn step(&mut self, action: &[i32]) -> usize {
        let id = self.pending;
        let mut inp = decode_action(action);
        let bar = if action.len() > 9 { action[9] } else { 0 };
        if !self.refill_keys(id, bar, &mut inp) {
            self.consume_keys(id, bar, &mut inp);
            self.potion_keys(id, bar == 1, &mut inp);
            self.pearl_keys(id, action.len() > 8 && action[8] != 0, &mut inp);
            self.limit_clicks(id, &mut inp);
        }
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
            if self.over {
                self.finish_episode(1);
            } else if self.ticks_left <= 0 {
                self.finish_episode(2);
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
            let e = self.m.s.thrown.iter().rev().find(|e| e.thrower == id && e.kind == Kind::Pearl);
            o[b + 26] = e.is_some() as u8 as f32;
            for a in 0..3 {
                o[b + 27 + a] = e.map(|e| e.pos[a] as f32).unwrap_or(0.0);
            }
            // Health and potions as the server has them, and this player's
            // newest potion in flight.
            o[b + 30] = self.m.s.p[id].health;
            o[b + 31] = self.m.s.p[id].potions.count_ones() as f32;
            let e = self.m.s.thrown.iter().rev().find(|e| e.thrower == id && e.kind == Kind::Potion);
            o[b + 32] = e.is_some() as u8 as f32;
            for a in 0..3 {
                o[b + 33 + a] = e.map(|e| e.pos[a] as f32).unwrap_or(0.0);
            }
            // As the server has them: what is in the hand (0 sword, 1 pearl,
            // 2 healing potion, 3 potion of Speed, 4 bottle, 5 steak, 6
            // nothing), whether it is being drunk (1) or eaten (2), the
            // seconds of Speed left (-1 without it), food, potions of Speed
            // left, and whether the effect's particles show.
            let sp = &self.m.s.p[id];
            let item = sp.in_hand().0;
            o[b + 36] = match item {
                Item::Sword => 0.0,
                Item::Pearl => 1.0,
                Item::Potion => 2.0,
                Item::Drink => 3.0,
                Item::Bottle => 4.0,
                Item::Food => 5.0,
                Item::Empty => 6.0,
            };
            o[b + 37] = match (sp.using_item && sp.use_slot == sp.held, item) {
                (true, Item::Drink) => 1.0,
                (true, Item::Food) => 2.0,
                _ => 0.0,
            };
            o[b + 38] = if sp.l.attr.speed_amp >= 0 { sp.speed_ticks as f32 / 20.0 } else { -1.0 };
            o[b + 39] = sp.food.level as f32;
            o[b + 40] = sp.drinks.count_ones() as f32;
            o[b + 41] = sp.fx as i32 as f32;
            // The inventory: whether this player's client has its screen
            // open, and the healing potions and the potions of Speed in
            // its rows as the server has them.
            o[b + 42] = lp.inv_open as i32 as f32;
            o[b + 43] = sp.rows.count(Item::Potion) as f32;
            o[b + 44] = sp.rows.count(Item::Drink) as f32;
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
        // Real health or not, and the health a plain hit costs.
        o[g + 22] = !s.cfg.infinite_health as u8 as f32;
        o[g + 23] = s.cfg.hit_health();
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
        for v in o[77..OBS_DIM].iter_mut() {
            *v = 0.0;
        }
        if let Some(e) = c.thrown.iter().rev().find(|e| e.mine && e.kind == Kind::Pearl) {
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
        if let Some(e) = c.thrown.iter().rev().find(|e| !e.mine && e.kind == Kind::Pearl) {
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
        // Health: whether hits cost any in this duel, this player's and the
        // opponent's as this client knows them, and the health a plain hit
        // costs with this armour and these swords.
        let real = !self.m.s.cfg.infinite_health;
        let hh = self.m.s.cfg.hit_health();
        o[96] = real as u8 as f32;
        o[97] = lp.health / MAX_HEALTH;
        o[98] = rp.health / MAX_HEALTH;
        o[99] = (lp.health - rp.health) / MAX_HEALTH;
        o[100] = if real { hh / 4.0 } else { 0.0 };
        o[101] = ((MAX_HEALTH - lp.health) / POTION_HEAL).min(2.5);
        // Potions: how many are left, whether one can be thrown now, where
        // the throw's key presses are, what was asked last tick, whether
        // the opponent has one in hand and how many of its potions this
        // client has seen.
        o[102] = lp.potions.count_ones() as f32 / 7.0;
        o[103] = self.potion_ready(id) as u8 as f32;
        o[104] = self.potion_stage[id] as f32 / 2.0;
        o[105] = self.asked_potion[id] as u8 as f32;
        o[106] = (rp.item == Item::Potion) as u8 as f32;
        o[107] = (lp.potions_seen.min(14)) as f32 / 7.0;
        // Where each player goes if it keeps moving as it does now.
        let feet = [b.pos, rp.pos];
        let seen = if vel[0].hypot(vel[2]) < 1.5 { [vel[0], vel[2]] } else { [0.0; 2] };
        let vel = [pace(b.motion, b.on_ground), seen];
        let world = &self.m.world;
        // Its own newest potion in flight: where it will break (relative to
        // where this player stands now), in how many ticks, and the health
        // it will then give this player and the opponent.
        if let Some(e) = c.thrown.iter().rev().find(|e| e.mine && e.kind == Kind::Potion) {
            let (at, ticks, heal) = potion_outcome(world, e.pos, e.motion, e.age + 2, 0, feet, vel);
            let (lf, lr) = local(yaw, at[0] - b.pos[0], at[2] - b.pos[2]);
            o[108] = 1.0;
            o[109] = clip(lf / 4.0, 4.0);
            o[110] = clip(lr / 4.0, 4.0);
            o[111] = clip((at[1] - b.pos[1]) / 3.0, 2.0);
            o[112] = (ticks.min(40)) as f32 / 20.0;
            o[113] = heal[0] / POTION_HEAL;
            o[114] = heal[1] / POTION_HEAL;
        }
        // The opponent's newest potion in flight, the same way.
        if let Some(e) = c.thrown.iter().rev().find(|e| !e.mine && e.kind == Kind::Potion) {
            let (at, ticks, heal) = potion_outcome(world, e.pos, e.motion, e.age + 2, 1, feet, vel);
            let (lf, lr) = local(yaw, at[0] - b.pos[0], at[2] - b.pos[2]);
            o[115] = 1.0;
            o[116] = clip(lf / 4.0, 4.0);
            o[117] = clip(lr / 4.0, 4.0);
            o[118] = clip((at[1] - b.pos[1]) / 3.0, 2.0);
            o[119] = (ticks.min(40)) as f32 / 20.0;
            o[120] = heal[0] / POTION_HEAL;
            o[121] = heal[1] / POTION_HEAL;
        }
        // A potion thrown on this tick flies along the view the server has:
        // how far from here it would break, in how many ticks, and the
        // health it would give this player if it keeps moving as it does,
        // if it stops, and the opponent.
        if lp.potions != 0 {
            let (from, dir) = throw_heading(Kind::Potion, b.pos, lp.l.yaw, lp.l.pitch);
            let v = Kind::Potion.speed() as f64;
            let m = [dir[0] * v, dir[1] * v, dir[2] * v];
            let (at, ticks, heal) = potion_outcome(world, from, m, 0, 0, feet, vel);
            let still = potion_outcome(world, from, m, 0, 0, feet, [[0.0; 2], vel[1]]).2;
            let (lf, lr) = local(yaw, at[0] - b.pos[0], at[2] - b.pos[2]);
            o[122] = clip(lf / 4.0, 4.0);
            o[123] = clip(lr / 4.0, 4.0);
            o[124] = (ticks.min(40)) as f32 / 20.0;
            o[125] = heal[0] / POTION_HEAL;
            o[126] = still[0] / POTION_HEAL;
            o[127] = heal[1] / POTION_HEAL;
        }
        // Speed that runs out, drinks and food, as this client knows them:
        // whether it has Speed and how much of a potion's time is left,
        // the potions of it and the steak it carries, its food and
        // saturation and whether that is enough to sprint; whether it is
        // drinking or eating, how far along, and which of the two it has
        // the button down for; what it asked of the hotbar last tick; and
        // of the opponent, whether particles of an effect show around it,
        // whether it is drinking or eating, for how long it has been, and
        // whether it holds an empty bottle.
        let speed = lp.speed_ticks > 0;
        o[128] = speed as u8 as f32;
        o[129] = if speed { lp.speed_ticks.min(1800) as f32 / 1800.0 } else { 0.0 };
        o[130] = lp.drinks.count_ones() as f32 / 2.0;
        o[131] = (lp.steak > 0) as u8 as f32;
        o[132] = lp.food as f32 / 20.0;
        o[133] = lp.saturation / 20.0;
        o[134] = (lp.food > 6) as u8 as f32;
        let using = if lp.consuming() { lp.bar().stack(lp.use_slot).0 } else { Item::Empty };
        o[135] = (using == Item::Drink) as u8 as f32;
        o[136] = (using == Item::Food) as u8 as f32;
        o[137] = if lp.consuming() { ((USE_TICKS - lp.use_count) as f32 / USE_TICKS as f32).clamp(0.0, 1.5) } else { 0.0 };
        o[138] = self.consume[id] as f32 / 2.0;
        o[139] = self.asked_bar[id] as f32 / 4.0;
        o[140] = rp.fx as u8 as f32;
        o[141] = (rp.using && rp.item == Item::Drink) as u8 as f32;
        o[142] = (rp.using && rp.item == Item::Food) as u8 as f32;
        o[143] = self.opp_use_ticks[id].min(48) as f32 / USE_TICKS as f32;
        // The inventory: whether its screen is open, the healing potions
        // and the potions of Speed in its rows, the free slots and the
        // empty bottles in the hotbar, and whether the screen was opened
        // for a potion that has not come down yet.
        let bar = lp.bar();
        o[144] = lp.inv_open as u8 as f32;
        o[145] = (lp.rows.count(Item::Potion) as f32 / 8.0).min(3.0);
        o[146] = (lp.rows.count(Item::Drink) as f32 / 2.0).min(3.0);
        o[147] = (1..9u8).filter(|&k| bar.stack(k).0 == Item::Empty).count() as f32 / 8.0;
        o[148] = lp.bottles.count_ones() as f32 / 2.0;
        o[149] = (self.refill[id] == 1) as u8 as f32;
        // The Speed it has left out of the last seconds in which a drink
        // is taken: 1 with more than that, when a drink would not be.
        o[150] = if self.params.drink_window_s < 0.0 {
            speed as u8 as f32
        } else {
            (lp.speed_ticks.max(0) as f32 / self.drink_window().max(1) as f32).min(1.0)
        };
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
        o[52] = s.thrown.iter().any(|e| e.thrower != id && e.kind == Kind::Pearl) as u8 as f32;
        o[53] = s.cfg.pearl_cooldown as f32 / 320.0;
        // Health and potions as the server has them.
        o[54] = s.p[id].health / MAX_HEALTH;
        o[55] = s.p[1 - id].health / MAX_HEALTH;
        o[56] = s.p[id].potions.count_ones() as f32 / 7.0;
        o[57] = s.p[1 - id].potions.count_ones() as f32 / 7.0;
        o[58] = s.thrown.iter().any(|e| e.thrower != id && e.kind == Kind::Potion) as u8 as f32;
        o[59] = if s.cfg.infinite_health { 0.0 } else { s.cfg.hit_health() / 4.0 };
        // Speed, food and drinks as the server has them.
        for (k, who) in [id, 1 - id].into_iter().enumerate() {
            let q = &s.p[who];
            o[60 + 4 * k] = q.speed_ticks.min(1800) as f32 / 1800.0;
            o[61 + 4 * k] = q.food.level as f32 / 20.0;
            o[62 + 4 * k] = q.drinks.count_ones() as f32 / 2.0;
            o[63 + 4 * k] = if q.using_item && q.use_slot != SLOT_SWORD { q.use_count.clamp(0, USE_TICKS) as f32 / USE_TICKS as f32 } else { 0.0 };
        }
        // The opponent's inventory: whether it has the screen open, and
        // the potions in its rows.
        o[68] = other.inv_open as u8 as f32;
        o[69] = (s.p[1 - id].rows.count(Item::Potion) as f32 / 8.0).min(3.0);
        o[70] = (s.p[1 - id].rows.count(Item::Drink) as f32 / 2.0).min(3.0);
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
