//! What plays a player from outside the game: the part between a policy and
//! a client. It turns what the policy chose into the keys and buttons of
//! one client tick, remembers from tick to tick the little it needs for
//! that, and writes down what the policy is shown. All of it works from one
//! client (its own player, what the server sent it of the other, the
//! blocks) and a few facts about the duel, so the same can be done around a
//! real game client. What only a simulation knows, the rewards, the
//! statistics and what the critic is shown, stays in the environment.
use crate::aabb::{dist, dist_sq, Aabb, V3};
use crate::bot;
use crate::fullheal;
use crate::client::{pearl_landing, pearl_preview, thrown_flight, Client, ClientCfg, ClientThrown, Input, TickOut, INV_SHUT};
use crate::mth::{self, Rng};
use crate::net::{refill_click, shift_click, Item, Kind, ROD_MAX_WEAR, SLOT_FOOD, SLOT_PEARL, SLOT_SWORD, USE_TICKS, WINDOW_BAR};
use crate::server::{splash_heal, throw_heading, MAX_HEALTH, POTION_HEAL, THROW_SPREAD};
use crate::world::{RayHit, World};

pub const ACT_HEADS: usize = 13;
/// forward, strafe, jump, sprint key, attack, yaw bin, pitch bin, right
/// mouse button (block with the sword), throw a pearl, and what to do with
/// the rest of the hotbar: nothing, throw a healing potion, drink the
/// potion of Speed, eat, stop drinking or eating, or get a healing potion
/// or a potion of Speed down from the inventory.
///
/// A pearl and a potion throw each stand for three key presses a player
/// makes in a row: the number key of the pearls or of a potion with a right
/// click, then the number key of the sword on the next tick (see
/// `Agent::pearl_keys` and `Agent::potion_keys`). The pearl does nothing while
/// the server's cooldown runs or when no pearl is left, the potion when no
/// potion is left. A drink or a meal is asked for once: the number key and
/// the right button, which then stays down until the server says it is
/// done or the player stops (see `Agent::consume_keys`). A potion from the
/// inventory is the inventory key, a shift click on the potion on the next
/// tick, and the key again on the third; asked for on every tick, another
/// potion comes down on each (see `Agent::refill_keys`).
///
/// The last control is a flick of the mouse, added to the yaw of the same
/// tick: nothing, an eighth of a turn or a quarter either way, or half a
/// turn (see `FLICK_BINS`). The yaw control alone had settled on small
/// turns so firmly that its large ones were never tried again; a control
/// of its own starts the same in every state and is learnt from there.
///
/// The control of the rest of the hotbar has three more choices: a healing
/// potion thrown with a turn, behind the player or to its right or left
/// (`BAR_TURNS`), what a player does who turns away to pot. Tick one: the
/// turn of the view, added to what the policy asked of the yaw, with the
/// number key of the potion, and the view's pitch set to the one the last
/// control asks for (`throw_pitch`: straight down to 20 degrees under
/// level, two degrees apart). Tick two: the right click, which throws
/// along the view the server has by then, and nothing else of the hotbar.
/// The keys, the mouse from tick two on, which pitch to throw at, where
/// to go while the potion flies, when to turn back, and which way to turn
/// or not to, are the policy's to learn.
///
/// Three choices more throw two potions with one turn: the same turns, and
/// on tick three the next potion's number key and a right click with the
/// view kept where the first one went, as a player pots twice in a row.
///
/// The control after the pitch is the tool of a kit that has one, a
/// fishing rod or snowballs or eggs in a slot of their own: nothing, use
/// the tool, or take the sword back (see `Agent::tool_keys`). With no tool
/// it does nothing.
pub const ACT_DIMS: [i32; ACT_HEADS] = [3, 3, 2, 2, 2, 31, 29, 2, 2, 13, 6, THROW_PITCHES as i32, 3];
/// The first of the choices of the hotbar control that throw a potion with
/// a turn, and the mouse counts of each one's turn: half a turn, a quarter
/// to the right, a quarter to the left. The choices after those make the
/// same turns for two potions.
pub const BAR_TURN: i32 = 7;
pub const BAR_TURNS: [i32; 3] = [1200, 600, -600];
/// The degrees of one mouse count.
pub const COUNT_DEG: f32 = 0.15;
/// The pitches a potion thrown with a turn can be thrown at: choice k of
/// the last control is `90 - 2 k` degrees.
pub const THROW_PITCHES: usize = 36;

/// The pitch of choice `k` of the last control.
pub fn throw_pitch(k: i32) -> f32 {
    90.0f32 - 2.0f32 * (k.clamp(0, THROW_PITCHES as i32 - 1) as f32)
}
/// Ticks after the turn of a potion thrown with a turn that observation
/// 152 counts down.
const TURN_SHOWN: i32 = 8;
/// Mouse counts a flick adds to the yaw of its tick.
pub const FLICK_BINS: [i32; 6] = [0, -300, 300, -600, 600, 1200];
/// The choices of the yaw and the pitch controls that leave the mouse still.
const YAW_STILL: i32 = 13;
const PITCH_STILL: i32 = 10;
/// Mouse counts per tick. One count is 0.15 degrees at sensitivity 0.5.
/// The four large ones at the end came later, for turning away from the
/// opponent and back within a tick or two: 600 counts are a quarter turn
/// and 1200 a half.
pub const YAW_BINS: [i32; 31] = [
    -377, -233, -144, -89, -55, -34, -21, -13, -8, -5, -3, -2, -1, 0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, -600, 600,
    -1200, 1200,
];
/// The eight large ones at the end came later, for looking at the ground
/// and back up within one tick: 600 counts are a quarter turn at the
/// sensitivity the agent plays with.
pub const PITCH_BINS: [i32; 29] = [
    -89, -55, -34, -21, -13, -8, -5, -3, -2, -1, 0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, -377, -233, -144, 144, 233, 377, -600, 600,
];

pub const OBS_DIM: usize = TOOL_AT + TOOL_OBS;
/// Where the tool of the kit, the hooks and the snowballs and eggs start
/// in an observation (after the cobwebs), and how many numbers they take.
pub const TOOL_AT: usize = WEB_AT + 2 + GRID_FWD.len() * GRID_RIGHT.len();
pub const TOOL_OBS: usize = 25;
/// Where what the last hit and the last second took of its health is in an
/// observation, and how many of its own ticks that second is.
pub const HURT_AT: usize = PEARL_AT + 3 * PEARL_TURNS.len();
pub const HURT_WINDOW: usize = 20;
/// Where the cobwebs start in an observation: whether its own box is in
/// one, whether the opponent's is (where its client has it), then the grid
/// of `web_grid`.
pub const WEB_AT: usize = HURT_AT + 2;

/// Where the previews of a pearl thrown with the view turned first start in
/// an observation, and the turns (degrees of yaw).
pub const PEARL_AT: usize = GRID_AT + GRID_FWD.len() * GRID_RIGHT.len();
pub const PEARL_TURNS: [f32; 4] = [45.0, -45.0, 90.0, -90.0];

/// The ground around a player, shown as a grid of columns along its view:
/// blocks forward (negative: behind) and to the right.
pub const GRID_FWD: [f64; 7] = [-2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0];
pub const GRID_RIGHT: [f64; 7] = [-3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0];
/// Where the grid starts in an observation.
pub const GRID_AT: usize = 154;
/// What a column with no ground near enough under the feet shows.
pub const NO_GROUND: f64 = -6.0;

/// Where the ground is in the column of blocks at (x, z) for feet at
/// height `feet`: the top of the highest block from the level of the head
/// down to five under the feet, relative to the feet; `NO_GROUND` for none
/// (a deep pit, the void). 1 is a block to jump onto, 0.5 a slab to walk
/// onto, 2 a wall.
pub fn ground(world: &World, x: i32, z: i32, feet: f64) -> f64 {
    let fy = mth::floor_d(feet);
    for y in (fy - 5..=fy + 1).rev() {
        let h = world.block_height(x, y, z);
        if h > 0.0 {
            return (y as f64 + h - feet).min(2.0);
        }
    }
    NO_GROUND
}

/// The cobwebs in the column of blocks at (x, z) for feet at height
/// `feet`: a half for one at the level of the feet, a half for one at the
/// level of the head.
pub fn webs(world: &World, x: i32, z: i32, feet: f64) -> f64 {
    let fy = mth::floor_d(feet);
    (world.web(x, fy, z) as u8 as f64 + world.web(x, fy + 1, z) as u8 as f64) / 2.0
}

/// The grid of `webs` around a player at `pos` looking along `yaw`, laid
/// out as the grid of `ground`.
pub fn web_grid(world: &World, pos: V3, yaw: f32, o: &mut [f32]) {
    let (s, c) = (yaw as f64).to_radians().sin_cos();
    for (i, f) in GRID_FWD.iter().enumerate() {
        for (j, r) in GRID_RIGHT.iter().enumerate() {
            let x = pos[0] - s * f - c * r;
            let z = pos[2] + c * f - s * r;
            o[i * GRID_RIGHT.len() + j] = webs(world, mth::floor_d(x), mth::floor_d(z), pos[1]) as f32;
        }
    }
}

/// The grid of `ground` around a player at `pos` looking along `yaw`,
/// divided by 3, forward row by forward row.
pub fn ground_grid(world: &World, pos: V3, yaw: f32, o: &mut [f32]) {
    let (s, c) = (yaw as f64).to_radians().sin_cos();
    for (i, f) in GRID_FWD.iter().enumerate() {
        for (j, r) in GRID_RIGHT.iter().enumerate() {
            // Forward is (-sin, cos), right is (-cos, -sin).
            let x = pos[0] - s * f - c * r;
            let z = pos[2] + c * f - s * r;
            let g = ground(world, mth::floor_d(x), mth::floor_d(z), pos[1]);
            o[i * GRID_RIGHT.len() + j] = (g / 3.0) as f32;
        }
    }
}

/// How many of its last positions an agent keeps.
const TRAIL: usize = 16;
/// A hurt of the other player counts as a hit of this one's for this many
/// ticks after a hit of its own went out: the server's answer takes a ping.
const HIT_WINDOW: i32 = 20;

/// What an agent is told about the duel it plays, the same for both
/// players and for all of the duel.
#[derive(Clone, Copy, Debug)]
pub struct Duel {
    /// Half the side of the arena, the height of its floor, and where its
    /// centre is (x and z): the origin in every duel of the simulator.
    pub arena_half: f64,
    pub floor_y: f64,
    pub center: [f64; 2],
    /// Whether hits cost health, and the health a plain hit costs with the
    /// armour and the swords of this duel.
    pub real_health: bool,
    pub hit_health: f32,
    /// Server ticks a player waits between two pearls.
    pub pearl_cooldown: i32,
    /// The last seconds of Speed in which a drink is taken (negative: at
    /// any time).
    pub drink_window_s: f64,
}

impl Duel {
    /// `drink_window_s` in ticks.
    pub fn drink_window(&self) -> i32 {
        (self.drink_window_s * 20.0).round() as i32
    }
}

/// What one player's agent remembers from tick to tick.
#[derive(Clone, Debug)]
pub struct Agent {
    /// The ping of its own player and of the other, in milliseconds.
    pub ping_ms: [f64; 2],
    /// Its click rate in this duel and what is left of its budget.
    pub cps: f64,
    click_tokens: f64,
    /// The keys and buttons of its last tick.
    pub prev_inp: Input,
    /// Where it is in the key presses of a pearl throw: 0 not throwing, 1
    /// the right click comes next, 2 the sword comes next.
    pearl_stage: u8,
    /// Own ticks until it may throw again, as it counts them itself from
    /// its last throw.
    pearl_wait: i32,
    asked_pearl: bool,
    /// Where it is in the key presses of a potion throw, as `pearl_stage`,
    /// and what it asked for on its last tick.
    potion_stage: u8,
    asked_potion: bool,
    /// A potion thrown with a turn: ticks since the turn, the tick of the
    /// turn being 1, and 0 when there is none or it is `TURN_SHOWN` ago.
    turned: i32,
    /// Whether the potion thrown with the turn under way has a second one
    /// after it, on the tick after its click.
    double: bool,
    /// Own ticks since its last potion throw.
    since_throw: i32,
    /// Where it stood after each of its last ticks, newest first, and the
    /// ticks from a click of its own to the first it sees of what the
    /// server made of it, counted on its last potion and guessed from its
    /// ping before that. The server has a player where it stood that long
    /// ago, which is where a potion in flight will find it.
    trail: [V3; TRAIL],
    lag: i32,
    /// The opponent's potion and its own that it sees in flight and that
    /// break on their next tick: which one, where, and the health it gives
    /// this player and the other.
    fate: [Option<(u32, V3, [f32; 2])>; 2],
    /// The same for pearls: which one and where it lands.
    pearl_fate: [Option<(u32, V3)>; 2],
    /// The opponent's pearl it last saw appear, and its own ticks since:
    /// for that long out of the server's wait the opponent has no pearl to
    /// come over with.
    opp_pearl: Option<u32>,
    opp_pearl_age: i32,
    /// Its health on its last tick (below zero before the first), what the
    /// last fall of it took, and what each of its last `HURT_WINDOW` ticks
    /// took, oldest at `hurt_at`.
    hp_last: f32,
    hp_drop: f32,
    hp_lost: [f32; HURT_WINDOW],
    hurt_at: usize,
    /// The drink (1) or the meal (2) it has its right button down for, and
    /// what it asked of the rest of the hotbar on its last tick.
    consume: u8,
    asked_bar: u8,
    /// Whether its client was drinking or eating on its last tick, and for
    /// how many ticks in a row it has seen the other player do so.
    pub was_consuming: bool,
    opp_use_ticks: i32,
    /// Where it is with its inventory: 0 the screen is shut, 1 it is open
    /// and the click that gets the potion it was opened for is still to
    /// come, 2 it is open and that click is made. And the kind of potion it
    /// was opened for.
    refill: u8,
    refill_kind: Item,
    /// The hits in a row it has landed and taken, as its own client can
    /// tell: the other player hurt soon after a hit of its own, and itself
    /// hurt other than by a pearl of its own coming down.
    combo: [u32; 2],
    /// Where its player stood after its last tick and the one before: a
    /// throw on the tick to come goes out with the movement between the
    /// two, on a server that adds it (`ServerCfg::throw_carry`).
    said: V3,
    said_before: V3,
    /// What it has seen of that: -1 not yet, 0 its potions fly as the
    /// view throws them, 1 they take on its movement. Each potion of its
    /// own thrown while moving tells (`CARRY_MOVED`).
    carry: i8,
    /// The last potion click: how fast the view throws it across the
    /// ground, the movement it goes out with, and the ticks since.
    probe: Option<([f64; 2], [f64; 2], i32)>,
    /// Training only: a teacher for the potions thrown with a turn, see
    /// `Teacher`. Off, it does nothing and costs nothing.
    pub teacher: Teacher,
    /// The slot the tool was last seen in (`NO_SLOT` for none): snowballs
    /// or eggs used up in hand leave the slot empty, and the sword is taken
    /// back.
    tool_slot: u8,
    /// Plans for potions thrown with a turn that give all eight points
    /// (`fullheal`), when set; off by default (`Params::full_heal`).
    pub full_heal: Option<fullheal::FullHealCfg>,
    /// The plan of the potion thrown with a turn under way, its tick (0
    /// the turn's), and the pitch from before it.
    heal_plan: Option<fullheal::HealPlan>,
    plan_t: usize,
    plan_pitch: f32,
    plan_pitch0: f32,
    /// A last guard against dying with potions at hand (`safety_bar`),
    /// when set; off by default (`Params::pot_safety`). And the ticks on
    /// which it changed the policy's choice.
    pub safety: Option<SafetyCfg>,
    pub safety_acts: u32,
    /// A potion thrown with a turn lets go of the right button on the
    /// tick of the turn, so that the click on the next tick is a press of
    /// its own: with the button held through the turn (a block) the click
    /// throws nothing. Off by default (`Params::turn_release`).
    pub turn_release: bool,
    /// What it takes the opponent's health to be on a server that does not
    /// send it (`GuessCfg`), when set; off by default
    /// (`Params::opp_hp_guess`). The guess, the newest potion of each side
    /// already counted, the health shown on its first tick, the opponent's
    /// hurts seen since, whether the health shown ever changed, and whether
    /// the policy is shown the guess.
    pub guess: Option<GuessCfg>,
    /// What the policy is shown of the opponent's health in place of what
    /// the server sends (`OppShown`), when set; off by default. Before the
    /// guess.
    pub opp_shown: Option<OppShown>,
    guess_hp: f32,
    guess_ids: [u32; 2],
    guess_first: f32,
    guess_changed: bool,
    guess_shown: bool,
    hidden: bool,
}

const NO_SLOT: u8 = u8::MAX;

/// See `Agent::opp_shown`. A network trained with the opponent's health
/// hidden in every duel (`Params::opp_hp_hide_prob` 1) is shown the same
/// value (`Params::opp_hp_shown`) on every server: `health` with `auto`
/// off. With `auto` on, only once the server is seen to hide the health
/// (`Agent::hidden`). A `health` below zero shows this player's own.
#[derive(Clone, Copy, Debug)]
pub struct OppShown {
    pub health: f32,
    pub auto: bool,
}

/// The lowest the guess of the opponent's health goes (`Agent::guess`).
pub const GUESS_MIN: f32 = 0.5;

/// See `Agent::guess`. The guess starts full and takes `scale` times `hit`
/// (the cost of a plain hit in this duel, `Duel::hit_health`) off each
/// time the opponent is seen hurt by a hit of its own (`scale` above one for the critical hits
/// and the stronger hits inside the hurt time, which are not seen), `block`
/// times that when it holds its sword up;
/// each potion that breaks adds what this client foresees it gives the
/// opponent, and one health comes back every `regen` ticks (natural
/// regeneration on a full stomach). With `auto` the policy is shown the
/// guess only once the server is seen to hide the health
/// (`Agent::hidden`); without it, always.
#[derive(Clone, Copy, Debug)]
pub struct GuessCfg {
    pub hit: f32,
    pub scale: f32,
    pub block: f32,
    pub regen: i32,
    pub auto: bool,
}

impl Default for GuessCfg {
    fn default() -> Self {
        GuessCfg { hit: 1.0, scale: 1.1, block: 0.75, regen: 160, auto: false }
    }
}

/// See `Agent::safety_bar`.
#[derive(Clone, Copy, Debug)]
pub struct SafetyCfg {
    /// Ticks from the click of a potion to its heal on the server.
    pub flight: i32,
    /// Hits counted on top of those the opponent can still land before
    /// that heal, and health on top of what they cost.
    pub extra: f32,
    pub margin: f32,
    /// The worst a hit can cost, as a share of the plain hit's cost of the
    /// duel (a critical hit: one and a half).
    pub crit: f32,
    /// No rule with the opponent shown farther than this (blocks across
    /// the ground).
    pub dist: f64,
    /// The pitch choice (`throw_pitch`) of the potion it throws, or the
    /// policy's of the tick when below zero.
    pub pitch: i32,
    /// The turn of that potion: 0 none (the view keeps its yaw and takes
    /// the pitch, the click follows on the next tick as for a turned
    /// throw), 1 to 3 those of `BAR_TURNS`.
    pub turn: i32,
    /// Only on a server seen to hide the opponent's health (`Agent::hidden`).
    pub hidden: bool,
}

impl Default for SafetyCfg {
    fn default() -> Self {
        SafetyCfg { flight: 5, extra: 0.0, margin: 0.5, crit: 1.5, dist: 6.0, pitch: 0, turn: 0, hidden: false }
    }
}

/// Training only. The way a potion thrown with a turn used to be played
/// for the policy, worked out from the player's own client: the pitch
/// whose potion heals most on average over the server's spread
/// (`Agent::teacher_aim`), sprinting along the turned view for the click,
/// turning back on the tick after, and walking the way it ran until the
/// potion is down. It plays nothing: it only says, for the controls it has
/// an opinion on, what it would have chosen (`teach`, -1 for none), for
/// the policy to imitate while it starts to learn these throws, and to
/// leave behind when it can do better.
#[derive(Clone, Debug)]
pub struct Teacher {
    pub on: bool,
    pub teach: [i32; ACT_HEADS],
    /// Ticks since the turn (1 on its tick, 0 for none), the turn's mouse
    /// counts, the pitch from before it, the way it runs, and the last
    /// tick it says how to run.
    tick: i32,
    by: i32,
    pitch: f32,
    dir: [f64; 2],
    until: i32,
}

impl Default for Teacher {
    fn default() -> Teacher {
        Teacher { on: false, teach: [-1; ACT_HEADS], tick: 0, by: 0, pitch: 0.0, dir: [0.0; 2], until: 0 }
    }
}

/// How likely the gaussian of a throw is to come out at each of the
/// deviations `Agent::teacher_aim` tries, from two under to two over in
/// halves (`exp(-z * z / 2)`, written out to be the same everywhere).
const SPREAD_WEIGHTS: [f64; 9] = [
    0.1353352832366127, 0.32465246735834974, 0.6065306597126334, 0.8824969025845955, 1.0, 0.8824969025845955, 0.6065306597126334, 0.32465246735834974, 0.1353352832366127,
];
/// The ticks of its own run the teacher aims over, the pitches it tries
/// (90 down to 20, a degree apart), and the most ticks it runs for.
pub const RUN_TICKS: usize = 16;
const AIM_PITCHES: usize = 71;
const MOVE_TICKS: i32 = 14;

/// The least movement across the ground in a tick with which a potion of
/// its own tells whether the server adds it to the throw.
const CARRY_MOVED: f64 = 0.08;

impl Agent {
    /// An agent at the start of a duel: the two pings, its click rate, and
    /// where its player stands.
    pub fn new(ping_ms: [f64; 2], cps: f64, feet: V3) -> Agent {
        Agent {
            ping_ms,
            cps,
            click_tokens: 1.0 + (cps / 20.0).min(1.0),
            prev_inp: Input::default(),
            pearl_stage: 0,
            pearl_wait: 0,
            asked_pearl: false,
            potion_stage: 0,
            asked_potion: false,
            turned: 0,
            double: false,
            since_throw: 1000,
            trail: [feet; TRAIL],
            lag: (ping_ms[0] / 50.0).floor() as i32 + 1,
            fate: [None; 2],
            pearl_fate: [None; 2],
            opp_pearl: None,
            opp_pearl_age: 100_000,
            hp_last: -1.0,
            hp_drop: 0.0,
            hp_lost: [0.0; HURT_WINDOW],
            hurt_at: 0,
            consume: 0,
            asked_bar: 0,
            was_consuming: false,
            opp_use_ticks: 0,
            refill: 0,
            refill_kind: Item::Potion,
            combo: [0; 2],
            said: feet,
            said_before: feet,
            carry: -1,
            probe: None,
            teacher: Teacher::default(),
            tool_slot: NO_SLOT,
            full_heal: None,
            heal_plan: None,
            plan_t: 0,
            plan_pitch: 0.0,
            plan_pitch0: 0.0,
            safety: None,
            safety_acts: 0,
            turn_release: false,
            guess: None,
            opp_shown: None,
            guess_hp: MAX_HEALTH,
            guess_ids: [u32::MAX; 2],
            guess_first: -1.0,
            guess_changed: false,
            guess_shown: false,
            hidden: false,
        }
    }

    /// The opponent's health the policy is shown: what the server sends,
    /// a value set in its place (`Agent::opp_shown`) or the guess
    /// (`Agent::guess`).
    pub fn opp_health(&self, c: &Client) -> f32 {
        if let Some(f) = self.opp_shown {
            if !f.auto || self.hidden {
                return if f.health < 0.0 { c.lp.health } else { f.health };
            }
        }
        if self.guess_shown {
            self.guess_hp
        } else {
            c.rp.health
        }
    }

    /// Whether the server is seen to hide the opponent's health: it was
    /// shown at most one on the first tick of the duel (minemen.club sends
    /// one on every tick; a duel starts at full health) and never changed
    /// since. Kept only with `guess`, `opp_shown` or a guard that asks for
    /// it (`SafetyCfg::hidden`).
    pub fn hidden(&self) -> bool {
        self.hidden
    }

    /// `Agent::guess` and `Agent::hidden` for this tick, after the potions'
    /// fates are known.
    fn guess_tick(&mut self, c: &Client) {
        if self.guess.is_none() && self.opp_shown.is_none() && !self.safety.is_some_and(|s| s.hidden) {
            return;
        }
        let rp = &c.rp;
        if self.guess_first < 0.0 {
            self.guess_first = rp.health;
        }
        self.guess_changed |= rp.health != self.guess_first;
        self.hidden = !self.guess_changed && self.guess_first <= 1.0;
        // A hurt that follows a hit of its own: a rod, a snowball or an egg
        // hurts for nothing.
        let hit_seen = rp.ticks_since_hurt == 0 && c.lp.ticks_since_attack <= HIT_WINDOW;
        let Some(g) = self.guess else { return };
        if self.guess_hp < MAX_HEALTH && g.regen > 0 {
            self.guess_hp = (self.guess_hp + 1.0 / g.regen as f32).min(MAX_HEALTH);
        }
        if hit_seen {
            let hit = if rp.using && rp.item == Item::Sword { g.hit * g.scale * g.block } else { g.hit * g.scale };
            self.guess_hp = (self.guess_hp - hit).max(GUESS_MIN);
        }
        for k in 0..2 {
            if let Some((id, _, heal)) = self.fate[k] {
                if id != self.guess_ids[k] {
                    self.guess_ids[k] = id;
                    self.guess_hp = (self.guess_hp + heal[1]).min(MAX_HEALTH);
                }
            }
        }
        self.guess_shown = !g.auto || self.hidden;
    }

    /// Have it take the server to add its movement to its throws, or not,
    /// as if a potion of its own had told it (for checks).
    pub fn knows_carry(&mut self, carry: bool) {
        self.carry = carry as i8;
    }

    /// The movement across the ground a throw on the tick to come goes out
    /// with, if the server is known to add it.
    fn carried(&self) -> [f64; 2] {
        if self.carry == 1 {
            [self.said[0] - self.said_before[0], self.said[2] - self.said_before[2]]
        } else {
            [0.0; 2]
        }
    }

    /// The keys and buttons of the tick to come for what the policy chose
    /// (one number per control, see `ACT_DIMS`). An open inventory takes
    /// the whole tick; otherwise a drink or a meal, a potion and a pearl
    /// are laid over the plain keys in that order, and the click rate comes
    /// last.
    pub fn keys(&mut self, c: &Client, world: &World, duel: &Duel, action: &[i32], rng: &mut Rng) -> Input {
        let mut inp = decode_action(action);
        let mouse = inp.dx;
        let mut bar = if action.len() > 9 { action[9] } else { 0 };
        if self.turned > 0 {
            self.turned += 1;
            if self.turned == 2 {
                // The click of a potion thrown with a turn: nothing else of the hotbar.
                bar = 0;
            } else if self.turned == 3 && self.double {
                // The second of two: the next potion, the view kept still.
                bar = 1;
                inp.dx = 0;
                inp.dy = 0;
            } else if self.turned > TURN_SHOWN {
                self.turned = 0;
            }
            if self.turned >= 3 {
                self.double = false;
            }
        }
        let mut pitch_choice = if action.len() > 11 { action[11] } else { 0 };
        // A potion of the guard's thrown with no turn (`SafetyCfg::turn`).
        let mut down = false;
        if let Some(s) = self.safety {
            if self.potion_stage == 0 && self.turned == 0 {
                let asked = bar;
                bar = self.safety_bar(c, duel, bar);
                if bar != asked {
                    self.safety_acts += 1;
                    if bar >= BAR_TURN {
                        // The right button let go on the tick of the turn, so
                        // that the click is a press of its own.
                        inp.use_item = false;
                        if s.pitch >= 0 {
                            pitch_choice = s.pitch;
                        }
                        down = s.turn <= 0;
                        bar = BAR_TURN + (s.turn - 1).clamp(0, 2);
                    }
                }
            }
        }
        let turns = BAR_TURNS.len() as i32;
        let turn = if down {
            0
        } else if bar >= BAR_TURN {
            BAR_TURNS[((bar - BAR_TURN).min(2 * turns - 1) % turns) as usize]
        } else {
            0
        };
        let double = bar >= BAR_TURN + turns;
        if !self.refill_keys(c, bar, &mut inp) {
            self.consume_keys(c, duel, bar, &mut inp);
            let pitch = throw_pitch(pitch_choice);
            self.potion_keys(c, bar == 1 || turn != 0 || down, turn, down, pitch, &mut inp);
            if turn != 0 && self.turned == 1 {
                self.double = double;
                if self.potion_stage == 1 && self.full_heal.is_some() {
                    self.heal_plan_start(c, world, double, &mut inp);
                }
            }
            self.pearl_keys(c, action.len() > 8 && action[8] != 0, &mut inp);
            self.tool_keys(c, if action.len() > 12 { action[12] } else { 0 }, &mut inp);
            self.limit_clicks(&mut inp, rng);
            self.heal_plan_keys(c, &mut inp);
        }
        if self.teacher.on {
            self.teach(c, world, mouse, if self.turned == 1 { turn } else { 0 });
        }
        inp
    }

    /// A last guard against dying with potions at hand (`SafetyCfg`): the
    /// hotbar choice `bar` of the policy, or a potion thrown with a turn
    /// (`BAR_TURN`, at the policy's pitch) when its health is within what
    /// the opponent can take before a potion thrown now would heal. That
    /// potion's heal comes `lag` ticks (the click to the server and the
    /// server's answers back, which is how late this client sees its own
    /// health) plus the turn's tick plus its flight after this tick; the
    /// last hurt this client saw lets the next hit come 10 ticks after it
    /// on the server, and one every 10 ticks after that; or, if more,
    /// what waiting for the next hit to pot then would let in: that hit
    /// and those after it before the later potion's heal. Each hit is
    /// counted at the worst a hit costs (`crit` plain hits, or the last fall
    /// of its health if that took more), with `extra` hits and `margin`
    /// health more. Only with a potion at hand and nothing else of the
    /// hotbar under way, none of its own on the way, the opponent shown
    /// within `dist`, and a policy that asks for no potion itself; a drink
    /// or a meal under way is stopped first (choice 4), and the potion
    /// follows on a later tick.
    fn safety_bar(&self, c: &Client, duel: &Duel, bar: i32) -> i32 {
        let Some(s) = self.safety else { return bar };
        let lp = &c.lp;
        if (s.hidden && !self.hidden) || !duel.real_health || bar == 1 || bar >= BAR_TURN || lp.potions == 0 || self.refill != 0 || self.pearl_stage != 0 {
            return bar;
        }
        if lp.potions_pending > 0 || c.thrown.iter().any(|e| e.mine && e.kind == Kind::Potion) {
            return bar;
        }
        let (a, b) = (lp.l.body.pos, c.rp.pos);
        let (dx, dz) = (a[0] - b[0], a[2] - b[2]);
        if dx * dx + dz * dz > s.dist * s.dist {
            return bar;
        }
        let w = self.lag + 1 + s.flight;
        let first = (10 - lp.ticks_since_hurt.max(0)).max(0);
        let now = if first <= w { (w - first) / 10 + 1 } else { 0 };
        // Waiting for the next hit to pot then: that hit, and those that
        // can come after it before a potion thrown then would heal.
        let hits = now.max(1 + w / 10);
        let hit = (duel.hit_health * s.crit).max(self.hp_drop);
        if lp.health > (hits as f32 + s.extra) * hit + s.margin {
            return bar;
        }
        if self.consume != 0 {
            return 4;
        }
        if !self.potion_ready(c) {
            return bar;
        }
        BAR_TURN
    }

    /// The plan under way and its tick, for the dump that checks the mod's
    /// copy (examples/fh_dump.rs).
    pub fn heal_plan_info(&self) -> Option<(&fullheal::HealPlan, usize)> {
        self.heal_plan.as_ref().map(|p| (p, self.plan_t))
    }

    /// On the tick of the turn of a potion thrown
    /// with a turn, a plan that gives all eight points, if there is one:
    /// its pitch for the throw now, its keys from this tick on.
    fn heal_plan_start(&mut self, c: &Client, world: &World, double: bool, inp: &mut Input) {
        self.heal_plan = None;
        if double {
            return;
        }
        fullheal::count(0);
        let s = c.lp.ticks_since_hurt;
        let (a, b) = (c.lp.l.body.pos, c.rp.pos);
        let (dx, dz) = (a[0] - b[0], a[2] - b[2]);
        let d = (dx * dx + dz * dz).sqrt();
        fullheal::count(24 + s.clamp(0, 11) as usize);
        fullheal::count(36 + (d.max(0.0) as usize).min(9));
        let Some(cfg) = self.full_heal else { return };
        if !fullheal::gate_open(&cfg, s, d) {
            fullheal::count(46);
            return;
        }
        let own = fullheal::PKey { fw: inp.forward, st: inp.strafe, sprint: inp.sprint_key, jump: inp.jump };
        match fullheal::plan(c, world, inp, own, self.carry) {
            Some(p) => {
                fullheal::count(1 + p.class as usize);
                fullheal::count((7 + p.jump as i32) as usize);
                fullheal::count((12 + (p.ticks - 3).clamp(0, 6)) as usize);
                fullheal::count(if p.pitch < 0.0 { 19 } else { 20 });
                if self.carry >= 0 {
                    fullheal::count(21);
                }
                // The pitch the policy chose for this tick: the view goes
                // back to it on the click's tick (the throw has this one's).
                self.plan_pitch = fullheal::pitch_of(c.lp.l.pitch, inp.dy);
                self.plan_pitch0 = c.lp.l.pitch;
                inp.dy = p.dy;
                self.plan_t = 0;
                self.heal_plan = Some(p);
            }
            None => {
                fullheal::count(4);
                if self.carry < 0 {
                    fullheal::count(22);
                }
                if !c.lp.l.body.on_ground {
                    fullheal::count(23);
                }
            }
        }
    }

    /// The movement keys of the plan under way, laid over the policy's,
    /// until the potion is due or a hit has moved the player. On the
    /// click's tick the view takes the pitch the policy had chosen for the
    /// turn's tick (plus its own turn of this one): the server throws with
    /// the turn tick's view, so from here on the policy sees what it would
    /// have without the plan.
    fn heal_plan_keys(&mut self, c: &Client, inp: &mut Input) {
        let Some(p) = &self.heal_plan else { return };
        let t = self.plan_t;
        let restore = self.full_heal.map_or(4, |cfg| cfg.restore);
        match (restore, t) {
            (1, 1) => inp.dy += counts(self.plan_pitch - c.lp.l.pitch),
            (2, 2) => inp.dy = counts(self.plan_pitch0 - c.lp.l.pitch),
            (3, 1) => inp.dy = counts(self.plan_pitch - c.lp.l.pitch),
            (4, 1) => inp.dy = counts(self.plan_pitch0 - c.lp.l.pitch),
            _ => {}
        }
        if t >= p.len || (t > 0 && c.lp.ticks_since_kb == 0) {
            if t < p.len {
                fullheal::count(5);
            }
            self.heal_plan = None;
            return;
        }
        let yaw = (c.lp.l.yaw as f64 + inp.dx as f64 * COUNT_DEG as f64) as f32;
        let k = fullheal::keys_now(p, t, yaw);
        inp.forward = k.fw;
        inp.strafe = k.st;
        inp.sprint_key = k.sprint;
        inp.jump = k.jump;
        self.plan_t += 1;
    }

    /// What the teacher would have chosen on this tick (see `Teacher`):
    /// `mouse` is the policy's own turn of the yaw for it, `turn` the mouse
    /// counts of a turn for a potion made on it, 0 for none.
    fn teach(&mut self, c: &Client, world: &World, mouse: i32, turn: i32) {
        let t = &mut self.teacher;
        t.teach = [-1; ACT_HEADS];
        if t.tick > 0 {
            t.tick += 1;
        }
        let deg = COUNT_DEG as f64;
        if turn != 0 {
            let yaw = (c.lp.l.yaw as f64 + turn as f64 * deg) as f32;
            let (pitch, ticks) = self.teacher_aim(c, world, yaw);
            let t = &mut self.teacher;
            let f = yaw * mth::PI_F / 180.0f32;
            *t = Teacher { on: true, teach: [-1; ACT_HEADS], tick: 1, by: turn, pitch: c.lp.l.pitch, dir: [-mth::sin(f) as f64, mth::cos(f) as f64], until: (ticks + 2).clamp(3, MOVE_TICKS) };
            t.teach[11] = (((90.0 - pitch) / 2.0).round() as i32).clamp(0, THROW_PITCHES as i32 - 1);
            // Sprinting along the turned view, the mouse still.
            t.teach[..4].copy_from_slice(&[1, 0, 0, 1]);
            t.teach[5] = YAW_STILL;
            t.teach[10] = 0;
            return;
        }
        match t.tick {
            0 => {}
            2 => {
                // The click: still sprinting along the view, the mouse still.
                t.teach[..4].copy_from_slice(&[1, 0, 0, 1]);
                t.teach[5] = YAW_STILL;
                t.teach[6] = PITCH_STILL;
                t.teach[10] = 0;
            }
            k if k <= t.until => {
                if k == 3 {
                    // The view turns back, to the pitch it had.
                    let back = -t.by;
                    t.teach[10] = FLICK_BINS.iter().position(|&b| b == back || b == back + 2400).unwrap_or(0) as i32;
                    t.teach[5] = YAW_STILL;
                    let want = counts(t.pitch - c.lp.l.pitch);
                    t.teach[6] = (0..PITCH_BINS.len()).min_by_key(|&i| (PITCH_BINS[i] - want).abs()).unwrap_or(PITCH_STILL as usize) as i32;
                }
                // Walking the way it ran, from where the policy's mouse
                // has the view now.
                let yaw = (c.lp.l.yaw as f64 + mouse as f64 * deg) as f32;
                let (fw, st) = walk_along(yaw, t.dir);
                t.teach[0] = match fw { 1 => 1, -1 => 2, _ => 0 };
                t.teach[1] = match st { 1 => 1, -1 => 2, _ => 0 };
                t.teach[2] = 0;
                t.teach[3] = 0;
            }
            _ => t.tick = 0,
        }
    }

    /// The teacher's pitch for a potion thrown on the tick after one that
    /// turns the view to `yaw`, with the player sprinting along that view
    /// (`run_feet`), and the ticks that potion flies (`aim_potion`). On a
    /// server that adds the thrower's movement, or one it is not yet known
    /// of, half and half, with that movement.
    fn teacher_aim(&self, c: &Client, world: &World, yaw: f32) -> (f32, i32) {
        let run = run_feet(c, world, yaw);
        let other = self.potion_feet(c, -1, self.lag + 2, true);
        let ways: &[f64] = match self.carry {
            1 => &[1.0],
            0 => &[0.0],
            _ => &[0.0, 1.0],
        };
        aim_potion(c, world, yaw, &run, ways, |t| other(1, t))
    }

    /// Called after the client has made its tick with `inp`, with what
    /// came of it.
    pub fn ticked(&mut self, c: &Client, duel: &Duel, inp: &Input, out: &TickOut) {
        if out.threw_potion {
            self.since_throw = 0;
        }
        self.trail.copy_within(0..TRAIL - 1, 1);
        self.trail[0] = c.lp.l.body.pos;
        if self.since_throw < 1000 {
            self.since_throw += 1;
        }
        self.was_consuming = c.lp.consuming();
        let seen = c.rp.using && matches!(c.rp.item, Item::Drink | Item::Food);
        self.opp_use_ticks = if seen { self.opp_use_ticks + 1 } else { 0 };
        if self.opp_pearl_age < 100_000 {
            self.opp_pearl_age += 1;
        }
        if out.threw {
            // The server starts its wait when the throw reaches it, a
            // little later than this; two ticks more cover the difference.
            self.pearl_wait = duel.pearl_cooldown + 2;
        }
        self.prev_inp = *inp;
    }

    /// Whether a throw, a pearl, a drink or a meal or the inventory has the keys.
    pub fn busy(&self) -> bool {
        self.potion_stage != 0 || self.pearl_stage != 0 || self.consume != 0 || self.refill != 0
    }

    /// Is the view turned for a potion thrown with a turn?
    pub fn turned(&self) -> bool {
        self.turned == 1 || self.turned == 2
    }

    /// Clicks the player can still make on its next tick.
    fn clicks_available(&self) -> f64 {
        let r = (self.cps / 20.0).min(1.0);
        (self.click_tokens + r).min(1.0 + r)
    }

    /// Apply the click rate to what was asked for on this tick: drop a
    /// click the budget does not cover, or add the second click of a tick
    /// for a rate above twenty.
    pub fn limit_clicks(&mut self, inp: &mut Input, rng: &mut Rng) {
        let c = self.cps;
        if c >= 20.0 {
            if inp.attack && c > 20.0 && rng.chance((c - 20.0) / 20.0) {
                inp.extra_clicks = 1;
            }
            return;
        }
        let t = self.clicks_available();
        if inp.attack && t >= 1.0 - 1e-9 {
            self.click_tokens = t - 1.0;
        } else {
            inp.attack = false;
            self.click_tokens = t;
        }
    }

    /// Can the player throw a pearl now, as far as it can tell itself?
    fn pearl_ready(&self, c: &Client) -> bool {
        let lp = &c.lp;
        lp.pearls > 0
            && self.pearl_wait <= 0
            && self.pearl_stage == 0
            && self.potion_stage == 0
            && self.consume == 0
            && hand_free(lp)
    }

    /// Can the player throw a potion now? With the sword in hand, or on
    /// the tick after another potion.
    fn potion_ready(&self, c: &Client) -> bool {
        let lp = &c.lp;
        lp.potions != 0
            && self.pearl_stage == 0
            && self.consume == 0
            && match self.potion_stage {
                0 => hand_free(lp),
                2 => true,
                _ => false,
            }
    }

    /// Is the player, as far as its own client knows, without Speed or in
    /// the last seconds of it?
    fn drink_due(&self, c: &Client, duel: &Duel) -> bool {
        duel.drink_window_s < 0.0 || c.lp.speed_ticks <= duel.drink_window()
    }

    /// The key presses of a drink or a meal, laid over what the player
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
    fn consume_keys(&mut self, c: &Client, duel: &Duel, choice: i32, inp: &mut Input) {
        self.asked_bar = choice.clamp(0, ACT_DIMS[9] - 1) as u8;
        let lp = &c.lp;
        if self.consume == 0 {
            let slot = match choice {
                2 if lp.drinks != 0 && self.drink_due(c, duel) => lp.drinks.trailing_zeros() as u8,
                3 if lp.steak > 0 && lp.food < 20 => SLOT_FOOD,
                _ => return,
            };
            if !hand_free(lp) || self.pearl_stage != 0 || self.potion_stage != 0 {
                return;
            }
            inp.slot_key = slot + 1;
            inp.attack = false;
            inp.use_item = true;
            inp.use_again = self.prev_inp.use_item;
            self.consume = (choice - 1) as u8;
            self.was_consuming = false;
        } else if (self.was_consuming && !lp.consuming()) || choice == 4 || lp.held == SLOT_SWORD {
            inp.slot_key = SLOT_SWORD + 1;
            inp.use_item = false;
            self.consume = 0;
        } else {
            inp.use_item = true;
            inp.attack = false;
            // The click that did not start a use (a full stomach, as the
            // server knows before the client): give up.
            if !lp.consuming() && !self.was_consuming && self.prev_inp.slot_key == 0 {
                inp.slot_key = SLOT_SWORD + 1;
                inp.use_item = false;
                self.consume = 0;
            }
        }
    }

    /// One click of the mouse on a slot of the inventory. It comes out of
    /// the same budget as the clicks with the sword.
    fn take_click(&mut self) -> bool {
        if self.cps >= 20.0 {
            return true;
        }
        let t = self.clicks_available();
        let can = t >= 1.0 - 1e-9;
        self.click_tokens = if can { t - 1.0 } else { t };
        can
    }

    /// The inventory: `choice` 5 asks for a healing potion
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
    fn refill_keys(&mut self, c: &Client, choice: i32, inp: &mut Input) -> bool {
        if self.refill != 0 && !c.lp.inv_open {
            self.refill = 0;
            return false;
        }
        let want = match choice {
            5 => Some(Item::Potion),
            6 => Some(Item::Drink),
            _ => None,
        };
        let lp = &c.lp;
        let (bar, rows) = (lp.bar(), lp.rows);
        let inv;
        if self.refill == 0 {
            let Some(kind) = want else { return false };
            if !hand_free(lp) || self.pearl_stage != 0 || self.potion_stage != 0 || self.consume != 0 {
                return false;
            }
            if refill_click(&bar, &rows, kind).is_none() {
                return false;
            }
            self.refill = 1;
            self.refill_kind = kind;
            self.click_tokens = self.clicks_available();
            inv = 1;
        } else {
            let kind = if self.refill == 1 { Some(self.refill_kind) } else { want };
            match kind.and_then(|k| refill_click(&bar, &rows, k)) {
                None => {
                    self.refill = 0;
                    self.click_tokens = self.clicks_available();
                    inv = 0;
                }
                Some(slot) => {
                    if self.take_click() {
                        let (mut b, mut r) = (bar, rows);
                        shift_click(&mut b, &mut r, slot);
                        let more = refill_click(&b, &r, Item::Potion).is_some() || refill_click(&b, &r, Item::Drink).is_some();
                        if !more {
                            self.refill = 0;
                            inv = slot + INV_SHUT;
                        } else {
                            // A bottle put away is not yet the potion.
                            if slot < WINDOW_BAR {
                                self.refill = 2;
                            }
                            inv = slot;
                        }
                    } else {
                        inv = 1;
                    }
                }
            }
        }
        self.asked_bar = choice.clamp(0, ACT_DIMS[9] - 1) as u8;
        self.asked_potion = false;
        self.asked_pearl = false;
        if self.pearl_wait > 0 {
            self.pearl_wait -= 1;
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
    /// last one. It comes before the pearl when both are asked for. Thrown
    /// with a turn of `turn` mouse counts (see `ACT_DIMS`), the first tick
    /// turns the view and sets its pitch to `pitch`, and the click is made
    /// on the second.
    fn potion_keys(&mut self, c: &Client, want: bool, turn: i32, down: bool, pitch: f32, inp: &mut Input) {
        self.asked_potion = want;
        match self.potion_stage {
            0 => {
                if want && self.potion_ready(c) {
                    inp.slot_key = c.lp.potions.trailing_zeros() as u8 + 1;
                    inp.attack = false;
                    if turn != 0 || down {
                        // The turn goes to the server with this tick; the click follows it.
                        inp.dx += turn;
                        inp.dy = counts(pitch - c.lp.l.pitch);
                        if self.turn_release {
                            inp.use_item = false;
                        }
                        self.potion_stage = 1;
                        self.turned = 1;
                    } else if self.prev_inp.use_item {
                        inp.use_item = false;
                        self.potion_stage = 1;
                    } else {
                        inp.use_item = true;
                        self.potion_stage = 2;
                        self.probe_throw(c);
                    }
                }
            }
            1 => {
                inp.use_item = true;
                inp.attack = false;
                self.potion_stage = 2;
                self.probe_throw(c);
            }
            _ => {
                if want && self.potion_ready(c) {
                    inp.slot_key = c.lp.potions.trailing_zeros() as u8 + 1;
                    inp.attack = false;
                    inp.use_item = true;
                    inp.use_again = self.prev_inp.use_item;
                    self.probe_throw(c);
                } else {
                    inp.slot_key = SLOT_SWORD + 1;
                    inp.use_item = false;
                    self.potion_stage = 0;
                }
            }
        }
    }

    /// The key presses of the tool, laid over what the player asked for on
    /// this tick: `choice` 1 uses it, 2 takes the sword back. Asked with the
    /// sword in hand and nothing else under way, 1 presses the number key
    /// of the tool with a right click: a rod is cast, a snowball or an egg
    /// thrown. Asked again with the tool in hand, the button is let go and
    /// pressed again between the two ticks, a click out of the same budget
    /// as the left ones, which brings a hook back or throws the next ball;
    /// with no click left the button stays down, and the game uses the tool
    /// again every fourth tick while it is. With the tool in hand and 0
    /// asked the right button is the right mouse control's. 2 presses the
    /// number key of the sword. No left click is made with a right click of
    /// the tool: one that lands on a block would swallow it. Snowballs or
    /// eggs used up in hand leave the slot empty, and the sword is taken
    /// back on the next tick that has nothing else under way.
    fn tool_keys(&mut self, c: &Client, choice: i32, inp: &mut Input) {
        let lp = &c.lp;
        let tool = lp.tool;
        if self.busy() || inp.slot_key != 0 {
            return;
        }
        if tool.item == Item::Empty {
            if self.tool_slot != NO_SLOT && lp.held == self.tool_slot {
                inp.slot_key = SLOT_SWORD + 1;
                inp.use_item = false;
                self.tool_slot = NO_SLOT;
            }
            return;
        }
        self.tool_slot = tool.slot;
        let holding = lp.held == tool.slot;
        match choice {
            1 if !holding && lp.held == SLOT_SWORD => {
                inp.slot_key = tool.slot + 1;
                inp.use_item = true;
                inp.use_again = self.prev_inp.use_item;
                inp.attack = false;
            }
            1 if holding => {
                inp.use_item = true;
                if self.prev_inp.use_item {
                    inp.use_again = self.take_click();
                }
                if inp.use_again || !self.prev_inp.use_item {
                    inp.attack = false;
                }
            }
            2 if holding => {
                inp.slot_key = SLOT_SWORD + 1;
                inp.use_item = false;
            }
            _ => {}
        }
    }

    /// A potion click on this tick: the throw the server will make of it
    /// from the view it has, and the movement it goes out with, for the
    /// potion to tell when it shows whether the server adds that.
    fn probe_throw(&mut self, c: &Client) {
        let (_, dir) = throw_heading(Kind::Potion, c.lp.l.body.pos, c.lp.l.yaw, c.lp.l.pitch);
        let v = Kind::Potion.speed() as f64;
        self.probe = Some(([dir[0] * v, dir[2] * v], [self.said[0] - self.said_before[0], self.said[2] - self.said_before[2]], 0));
    }

    /// The key presses of a pearl throw, laid over what the player asked
    /// for on this tick. Tick one: the number key of the pearls and a right
    /// click, which throws along the view the server already has. Tick two:
    /// the number key of the sword. If the right button was down, it is let
    /// go on tick one and the click comes a tick later. No left click is
    /// made on the tick of the throw, because one that lands on a block
    /// would swallow the right click.
    fn pearl_keys(&mut self, c: &Client, want: bool, inp: &mut Input) {
        if self.pearl_wait > 0 {
            self.pearl_wait -= 1;
        }
        self.asked_pearl = want;
        match self.pearl_stage {
            0 => {
                if want && self.pearl_ready(c) {
                    inp.slot_key = SLOT_PEARL + 1;
                    inp.attack = false;
                    if self.prev_inp.use_item {
                        inp.use_item = false;
                        self.pearl_stage = 1;
                    } else {
                        inp.use_item = true;
                        self.pearl_stage = 2;
                    }
                }
            }
            1 => {
                inp.use_item = true;
                inp.attack = false;
                self.pearl_stage = 2;
            }
            _ => {
                inp.slot_key = SLOT_SWORD + 1;
                inp.use_item = false;
                self.pearl_stage = 0;
            }
        }
    }

    /// Where the two players are on the server while a potion flies there,
    /// as this client can tell: the first of the two is its own
    /// player and the second its opponent, on the potion's tick `t` from
    /// now. Of the opponent the client has what the server last sent, and
    /// takes it to go on as it does, `on` ticks further on the potion's
    /// next tick. Of itself it knows more than the server: its moves take
    /// `lag` ticks to get there and for what the server makes of them to
    /// come back, so what it sees of a potion happened around where it
    /// stood that long ago. `back` is how many ticks ago it stood where
    /// the server has it on the potion's next tick; from now on it keeps
    /// moving as it does, or stops if not `moving`.
    fn potion_feet<'a>(&'a self, c: &'a Client, back: i32, on: i32, moving: bool) -> impl Fn(usize, i32) -> V3 + 'a {
        let (b, rp) = (&c.lp.l.body, &c.rp);
        let seen = [rp.pos[0] - rp.prev_pos[0], rp.pos[2] - rp.prev_pos[2]];
        let seen = if seen[0].hypot(seen[1]) < 1.5 { seen } else { [0.0; 2] };
        let own = pace(b.motion, b.on_ground);
        let there = if rp.increments > 0 { rp.target } else { rp.pos };
        let (here, trail) = (b.pos, &self.trail);
        move |j: usize, t: i32| {
            if j == 1 {
                let n = (on + t) as f64;
                return [there[0] + seen[0] * n, there[1], there[2] + seen[1] * n];
            }
            let k = back - t;
            if k >= 0 {
                trail[(k as usize).min(TRAIL - 1)]
            } else if moving {
                [here[0] - own[0] * k as f64, here[1], here[2] - own[1] * k as f64]
            } else {
                here
            }
        }
    }

    /// `potion_outcome` for a potion this client sees in flight, with its
    /// own player first. The copy has made `age` ticks of its own and
    /// moves as the server's did after `air`: the server made that tick
    /// `age - 1` ticks after the one this client first saw the potion in,
    /// so on the tick after it the player was where it stood
    /// `lag + age - 1 - air` ticks ago, and the opponent is
    /// `air - age + 1` ticks on from what was last seen of it. The copy
    /// flies on through whatever the server's broke on until the server
    /// says it is gone, so what was foreseen for the tick before it broke
    /// is kept (`fate`).
    fn potion_seen(&self, c: &Client, world: &World, e: &ClientThrown) -> (V3, i32, [f32; 2]) {
        if let Some((known, at, heal)) = self.fate[e.mine as usize] {
            if known == e.id {
                return (at, 0, heal);
            }
        }
        let thrower = if e.mine { 0 } else { 1 };
        let feet = self.potion_feet(c, self.lag + e.age - 1 - e.air, e.air - e.age + 1, true);
        potion_outcome(world, e.server_side(), e.motion, e.air, thrower, feet)
    }

    /// `pearl_landing` for a pearl this client sees in flight, kept from
    /// the tick before it lands as for a potion.
    fn pearl_seen(&self, world: &World, e: &ClientThrown) -> (V3, i32) {
        match self.pearl_fate[e.mine as usize] {
            Some((known, at)) if known == e.id => (at, 0),
            _ => pearl_landing(world, e),
        }
    }

    /// Called when the packets of this client are in and its tick is to
    /// come, which is when it is told what it sees. A hurt that came in
    /// since its last tick goes to the hits in a row. Its own potion, seen
    /// for the first time: the ticks since it threw it are how far behind
    /// the server's answers are. And what it foresees for the potions and
    /// pearls that break on their next tick is kept.
    pub fn before_tick(&mut self, c: &Client, world: &World) {
        let h = c.lp.health;
        let d = if self.hp_last >= 0.0 { (self.hp_last - h).max(0.0) } else { 0.0 };
        self.hp_last = h;
        if d > 0.0 {
            self.hp_drop = d;
        }
        self.hp_lost[self.hurt_at] = d;
        self.hurt_at = (self.hurt_at + 1) % HURT_WINDOW;
        self.said_before = self.said;
        self.said = c.lp.l.body.pos;
        if let Some((thrown, moved, age)) = self.probe {
            // Its potion as the server has it after its first tick: the
            // throw and what was added to it, slowed by the air once.
            if let Some(e) = c.thrown.iter().rev().find(|e| e.mine && e.kind == Kind::Potion && e.age == 0) {
                let n = moved[0] * moved[0] + moved[1] * moved[1];
                if n > CARRY_MOVED * CARRY_MOVED {
                    let r = [e.motion[0] / 0.99 - thrown[0], e.motion[2] / 0.99 - thrown[1]];
                    self.carry = ((r[0] * moved[0] + r[1] * moved[1]) / n > 0.5) as i8;
                }
                self.probe = None;
            } else if age >= 20 {
                self.probe = None;
            } else {
                self.probe = Some((thrown, moved, age + 1));
            }
        }
        if c.rp.ticks_since_hurt == 0 && c.lp.ticks_since_attack <= HIT_WINDOW {
            self.combo = [self.combo[0] + 1, 0];
        }
        if c.lp.ticks_since_hurt == 0 && c.lp.ticks_since_moved != 0 {
            self.combo = [0, self.combo[1] + 1];
        }
        if c.lp.potions_pending == 0 && self.since_throw < TRAIL as i32 && c.thrown.iter().any(|e| e.mine && e.kind == Kind::Potion && e.age == 0) {
            self.lag = self.since_throw.max(1);
        }
        for mine in [false, true] {
            let newest = |kind: Kind| c.thrown.iter().rev().find(|e| e.mine == mine && e.kind == kind);
            let fate = newest(Kind::Potion).and_then(|e| {
                let (at, ticks, heal) = self.potion_seen(c, world, e);
                (ticks == 0).then_some((e.id, at, heal))
            });
            let pearl = newest(Kind::Pearl).and_then(|e| {
                let (at, ticks) = self.pearl_seen(world, e);
                (ticks == 0).then_some((e.id, at))
            });
            self.fate[mine as usize] = fate;
            self.pearl_fate[mine as usize] = pearl;
        }
        if let Some(e) = c.thrown.iter().rev().find(|e| !e.mine && e.kind == Kind::Pearl) {
            if self.opp_pearl != Some(e.id) {
                self.opp_pearl = Some(e.id);
                self.opp_pearl_age = 0;
            }
        }
        self.guess_tick(c);
    }

    /// What the policy is shown: what the agent can know from its own
    /// client and of the duel.
    pub fn write_obs(&self, c: &Client, world: &World, ccfg: &ClientCfg, duel: &Duel, o: &mut [f32]) {
        let combo = self.combo;
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
        o[13] = clip((b.pos[0] - duel.center[0]) / duel.arena_half, 1.5);
        o[14] = clip((b.pos[2] - duel.center[1]) / duel.arena_half, 1.5);
        o[15] = clip((b.pos[1] - duel.floor_y) / 3.0, 2.0);
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
        let pk = c.pick(world, ccfg);
        o[44] = pk.entity as u8 as f32;
        let ebb = rp.bb.expand(0.1, 0.1, 0.1);
        let reach = dist(eye, ebb.closest_point(eye));
        o[45] = clip(reach / 6.0, 3.0);
        o[46] = (reach <= 3.0) as u8 as f32;
        o[47] = clip((reach - 3.0) / 1.0, 1.0);
        o[48] = (rp.ticks_since_hurt.min(20)) as f32 / 20.0;
        o[49] = (lp.ticks_since_hurt.min(20)) as f32 / 20.0;
        o[50] = (self.ping_ms[0] / 200.0) as f32;
        o[51] = (self.ping_ms[1] / 200.0) as f32;
        let pi = &self.prev_inp;
        let dpc = ccfg.mouse_f1 as f64 * 0.15;
        o[52] = pi.forward as f32;
        o[53] = pi.strafe as f32;
        o[54] = pi.jump as u8 as f32;
        o[55] = pi.sprint_key as u8 as f32;
        o[56] = pi.attack as u8 as f32;
        o[57] = clip(pi.dx as f64 * dpc / 30.0, 2.0);
        o[58] = clip(pi.dy as f64 * dpc / 15.0, 1.0);
        o[59] = (combo[0] as f32 / 10.0).min(2.0) - (combo[1] as f32 / 10.0).min(2.0);
        let avail = self.clicks_available();
        o[60] = (avail >= 1.0 - 1e-9) as u8 as f32;
        o[61] = (avail / 2.0) as f32;
        o[62] = (self.cps / 20.0) as f32;
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
        let cd = duel.pearl_cooldown.max(1) as f32;
        o[68] = (lp.held == SLOT_SWORD) as u8 as f32;
        o[69] = lp.pearls as f32 / 16.0;
        o[70] = self.pearl_ready(c) as u8 as f32;
        o[71] = (self.pearl_wait.max(0) as f32 / cd).min(1.0);
        o[72] = self.pearl_stage as f32 / 2.0;
        o[73] = self.asked_pearl as u8 as f32;
        o[74] = (rp.item == Item::Pearl) as u8 as f32;
        o[75] = (lp.ticks_since_moved.min(20)) as f32 / 20.0;
        o[76] = (rp.ticks_since_jump.min(20)) as f32 / 20.0;
        // The newest pearl of its own in flight: where it will put this
        // player (relative to where it stands now), in how many ticks, and
        // how far from the opponent that is.
        for v in o[77..TOOL_AT].iter_mut() {
            *v = 0.0;
        }
        if let Some(e) = c.thrown.iter().rev().find(|e| e.mine && e.kind == Kind::Pearl) {
            let (land, ticks) = self.pearl_seen(world, e);
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
            let (land, ticks) = self.pearl_seen(world, e);
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
            let land_at = |yaw: f32| {
                if self.carry == 1 {
                    let (from, dir) = throw_heading(Kind::Pearl, b.pos, yaw, lp.l.pitch);
                    let (v, add) = (Kind::Pearl.speed() as f64, self.carried());
                    thrown_flight(world, Kind::Pearl, from, [dir[0] * v + add[0], dir[1] * v, dir[2] * v + add[1]])
                } else {
                    pearl_preview(world, b.pos, yaw, lp.l.pitch)
                }
            };
            let (land, ticks) = land_at(lp.l.yaw);
            let (lf, lr) = local(yaw, land[0] - b.pos[0], land[2] - b.pos[2]);
            o[92] = clip(lf.hypot(lr) / 16.0, 4.0);
            o[93] = (ticks.min(60)) as f32 / 30.0;
            o[94] = clip((lf - rf) / 8.0, 4.0);
            o[95] = clip((lr - rr) / 8.0, 4.0);
            // The same for a view turned first, to either side: where a
            // pearl would put this player as seen from the opponent, and in
            // how many ticks.
            for (k, turn) in PEARL_TURNS.iter().enumerate() {
                let (land, ticks) = land_at(lp.l.yaw + turn);
                let (lf, lr) = local(yaw, land[0] - b.pos[0], land[2] - b.pos[2]);
                let at = PEARL_AT + 3 * k;
                o[at] = clip((lf - rf) / 8.0, 4.0);
                o[at + 1] = clip((lr - rr) / 8.0, 4.0);
                o[at + 2] = (ticks.min(60)) as f32 / 30.0;
            }
        }
        // Health: whether hits cost any in this duel, this player's and the
        // opponent's as this client knows them, and the health a plain hit
        // costs with this armour and these swords.
        let real = duel.real_health;
        let hh = duel.hit_health;
        o[96] = real as u8 as f32;
        o[97] = lp.health / MAX_HEALTH;
        let rh = self.opp_health(c);
        o[98] = rh / MAX_HEALTH;
        o[99] = (lp.health - rh) / MAX_HEALTH;
        o[100] = if real { hh / 4.0 } else { 0.0 };
        o[101] = ((MAX_HEALTH - lp.health) / POTION_HEAL).min(2.5);
        // How hard it is being hit: what the last fall of its health took
        // and what its last second took, in hearts over five.
        let mut lost = 0.0f32;
        for k in 0..HURT_WINDOW {
            lost += self.hp_lost[(self.hurt_at + k) % HURT_WINDOW];
        }
        o[HURT_AT] = (self.hp_drop / 10.0).min(2.0);
        o[HURT_AT + 1] = (lost / 10.0).min(2.0);
        // Potions: how many are left, whether one can be thrown now, where
        // the throw's key presses are, what was asked last tick, whether
        // the opponent has one in hand and how many of its potions this
        // client has seen.
        o[102] = lp.potions.count_ones() as f32 / 7.0;
        o[103] = self.potion_ready(c) as u8 as f32;
        o[104] = self.potion_stage as f32 / 2.0;
        o[105] = self.asked_potion as u8 as f32;
        o[106] = (rp.item == Item::Potion) as u8 as f32;
        o[107] = (lp.potions_seen.min(14)) as f32 / 7.0;
        // Its own newest potion in flight: where it will break (relative to
        // where this player stands now), in how many ticks, and the health
        // it will then give this player and the opponent.
        if let Some(e) = c.thrown.iter().rev().find(|e| e.mine && e.kind == Kind::Potion) {
            let (at, ticks, heal) = self.potion_seen(c, world, e);
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
            let (at, ticks, heal) = self.potion_seen(c, world, e);
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
            let add = self.carried();
            let m = [dir[0] * v + add[0], dir[1] * v, dir[2] * v + add[1]];
            // The server takes the throw from where this player stands
            // now and has it a step further on the potion's first tick;
            // the opponent is by then as much further as the throw takes
            // to get there and what was last seen of it took to get here.
            let lag = self.lag;
            let (at, ticks, heal) = potion_outcome(world, from, m, 0, 0, self.potion_feet(c, -1, lag + 1, true));
            let still = potion_outcome(world, from, m, 0, 0, self.potion_feet(c, -1, lag + 1, false)).2;
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
        o[138] = self.consume as f32 / 2.0;
        o[139] = self.asked_bar as f32 / 4.0;
        o[140] = rp.fx as u8 as f32;
        o[141] = (rp.using && rp.item == Item::Drink) as u8 as f32;
        o[142] = (rp.using && rp.item == Item::Food) as u8 as f32;
        o[143] = self.opp_use_ticks.min(48) as f32 / USE_TICKS as f32;
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
        o[149] = (self.refill == 1) as u8 as f32;
        // The Speed it has left out of the last seconds in which a drink
        // is taken: 1 with more than that, when a drink would not be.
        o[150] = if duel.drink_window_s < 0.0 {
            speed as u8 as f32
        } else {
            (lp.speed_ticks.max(0) as f32 / duel.drink_window().max(1) as f32).min(1.0)
        };
        // How much of the server's wait between pearls the opponent still
        // has in front of it, counted from when its last pearl was seen.
        o[151] = ((duel.pearl_cooldown - self.opp_pearl_age).max(0) as f32 / cd).min(1.0);
        // A potion thrown with a turn: the ticks left of the first
        // `TURN_SHOWN` after the turn.
        o[152] = if self.turned > 0 { (TURN_SHOWN + 1 - self.turned) as f32 / TURN_SHOWN as f32 } else { 0.0 };
        // Whether its throws take on its movement: not seen yet, no, yes.
        o[153] = [0.5, 0.0, 1.0][(self.carry + 1) as usize];
        // The ground around it: steps, walls, pits and drops.
        ground_grid(world, b.pos, yaw, &mut o[GRID_AT..TOOL_AT]);
        // Cobwebs: in one, the opponent in one, and those around it.
        if world.has_webs() {
            o[WEB_AT] = world.in_web(&crate::aabb::Aabb::player(b.pos)) as u8 as f32;
            o[WEB_AT + 1] = world.in_web(&crate::aabb::Aabb::player(rp.pos)) as u8 as f32;
            web_grid(world, b.pos, yaw, &mut o[WEB_AT + 2..TOOL_AT]);
        } else {
            o[WEB_AT..TOOL_AT].fill(0.0);
        }
        self.tool_obs(c, &mut o[TOOL_AT..OBS_DIM]);
    }

    /// The tool of its kit, the hooks out and the snowballs and eggs in
    /// flight. A hook is where the server last said it is, in the 32nds of
    /// a block positions come in: a client's own copy of it moves in ways
    /// of its own between those, and the server's is the one that pulls.
    fn tool_obs(&self, c: &Client, o: &mut [f32]) {
        o.fill(0.0);
        let (lp, rp) = (&c.lp, &c.rp);
        let b = &lp.l.body;
        let yaw = lp.l.yaw;
        let tool = lp.tool;
        let rel = |o: &mut [f32], p: V3| {
            let (f, r) = local(yaw, p[0] - b.pos[0], p[2] - b.pos[2]);
            o[0] = clip(f / 8.0, 4.0);
            o[1] = clip(r / 8.0, 4.0);
            o[2] = clip((p[1] - b.pos[1]) / 3.0, 2.0);
        };
        match tool.item {
            Item::Rod => {
                o[0] = 1.0;
                o[3] = 1.0 - tool.wear as f32 / ROD_MAX_WEAR as f32;
            }
            Item::Snowball | Item::Egg => {
                o[if tool.item == Item::Snowball { 1 } else { 2 }] = 1.0;
                o[3] = (tool.n as f32 / 16.0).min(1.0);
            }
            _ => {}
        }
        o[4] = (tool.item != Item::Empty && lp.held == tool.slot) as u8 as f32;
        if let Some(e) = c.thrown.iter().rev().find(|e| e.mine && e.kind == Kind::Hook) {
            let p = e.server_side();
            o[5] = 1.0;
            o[6] = (e.age.min(40)) as f32 / 20.0;
            rel(&mut o[7..10], p);
            o[10] = clip(dist(p, rp.pos) / 4.0, 4.0);
        }
        o[11] = (rp.item == Item::Rod) as u8 as f32;
        o[12] = matches!(rp.item, Item::Snowball | Item::Egg) as u8 as f32;
        if let Some(e) = c.thrown.iter().rev().find(|e| !e.mine && e.kind == Kind::Hook) {
            o[13] = 1.0;
            rel(&mut o[14..17], e.server_side());
        }
        // The opponent's snowball or egg nearest to this player, and how it moves.
        let near = c
            .thrown
            .iter()
            .filter(|e| !e.mine && matches!(e.kind, Kind::Snowball | Kind::Egg))
            .min_by(|a, b2| dist_sq(a.pos, b.pos).total_cmp(&dist_sq(b2.pos, b.pos)));
        if let Some(e) = near {
            o[17] = 1.0;
            rel(&mut o[18..21], e.pos);
            let (mf, mr) = local(yaw, e.motion[0], e.motion[2]);
            o[21] = clip(mf / 2.0, 2.0);
            o[22] = clip(mr / 2.0, 2.0);
            o[23] = clip(e.motion[1] / 2.0, 2.0);
        }
        o[24] = c.thrown.iter().any(|e| e.mine && matches!(e.kind, Kind::Snowball | Kind::Egg)) as u8 as f32;
    }
}

/// The sword in hand, or the tool of the kit: a pearl, a potion, a drink
/// or a meal can be started from either, with its own number key.
#[inline]
fn hand_free(lp: &crate::client::LocalPlayer) -> bool {
    lp.held == SLOT_SWORD || (lp.tool.item != Item::Empty && lp.held == lp.tool.slot)
}

/// Of W, S, A, D and their pairs, the keys that take a player looking at
/// `yaw` closest to the way `dir` (x and z, of length one), as forward
/// and strafe (1, 0 or -1).
pub fn walk_along(yaw: f32, dir: [f64; 2]) -> (i8, i8) {
    let f = yaw * mth::PI_F / 180.0f32;
    let (sn, cs) = (mth::sin(f) as f64, mth::cos(f) as f64);
    let mut best = (f64::MIN, 0i8, 0i8);
    for fw in [1i8, 0, -1] {
        for st in [0i8, 1, -1] {
            if fw == 0 && st == 0 {
                continue;
            }
            let (x, z) = (st as f64 * cs - fw as f64 * sn, fw as f64 * cs + st as f64 * sn);
            let d = (x * dir[0] + z * dir[1]) / (x * x + z * z).sqrt();
            if d > best.0 + 1e-9 {
                best = (d, fw, st);
            }
        }
    }
    (best.1, best.2)
}

/// Where a player that turns its view to `yaw` on the tick to come will
/// stand after that tick and after each of the next, as its own client can
/// work out: sprinting along that view for two ticks, then walking on the
/// same way (`walk_along`), as the teacher plays it. The pull of its keys and the friction of the
/// ground or the air on the motion it has, the fall to the ground under
/// it, and a wall in its way.
pub fn run_feet(c: &Client, world: &World, yaw: f32) -> [V3; RUN_TICKS] {
    let l = &c.lp.l;
    let (mut pos, mut m, mut on_ground) = (l.body.pos, l.body.motion, l.body.on_ground);
    let floor = match world.ray_trace_blocks([pos[0], pos[1] + 0.1, pos[2]], [pos[0], pos[1] - 6.0, pos[2]], false) {
        RayHit::Block(h) => h[1],
        _ => pos[1] - 6.0,
    };
    // The speed it has with the sprint it will have: on with food enough,
    // off without, and off once it walks.
    let sprint = c.lp.food > 6;
    let run = l.attr.value() * if sprint == l.sprinting { 1.0 } else if sprint { 1.3 } else { 1.0 / 1.3 };
    let walk = if sprint { run / 1.3 } else { run };
    let f = yaw * mth::PI_F / 180.0f32;
    let dir = [-mth::sin(f) as f64, mth::cos(f) as f64];
    let mut out = [pos; RUN_TICKS];
    for (q, at) in out.iter_mut().enumerate() {
        let sprinting = sprint && q < 2;
        let a = if on_ground { (if q < 2 { run } else { walk }) * 0.98 } else if sprinting { 0.026 * 0.98 } else { 0.02 * 0.98 };
        m[0] += dir[0] * a;
        m[2] += dir[1] * a;
        let step = (m[0] * m[0] + m[2] * m[2]).sqrt();
        if step > 1e-9 {
            let reach = 1.0 + 0.3 / step;
            let (from, to) = ([pos[0], pos[1] + 0.5, pos[2]], [pos[0] + m[0] * reach, pos[1] + 0.5, pos[2] + m[2] * reach]);
            if world.clear_path(from, to) || !matches!(world.ray_trace_blocks(from, to, false), RayHit::Block(_)) {
                pos[0] += m[0];
                pos[2] += m[2];
            } else {
                m[0] = 0.0;
                m[2] = 0.0;
            }
        }
        let fr = if on_ground { 0.546 } else { 0.91 };
        pos[1] += m[1];
        on_ground = pos[1] <= floor;
        if on_ground {
            pos[1] = floor;
            m[1] = 0.0;
        }
        m[1] = (m[1] - 0.08) * 0.98;
        m[0] *= fr;
        m[2] *= fr;
        *at = pos;
    }
    out
}

/// The pitch for a potion thrown on the tick after one that turns the
/// view of the player of `c` to `yaw`, with that player then where `run`
/// says (after the turn's tick and each one after) and the other one where
/// `other` says at each tick, and the ticks that potion flies. Of the
/// pitches from straight down to 20 degrees under level, in steps of one,
/// the one whose potion gives the thrower the most health on average over
/// the little the server throws off the view at random and over `ways`
/// (how much of the thrower's movement the server adds, 0 or 1), then
/// breaks soonest, then gives the other one the least. Used by the
/// teacher and by the scripted bots that pot well.
pub fn aim_potion(c: &Client, world: &World, yaw: f32, run: &[V3; RUN_TICKS], ways: &[f64], other: impl Fn(i32) -> V3) -> (f32, i32) {
    let v = Kind::Potion.speed() as f64;
    let b = c.lp.l.body.pos;
    let moved = [run[0][0] - b[0], run[0][2] - b[2]];
    let feet = |j: usize, t: i32| if j == 0 { run[((t + 1).max(0) as usize).min(RUN_TICKS - 1)] } else { other(t) };
    let mut out = [(0i32, 0.0f64, 0.0f64); AIM_PITCHES];
    for (k, o) in out.iter_mut().enumerate() {
        let (from, dir) = throw_heading(Kind::Potion, run[0], yaw, 90.0f32 - k as f32);
        let (mut sum, mut opp, mut all) = (0.0f64, 0.0f64, 0.0f64);
        for (q, &p) in SPREAD_WEIGHTS.iter().enumerate() {
            let z = (q as f64 - 4.0) * 0.5;
            for &w in ways {
                let m = [dir[0] * v + w * moved[0], (dir[1] + z * THROW_SPREAD) * v, dir[2] * v + w * moved[1]];
                let (_, ticks, heal) = potion_outcome(world, from, m, 0, 0, &feet);
                o.0 = o.0.max(ticks);
                sum += p * heal[0] as f64;
                opp += p * heal[1] as f64;
                all += p;
            }
        }
        *o = (o.0, sum / all, opp / all);
    }
    let mut best = 0usize;
    for k in 1..AIM_PITCHES {
        let (b, o) = (out[best], out[k]);
        let same = (o.1 - b.1).abs() <= 1e-9;
        if (o.1 > b.1 && !same) || (same && (o.0 < b.0 || (o.0 == b.0 && o.2 < b.2))) {
            best = k;
        }
    }
    (90.0f32 - best as f32, out[best].0)
}

/// The mouse counts of a turn by `deg` degrees, to the nearest.
fn counts(deg: f32) -> i32 {
    (deg / COUNT_DEG + 0.5).floor() as i32
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
        dx: YAW_BINS[pick(a[5], 31)] + if a.len() > 10 { FLICK_BINS[pick(a[10], 6)] } else { 0 },
        dy: PITCH_BINS[pick(a[6], 29)],
        use_item: a.len() > 7 && a[7] != 0,
        use_again: false,
        slot_key: 0,
        inv: 0,
    }
}

#[inline]
pub(crate) fn local(yaw: f32, x: f64, z: f64) -> (f64, f64) {
    let (s, c) = (yaw as f64).to_radians().sin_cos();
    // Forward is (-sin, cos); right is (-cos, -sin).
    (-s * x + c * z, -c * x - s * z)
}

#[inline]
pub(crate) fn clip(v: f64, lim: f64) -> f32 {
    v.clamp(-lim, lim) as f32
}

/// Yaw error, pitch error (degrees) and horizontal distance from an eye
/// with the given rotation to the nearest height of a box.
pub(crate) fn aim_error(eye: V3, yaw: f32, pitch: f32, bb: &Aabb) -> (f64, f64, f64) {
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
/// do: where it breaks (its position at the start of that tick, from which
/// its reach is measured), after how many ticks, and the health it offers
/// each player. `feet(j, t)` is where player `j` stands on the server on
/// the potion's tick `t` from now, `thrower` the index of the one who threw
/// it, whom it passes through for its first ticks.
pub(crate) fn potion_outcome(world: &World, mut pos: V3, mut m: V3, mut age: i32, thrower: usize, feet: impl Fn(usize, i32) -> V3) -> (V3, i32, [f32; 2]) {
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
        let at = |j: usize| feet(j, t);
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

/// Everything an agent reads of its client, as a row of numbers: what the
/// client holds when its packets are in and its tick is to come. A copy of
/// the agent elsewhere is handed this, and a real client has to show the
/// same. The slot and the ticks of an item in use are told only while one
/// is in use.
pub fn view(c: &Client, world: &World, ccfg: &ClientCfg) -> Vec<f64> {
    struct Out(Vec<f64>);
    impl Out {
        fn n<T: Into<f64>>(&mut self, v: T) {
            self.0.push(v.into());
        }
        fn b(&mut self, v: bool) {
            self.0.push(v as u8 as f64);
        }
        fn v(&mut self, v: V3) {
            self.0.extend_from_slice(&v);
        }
    }
    let mut o = Out(Vec::with_capacity(VIEW_HEAD + VIEW_THROWN * c.thrown.len()));
    let (lp, rp) = (&c.lp, &c.rp);
    let b = &lp.l.body;
    o.v(b.pos);
    o.v(b.motion);
    o.b(b.on_ground);
    o.b(b.collided_h);
    o.n(b.fall_distance);
    o.n(lp.l.yaw);
    o.n(lp.l.pitch);
    o.b(lp.l.sprinting);
    o.n(lp.l.jump_ticks);
    o.n(lp.l.attr.value());
    o.b(lp.server_sprint_state);
    o.n(lp.hurt_time);
    o.v(lp.last_kb);
    o.n(lp.ticks_since_kb);
    o.n(lp.ticks_since_swing);
    o.n(lp.ticks_since_attack);
    o.n(lp.ticks_since_hurt);
    o.n(lp.ticks_since_moved);
    o.b(lp.using_item);
    o.n(lp.right_click_delay);
    o.b(lp.flag_using);
    o.n(lp.held);
    o.n(lp.pearls);
    o.n(lp.potions);
    o.n(lp.drinks);
    o.n(lp.bottles);
    o.n(lp.steak);
    o.n(lp.potions_seen);
    o.n(lp.potions_pending);
    o.n(lp.food);
    o.n(lp.saturation);
    o.n(lp.speed_ticks);
    o.n(lp.health);
    o.b(lp.consuming());
    o.n(if lp.using_item { lp.use_slot } else { 0 });
    o.n(if lp.using_item { lp.use_count } else { 0 });
    o.b(lp.inv_open);
    for x in lp.rows.0 {
        o.n(x);
    }
    o.v(rp.pos);
    o.v(rp.prev_pos);
    o.v(rp.target);
    o.n(rp.increments);
    o.v(rp.bb.min);
    o.v(rp.bb.max);
    o.b(rp.on_ground);
    o.b(rp.sprinting);
    o.n(rp.hurt_time);
    o.b(rp.swing_in_progress);
    o.n(rp.swing_progress_int);
    o.n(rp.yaw);
    o.n(rp.pitch);
    o.n(rp.ticks_since_hurt);
    o.n(rp.ticks_since_jump);
    o.b(rp.using);
    o.n(rp.item as u8);
    o.n(rp.health);
    o.b(rp.fx);
    o.b(c.pick(world, ccfg).entity);
    assert_eq!(o.0.len() + 1, VIEW_HEAD);
    o.n(c.thrown.len() as u32);
    for e in &c.thrown {
        o.n(e.id);
        o.n(e.kind as u8);
        o.b(e.mine);
        if e.kind == Kind::Hook {
            // Of a hook only where the server has it counts (`Agent::tool_obs`).
            o.v(e.server_side());
            o.v([0.0; 3]);
        } else {
            o.v(e.pos);
            o.v(e.motion);
        }
        o.n(e.age);
        o.n(e.air);
        o.v(e.server_side());
    }
    let t = lp.tool;
    o.n(t.item as u8);
    o.n(t.slot);
    o.n(t.n);
    o.n(t.wear);
    o.0
}

/// The numbers at the end of `view`, after the pearls and potions: the
/// tool of the kit, its slot, how many and its wear.
pub const VIEW_TAIL: usize = 4;

/// The numbers of `view` before the pearls and potions in flight (the last
/// of them is how many those are), and the numbers of each of those.
pub const VIEW_HEAD: usize = 101;
pub const VIEW_THROWN: usize = 14;

/// What the numbers of `view` are, for a message about one of them.
pub fn view_name(k: usize, len: usize) -> String {
    const HEAD: [&str; VIEW_HEAD] = [
        "x", "y", "z", "motion x", "motion y", "motion z", "on ground", "at a wall", "fall distance", "yaw", "pitch",
        "sprinting", "jump ticks", "speed attribute", "sprint told", "hurt time",
        "knock x", "knock y", "knock z", "ticks since knock", "ticks since swing", "ticks since hit", "ticks since hurt", "ticks since moved",
        "using an item", "click delay", "use flag", "slot in hand", "pearls", "potions", "drinks", "bottles", "steak",
        "potions seen", "potions pending", "food", "saturation", "ticks of speed", "health",
        "drinking or eating", "slot in use", "ticks of use", "inventory open",
        "row 0", "row 1", "row 2", "row 3", "row 4", "row 5", "row 6", "row 7", "row 8", "row 9", "row 10", "row 11", "row 12", "row 13",
        "row 14", "row 15", "row 16", "row 17", "row 18", "row 19", "row 20", "row 21", "row 22", "row 23", "row 24", "row 25", "row 26",
        "other x", "other y", "other z", "other before x", "other before y", "other before z",
        "other heading x", "other heading y", "other heading z", "other steps",
        "other box min x", "other box min y", "other box min z", "other box max x", "other box max y", "other box max z",
        "other on ground", "other sprinting", "other hurt time", "other swinging", "other swing", "other yaw", "other pitch",
        "other ticks since hurt", "other ticks since moved far", "other using", "other holds", "other health", "other particles",
        "on target", "thrown",
    ];
    const THROWN: [&str; VIEW_THROWN] =
        ["number", "kind", "own", "x", "y", "z", "motion x", "motion y", "motion z", "age", "air", "server x", "server y", "server z"];
    if k < VIEW_HEAD {
        HEAD[k].to_string()
    } else if k >= len - VIEW_TAIL {
        ["tool", "tool slot", "tool count", "tool wear"][k + VIEW_TAIL - len].to_string()
    } else {
        format!("thrown {} {}", (k - VIEW_HEAD) / VIEW_THROWN, THROWN[(k - VIEW_HEAD) % VIEW_THROWN])
    }
}
