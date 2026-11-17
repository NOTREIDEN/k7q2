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
use crate::client::{pearl_landing, pearl_preview, Client, ClientCfg, ClientThrown, Input, TickOut, INV_SHUT};
use crate::mth::{self, Rng};
use crate::net::{refill_click, shift_click, Item, Kind, SLOT_FOOD, SLOT_PEARL, SLOT_SWORD, USE_TICKS, WINDOW_BAR};
use crate::server::{splash_heal, throw_heading, MAX_HEALTH, POTION_HEAL};
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
/// `Agent::pearl_keys` and `Agent::potion_keys`). The pearl does nothing while
/// the server's cooldown runs or when no pearl is left, the potion when no
/// potion is left. A drink or a meal is asked for once: the number key and
/// the right button, which then stays down until the server says it is
/// done or the player stops (see `Agent::consume_keys`). A potion from the
/// inventory is the inventory key, a shift click on the potion on the next
/// tick, and the key again on the third; asked for on every tick, another
/// potion comes down on each (see `Agent::refill_keys`).
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

/// How many of its last positions an agent keeps.
const TRAIL: usize = 16;

/// What an agent is told about the duel it plays, the same for both
/// players and for all of the duel.
#[derive(Clone, Copy, Debug)]
pub struct Duel {
    /// Half the side of the arena, which is centred on the origin, and the
    /// height of its floor.
    pub arena_half: f64,
    pub floor_y: f64,
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
}

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
            since_throw: 1000,
            trail: [feet; TRAIL],
            lag: (ping_ms[0] / 50.0).floor() as i32 + 1,
            fate: [None; 2],
            pearl_fate: [None; 2],
            consume: 0,
            asked_bar: 0,
            was_consuming: false,
            opp_use_ticks: 0,
            refill: 0,
            refill_kind: Item::Potion,
        }
    }

    /// The keys and buttons of the tick to come for what the policy chose
    /// (one number per control, see `ACT_DIMS`). An open inventory takes
    /// the whole tick; otherwise a drink or a meal, a potion and a pearl
    /// are laid over the plain keys in that order, and the click rate comes
    /// last.
    pub fn keys(&mut self, c: &Client, duel: &Duel, action: &[i32], rng: &mut Rng) -> Input {
        let mut inp = decode_action(action);
        let bar = if action.len() > 9 { action[9] } else { 0 };
        if !self.refill_keys(c, bar, &mut inp) {
            self.consume_keys(c, duel, bar, &mut inp);
            self.potion_keys(c, bar == 1, &mut inp);
            self.pearl_keys(c, action.len() > 8 && action[8] != 0, &mut inp);
            self.limit_clicks(&mut inp, rng);
        }
        inp
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
        if out.threw {
            // The server starts its wait when the throw reaches it, a
            // little later than this; two ticks more cover the difference.
            self.pearl_wait = duel.pearl_cooldown + 2;
        }
        self.prev_inp = *inp;
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
            && lp.held == SLOT_SWORD
    }

    /// Can the player throw a potion now? With the sword in hand, or on
    /// the tick after another potion.
    fn potion_ready(&self, c: &Client) -> bool {
        let lp = &c.lp;
        lp.potions != 0
            && self.pearl_stage == 0
            && self.consume == 0
            && match self.potion_stage {
                0 => lp.held == SLOT_SWORD,
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
        self.asked_bar = choice.clamp(0, 6) as u8;
        let lp = &c.lp;
        if self.consume == 0 {
            let slot = match choice {
                2 if lp.drinks != 0 && self.drink_due(c, duel) => lp.drinks.trailing_zeros() as u8,
                3 if lp.steak > 0 && lp.food < 20 => SLOT_FOOD,
                _ => return,
            };
            if lp.held != SLOT_SWORD || self.pearl_stage != 0 || self.potion_stage != 0 {
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
            if lp.held != SLOT_SWORD || self.pearl_stage != 0 || self.potion_stage != 0 || self.consume != 0 {
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
        self.asked_bar = choice.clamp(0, 6) as u8;
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
    /// last one. It comes before the pearl when both are asked for.
    fn potion_keys(&mut self, c: &Client, want: bool, inp: &mut Input) {
        self.asked_potion = want;
        match self.potion_stage {
            0 => {
                if want && self.potion_ready(c) {
                    inp.slot_key = c.lp.potions.trailing_zeros() as u8 + 1;
                    inp.attack = false;
                    if self.prev_inp.use_item {
                        inp.use_item = false;
                        self.potion_stage = 1;
                    } else {
                        inp.use_item = true;
                        self.potion_stage = 2;
                    }
                }
            }
            1 => {
                inp.use_item = true;
                inp.attack = false;
                self.potion_stage = 2;
            }
            _ => {
                if want && self.potion_ready(c) {
                    inp.slot_key = c.lp.potions.trailing_zeros() as u8 + 1;
                    inp.attack = false;
                    inp.use_item = true;
                    inp.use_again = self.prev_inp.use_item;
                } else {
                    inp.slot_key = SLOT_SWORD + 1;
                    inp.use_item = false;
                    self.potion_stage = 0;
                }
            }
        }
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
    /// come, which is when it is told what it sees. Its own potion, seen
    /// for the first time: the ticks since it threw it are how far behind
    /// the server's answers are. And what it foresees for the potions and
    /// pearls that break on their next tick is kept.
    pub fn before_tick(&mut self, c: &Client, world: &World) {
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
    }

    /// What the policy is shown: what the agent can know from its own
    /// client and of the duel. `combo` is the hits in a row this player
    /// has landed, and the other.
    pub fn write_obs(&self, c: &Client, world: &World, ccfg: &ClientCfg, duel: &Duel, combo: [u32; 2], o: &mut [f32]) {
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
        o[13] = clip(b.pos[0] / duel.arena_half, 1.5);
        o[14] = clip(b.pos[2] / duel.arena_half, 1.5);
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
        for v in o[77..OBS_DIM].iter_mut() {
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
            let (land, ticks) = pearl_preview(world, b.pos, lp.l.yaw, lp.l.pitch);
            let (lf, lr) = local(yaw, land[0] - b.pos[0], land[2] - b.pos[2]);
            o[92] = clip(lf.hypot(lr) / 16.0, 4.0);
            o[93] = (ticks.min(60)) as f32 / 30.0;
            o[94] = clip((lf - rf) / 8.0, 4.0);
            o[95] = clip((lr - rr) / 8.0, 4.0);
        }
        // Health: whether hits cost any in this duel, this player's and the
        // opponent's as this client knows them, and the health a plain hit
        // costs with this armour and these swords.
        let real = duel.real_health;
        let hh = duel.hit_health;
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
            let m = [dir[0] * v, dir[1] * v, dir[2] * v];
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
fn potion_outcome(world: &World, mut pos: V3, mut m: V3, mut age: i32, thrower: usize, feet: impl Fn(usize, i32) -> V3) -> (V3, i32, [f32; 2]) {
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
