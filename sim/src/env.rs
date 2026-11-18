//! The training environment around a duel: action decoding, observations,
//! rewards, episode resets with randomised conditions, and a batch of
//! environments stepped together.
//!
//! Stepping is per client tick. In self-play the two agents take turns
//! (agent 0 ticks, then agent 1, 50 ms per round), because their clocks are
//! out of phase and each must see what reached it before its own tick. Each
//! call returns the observation of the agent that acts next, together with
//! the reward that agent earned since its previous action.
use crate::aabb::{dist, V3};
use crate::agent::{aim_error, clip, local, Agent, Duel};
pub use crate::agent::{decode_action, ACT_DIMS, ACT_HEADS, BAR_TURN, BAR_TURNS, FLICK_BINS, HURT_AT, HURT_WINDOW, OBS_DIM, PITCH_BINS, YAW_BINS};
use crate::bot::{self, BotParams, BotState};
use crate::client::{Client, ClientCfg, Edge, Input, TickOut, INV_SHUT, MAX_BACKTRACK};
use crate::mth::Rng;
use crate::net::{potion_mask, Item, Kind, Rows, SLOT_PEARL, SLOT_SWORD, USE_TICKS};
use crate::server::{
    feather_points, prot_points, Food, KbPlugin, KbProfile, ServerCfg, DRINK_SHOWN, FOR_GOOD, MAX_HEALTH, PEARL_DAMAGE,
    POTION_HEAL,
};
use crate::sim::{Match, NetCfg, Spawn, TICK_US};
use crate::world::World;

/// How many of the points a potion did not offer its thrower `w_miss` counts.
pub const MISS_POINTS: f32 = 2.0;
/// How many ticks of looking away `w_pot_time` counts before a potion is
/// thrown, and as many after it.
pub const POT_TIME_TICKS: i32 = 10;
/// The food from which the game heals a player by itself, and the food a
/// player needs more than to sprint.
pub const FOOD_TO_HEAL: i32 = 18;
pub const FOOD_TO_SPRINT: i32 = 6;
/// The degrees of a turn undone at once that `w_shake` counts in full, and
/// how many ticks the view may have stood still in between.
pub const SHAKE_DEG: f32 = 6.0;
pub const SHAKE_TICKS: i32 = 2;
pub const PRIV_DIM: usize = 71;
pub const STATS_DIM: usize = 101;
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
    /// hit already in the run (at most `combo_cap`, five unless set), and
    /// the same amount taken from the player on the receiving end. Zero
    /// leaves every hit worth the same.
    pub w_combo: f32,
    pub combo_cap: f64,
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
    /// Further cost of the part of an own potion that never reached its
    /// thrower, whether it was needed or not: a potion that breaks a step
    /// away offers seven of its eight points, one caught in the air all of
    /// them. It counts the first `MISS_POINTS` points only, so it tells a
    /// near miss from a perfect throw without making a potion that was
    /// knocked out of reach cost more than the potion. Nothing can be
    /// earned with it: the best a throw can do is to lose none.
    pub w_miss: f32,
    /// Cost of every tick the crosshair is off the opponent for a potion,
    /// in plain hits: the ticks in a row right before the throw and the
    /// ticks after it until the crosshair is back, `POT_TIME_TICKS` of each
    /// at most, so that a potion thrown on the run costs a fixed two
    /// seconds' worth and never more than it gives. A throw that takes one
    /// tick of looking down costs a tenth of one that takes ten. Nothing
    /// can be earned with it either.
    pub w_pot_time: f32,
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
    /// Share of the duels in which the scripted bot is a runner (see
    /// `BotParams::runner`).
    pub bot_runner: f64,
    /// Share of the duels without a runner in which the scripted bot
    /// presses (see `BotParams::presser`).
    pub bot_presser: f64,
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
    /// 16, 32 and 64 are the extreme ones, from close by, with the first
    /// player hurt and carrying potions that have to be thrown with the
    /// other one there. 16, against scripted bots only: the bot takes next
    /// to no knockback, has Speed III to X and runs rings round it
    /// (`BotParams::circler`). 32: hits hardly move either player, so
    /// neither can be got off the other. 64: the first player stands in a
    /// corner of the arena with the other one in its way out, who takes
    /// less knockback than the game gives; most of the time it has no
    /// pearl to leave with. 128: the first player, hurt as in those, has
    /// its back to the other one, who is a few blocks behind and coming:
    /// what it throws lands away from the opponent, and its view has to
    /// come round again. 256, with nobody hurt: the other one is next to
    /// it, to a side or behind, as after a pearl thrown there, and the
    /// view has to get there.
    pub lesson_prob: f64,
    pub lessons: u32,
    /// Of the lessons, the share that is one of the extreme ones. Their
    /// duels are short, so that as lessons among others they get little
    /// of the playing time. Negative: each as likely as any other.
    pub extreme_share: f64,
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
    /// Cost of a turn of the view that undoes the turn before it, sideways
    /// and up or down each on its own: the degrees taken back, `SHAKE_DEG`
    /// of them at most, as a share of that many. A crosshair that goes
    /// left and right again tick after tick pays on every tick; one that
    /// follows the opponent pays once when the opponent turns round, and a
    /// look at the ground and back up for a potion pays once. Nothing can
    /// be earned with it.
    pub w_shake: f32,
    /// Share of the duels with real health in which a healing potion that
    /// did not offer its thrower all eight points gives it only the share
    /// `pot_strict` of what the game would: a lesson in throwing them so
    /// that they do. The players are not told which duels these are.
    pub pot_strict_prob: f64,
    pub pot_strict: f64,
    /// What the thrower of a healing potion pays, on top of what the
    /// opponent gains, for every point of health its potion gives the
    /// opponent: a potion thrown towards an opponent that is close heals
    /// both.
    pub w_gift: f32,
    /// Cost per tick of carrying steak with less food than the game asks
    /// for to heal a player by itself (18): half of it as soon as that is
    /// so, all of it at the food that ends the sprint.
    pub w_hungry: f32,
    /// Cost per tick of being without Speed while carrying a potion of it,
    /// in the hotbar or in the inventory.
    pub w_slow: f32,
    /// Cost per tick of having the inventory open with the opponent closer
    /// than `inv_near_dist` blocks (across the ground): potions are got
    /// down when the opponent cannot be at them, not in the middle of a
    /// trade, where the ticks of the screen are ticks without a hit, a
    /// block or a sprint.
    pub w_inv_near: f32,
    pub inv_near_dist: f64,
    /// What it costs, in hits, to get a potion of Speed down from the
    /// inventory that is not needed: with another one already in the
    /// hotbar, or with more Speed left than `drink_window_s`. The slot it
    /// takes could hold a healing potion, and the click a hit.
    pub w_speed_refill: f32,
    /// Share of the duels against scripted bots fought against the
    /// hardest one: level 4 that presses and pots as the best players do
    /// (`BotParams::perfect_pot`), on or near the server (0 to 60 ms), with
    /// 3.0 to 3.4 blocks of reach and 0 to 40 % of the knockback.
    pub insane_prob: f64,
    /// Paid, in hits, to the thrower of a pearl that landed when the next
    /// hit within two seconds is its own, and taken from it when it is the
    /// other player's: a pearl is for getting at the other one (at its
    /// potion, out of its combo, on with one's own), not for its own sake.
    pub w_pearl_next: f32,
    /// Share of duels fought on terrain (`terrain::build`): steps, slabs,
    /// pillars, walls, platforms, houses, pits and drops into the void.
    pub terrain_prob: f64,
    /// How much terrain such a duel has, from 0 to 1.
    pub terrain_amount: f64,
    /// Of the duels on terrain, the share with a strip open to the void
    /// across the way from one player to the other (`terrain::build`).
    pub terrain_gap_prob: f64,
    /// Health in the shaping potential (`Env::potential`): `w_hp_pot`
    /// times `-exp(-h / hp_pot_scale)` of the player's own health, less the
    /// same of the opponent's. A point of health counts the more the lower
    /// the health it comes from or goes to, so a potion with little health
    /// left, or a hit kept off then, pays at once what it is worth to the
    /// win that comes seconds later; as a potential it leaves which play is
    /// best as it was.
    pub w_hp_pot: f32,
    pub hp_pot_scale: f32,
    /// Share of duels in which the second player's Strength comes and goes
    /// out of nothing as far as the first can tell (a drink it did not see,
    /// a command, an effect running out): first somewhere in the first two
    /// thirds of the duel, then again every 5 to 25 seconds, each time to a
    /// level drawn evenly from none and I to `opp_strength_max`, so that its
    /// hits cost 2.3, 3.6 or 4.9 times as much, or the plain hit again.
    pub opp_strength_prob: f64,
    pub opp_strength_max: f64,
    /// Share of duels in which the first player is ganked now and then:
    /// one to `gank_max` players it cannot see, each on a side of its own,
    /// each hitting it every 10 to 16 ticks for 2 to 6 seconds with the
    /// sword of the kit and no Strength, Strength I or II, the first time
    /// within the first half of the duel and again 10 to 40 seconds after
    /// each. The game's invulnerable ticks decide which of their hits count.
    pub gank_prob: f64,
    pub gank_max: f64,
    /// What dying costs on top of losing the duel (`w_win`): not dying is
    /// worth more than killing.
    pub w_death: f32,
    /// Taken, in hits, from the thrower of a pearl when it throws it, and
    /// given back if the other player dies within `pearl_kill_s` seconds:
    /// a pearl is for a kill, not for its own sake. It never pays more
    /// than it costs.
    pub w_pearl_cost: f32,
    pub pearl_kill_s: f64,
    /// Share of duels in which the first player loses `drop_min` to
    /// `drop_max` health at once (past the armour, with a knockback) at a
    /// moment drawn in the first half of the duel and again every 15 to 45
    /// seconds: a hit with Strength or by a player it does not see. It is
    /// always left with two health at least.
    pub drop_prob: f64,
    pub drop_min: f64,
    pub drop_max: f64,
    /// Share of the duels with real health that are a survival drill for
    /// the first player: the second cannot die and takes no knockback, and
    /// `drill_gankers` more players it cannot see come at it in waves of 5
    /// to 15 seconds, 1 to 4 seconds apart, the first within 3 seconds.
    /// Each starts a few blocks away, runs at it as fast as a sprinting
    /// player with Speed II and hits it every 10 to 16 ticks while within
    /// reach, all of a wave with the same Strength, drawn evenly from none
    /// to level `drill_strength_max`. It can only be got away from: a
    /// pearl or a run puts it out of their reach for a while. Only where
    /// the second player does not learn: against the bot, and in self-play
    /// only with `drill_self` (for duels against earlier versions).
    pub drill_prob: f64,
    pub drill_self: bool,
    pub drill_gankers: f64,
    pub drill_strength_max: f64,
    /// How long a drill duel lasts at most, on average (a quarter more or
    /// less): long enough for the potions it has to see it through if it
    /// pots well and gets away now and then.
    pub drill_seconds: f64,
    /// Share of duels with cobwebs (`terrain::add_webs`): `web_amount`
    /// from 0 to 1 says how many patches, and in `web_trap_prob` of them
    /// the first player starts caught in one.
    pub web_prob: f64,
    pub web_amount: f64,
    pub web_trap_prob: f64,
    /// Share of new duels that are a retry of one the first player lost,
    /// from a few seconds before its death (as GT Sophy trained on its
    /// mistakes): the same moment of the same duel, played again. Each lost
    /// moment is kept for two retries; with none kept the duel is new.
    pub retry_prob: f64,
    /// In self-play, the share of duels in which player 1 goes for the
    /// other player's potions: while that player holds a potion, has one in
    /// the air or is under 12 health, it runs straight at it (`steal_keys`).
    /// A potion thrown at its own feet in front of such a player heals both.
    /// Its ticks played that way are not its policy's (`Env::overridden`):
    /// an earlier version in the league is not trained on anyway, and in
    /// plain self-play those ticks are left out of player 1's training.
    pub league_steal: f64,
    /// Share of duels on a server whose throws take on the thrower's motion
    /// (`ServerCfg::throw_carry`).
    pub throw_carry_prob: f64,
    /// Above 0, the agents' teachers say what they would have chosen
    /// around a potion thrown with a turn (`agent::Teacher`,
    /// `Env::teach`), for training to imitate.
    pub teach: f64,
    /// Share of the players that start with potions of Speed at hand whose
    /// potions of it are in the inventory instead, as most players carry
    /// them: to drink one it has to be brought down first.
    pub drinks_stowed: f64,
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
            combo_cap: 5.0,
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
            w_miss: 0.0,
            w_pot_time: 0.0,
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
            bot_runner: 0.0,
            bot_presser: 0.0,
            extreme_share: -1.0,
            w_stock: 0.0,
            lesson_prob: 0.0,
            lessons: 15,
            lesson_dist: [10.0, 20.0],
            drink_window_s: 15.0,
            w_shake: 0.0,
            pot_strict_prob: 0.0,
            pot_strict: 0.25,
            w_gift: 0.0,
            w_hungry: 0.0,
            w_slow: 0.0,
            w_inv_near: 0.0,
            inv_near_dist: 4.5,
            w_speed_refill: 0.0,
            insane_prob: 0.0,
            w_pearl_next: 0.0,
            terrain_prob: 0.0,
            terrain_amount: 0.6,
            terrain_gap_prob: 0.0,
            w_hp_pot: 0.0,
            hp_pot_scale: 4.0,
            opp_strength_prob: 0.0,
            opp_strength_max: 3.0,
            gank_prob: 0.0,
            gank_max: 1.0,
            w_death: 0.0,
            w_pearl_cost: 0.0,
            pearl_kill_s: 10.0,
            drop_prob: 0.0,
            drop_min: 8.0,
            drop_max: 14.0,
            drill_prob: 0.0,
            drill_self: false,
            drill_gankers: 5.0,
            drill_strength_max: 2.0,
            drill_seconds: 40.0,
            web_prob: 0.0,
            web_amount: 0.6,
            web_trap_prob: 0.3,
            retry_prob: 0.0,
            league_steal: 0.0,
            throw_carry_prob: 0.0,
            teach: 0.0,
            drinks_stowed: 0.0,
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
            110 => self.w_miss = v as f32,
            111 => self.w_pot_time = v as f32,
            112 => self.bot_runner = v,
            121 => self.bot_presser = v,
            122 => self.w_inv_near = v as f32,
            123 => self.inv_near_dist = v,
            124 => self.league_steal = v,
            125 => self.throw_carry_prob = v,
            126 => self.teach = v,
            127 => self.w_speed_refill = v as f32,
            128 => self.insane_prob = v,
            129 => self.w_pearl_next = v as f32,
            130 => self.terrain_prob = v,
            131 => self.terrain_amount = v,
            133 => self.terrain_gap_prob = v,
            134 => self.w_hp_pot = v as f32,
            135 => self.hp_pot_scale = v as f32,
            136 => self.opp_strength_prob = v,
            137 => self.opp_strength_max = v,
            138 => self.gank_prob = v,
            139 => self.w_death = v as f32,
            140 => self.gank_max = v,
            141 => self.w_pearl_cost = v as f32,
            142 => self.pearl_kill_s = v,
            143 => self.drop_prob = v,
            144 => self.drop_min = v,
            145 => self.drop_max = v,
            146 => self.drill_prob = v,
            147 => self.drill_self = v != 0.0,
            148 => self.drill_gankers = v,
            149 => self.drill_strength_max = v,
            150 => self.drill_seconds = v,
            151 => self.combo_cap = v,
            152 => self.web_prob = v,
            153 => self.web_amount = v,
            154 => self.web_trap_prob = v,
            132 => self.retry_prob = v,
            113 => self.extreme_share = v,
            114 => self.w_shake = v as f32,
            115 => self.pot_strict_prob = v,
            116 => self.pot_strict = v,
            117 => self.w_gift = v as f32,
            118 => self.w_hungry = v as f32,
            119 => self.w_slow = v as f32,
            120 => self.drinks_stowed = v,
            _ => return false,
        }
        true
    }
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
    /// Degrees of turning the view that the next turn undid (see
    /// `Params::w_shake`), without the limit on what one turn counts.
    pub shake: [f64; 2],
    /// Potions of Speed got down that were not needed (`w_speed_refill`),
    /// ticks of the inventory open while the player could not be hurt
    /// (the hurt time after a hit), and pearls thrown with a reason: the
    /// other player has a healing potion in hand or in the air, or has hit
    /// this one twice or more in a row, or this one has, with the other
    /// out of reach.
    pub speed_waste: [f64; 2],
    pub inv_hurt: [f64; 2],
    pub pearl_sense: [f64; 2],
    /// Deaths in the void, and ticks played on terrain (`terrain_prob`).
    pub void_falls: [f64; 2],
    pub terrain_ticks: [f64; 2],
    /// Episodes that were a retry of a lost moment.
    pub retries: f64,
    /// Episodes that were a survival drill, and those of them in which the
    /// first player died.
    pub drills: f64,
    pub drill_deaths: f64,
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
            self.shake[a] += o.shake[a];
            self.speed_waste[a] += o.speed_waste[a];
            self.inv_hurt[a] += o.inv_hurt[a];
            self.pearl_sense[a] += o.pearl_sense[a];
            self.void_falls[a] += o.void_falls[a];
            self.terrain_ticks[a] += o.terrain_ticks[a];
        }
        self.retries += o.retries;
        self.drills += o.drills;
        self.drill_deaths += o.drill_deaths;
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
            self.shake[0],
            self.shake[1],
            self.speed_waste[0],
            self.speed_waste[1],
            self.inv_hurt[0],
            self.inv_hurt[1],
            self.pearl_sense[0],
            self.pearl_sense[1],
            self.void_falls[0],
            self.void_falls[1],
            self.terrain_ticks[0],
            self.terrain_ticks[1],
            self.retries,
            self.drills,
            self.drill_deaths,
        ];
        for (o, x) in out.iter_mut().zip(v.iter()) {
            *o = *x;
        }
    }
}

/// A moment of a duel, to play it again from there (`Params::retry_prob`).
#[derive(Clone)]
struct Moment {
    m: Match,
    pending: usize,
    /// The tick of the duel it was kept on, and the ganks under way then.
    at: i32,
    gank: Gank,
    bot: BotParams,
    bot_state: BotState,
    ag: [Agent; 2],
    combo: [u32; 2],
    ping_ms: [f64; 2],
    landed: Option<(usize, i32)>,
    pearl_kill: [i32; 2],
    since_block: [i32; 2],
    pot_since: [i32; 2],
    pot_back: [bool; 2],
    off_run: [i32; 2],
    steal: bool,
    was_open: [bool; 2],
    view_was: [[f32; 2]; 2],
    turn_last: [[f32; 2]; 2],
    turn_age: [[i32; 2]; 2],
    lesson: u8,
}

/// The most players that gank the first player at once.
const GANK_MAX: usize = 6;
/// Players of a drill: blocks a tick they run (a sprint with Speed II),
/// how close they come, and their reach.
const DRILL_SPEED: f64 = 0.39;
const DRILL_CLOSE: f64 = 2.0;
const DRILL_REACH: f64 = 3.0;
/// How many moments of the duel in progress are kept, one a second.
const MOMENTS: usize = 5;
/// How many lost moments are kept for retries, and how often each is played.
const LOSSES: usize = 4;
const RETRIES: u8 = 2;

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
    /// What plays each player: for the bot's seat, only its click rate and
    /// what an agent remembers of a tick are used.
    ag: [Agent; 2],
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
    /// The thrower of the last pearl that landed, and the server ticks left
    /// in which the next hit is counted for or against that pearl.
    landed: Option<(usize, i32)>,
    /// Server ticks left in which a death of the other player gives each
    /// player back what its last pearl cost (`w_pearl_cost`).
    pearl_kill: [i32; 2],
    /// Own ticks since each player last had its sword up; 0 while it is.
    since_block: [i32; 2],
    /// Ticks since each player's last potion throw, whether its crosshair
    /// has been back on the other player since, and for how many ticks in
    /// a row it has been off it.
    pot_since: [i32; 2],
    pot_back: [bool; 2],
    off_run: [i32; 2],
    /// Whether player 1 goes for player 0's potions in this duel (see
    /// `Params::league_steal`), and whether its keys were laid over that
    /// way on the last tick made.
    steal: bool,
    pub overridden: bool,
    /// What the teacher of the agent that made the last tick would have
    /// chosen on it, -1 for each control it has no opinion on.
    pub teach: [i32; ACT_HEADS],
    /// Whether each player's inventory was open on its last tick.
    was_open: [bool; 2],
    /// Each player's view after its last tick, its last turn sideways and
    /// up or down, and the ticks it has not turned that way since.
    view_was: [[f32; 2]; 2],
    turn_last: [[f32; 2]; 2],
    turn_age: [[i32; 2]; 2],
    /// The lesson this duel started as (0: none).
    pub lesson: u8,
    /// A player has died: the duel ends when both have made their tick.
    over: bool,
    /// The last seconds of the duel in progress, oldest first, and moments
    /// from before the first player's deaths with the retries left of each.
    moments: std::collections::VecDeque<Moment>,
    losses: Vec<(Moment, u8)>,
    /// Tick of the duel at which the second player's Strength changes next;
    /// -1 for never (see `Params::opp_strength_prob`).
    strength_at: i32,
    /// Tick of the duel of the next sudden fall of the first player's
    /// health, -1 for none (see `Params::drop_prob`).
    drop_next: i32,
    /// Ganks (see `Params::gank_prob`): the tick the next one starts, -1
    /// for none; the tick the one under way ends, how many hit in it, and
    /// for each its next hit, the damage of its hits and the direction they
    /// come from.
    gank: Gank,
}

/// The ganks of the first player (see `Params::gank_prob` and
/// `Params::drill_prob`): the tick the next one starts, -1 for none; the
/// tick the one under way ends, how many hit in it, and for each its next
/// hit, the damage of its hits and the direction they come from. In a
/// drill each one also has a place, which the direction is taken from.
#[derive(Clone, Copy, Debug)]
struct Gank {
    drill: bool,
    next: i32,
    end: i32,
    n: usize,
    hit: [i32; GANK_MAX],
    damage: [f32; GANK_MAX],
    dir: [[f64; 2]; GANK_MAX],
    pos: [[f64; 2]; GANK_MAX],
}

impl Gank {
    const NONE: Gank = Gank { drill: false, next: -1, end: 0, n: 0, hit: [0; GANK_MAX], damage: [0.0; GANK_MAX], dir: [[1.0, 0.0]; GANK_MAX], pos: [[0.0; 2]; GANK_MAX] };

    /// The same ganks with the ticks counted from `at` on.
    fn shifted(mut self, at: i32) -> Gank {
        if self.next >= 0 {
            self.next -= at;
        }
        self.end -= at;
        for h in self.hit.iter_mut() {
            *h -= at;
        }
        self
    }
}

/// The keys of a player that goes for its opponent's potions, laid over
/// what its policy asked for: while the opponent holds a potion, has one
/// of its own in the air or is under 12 health, the view turned onto it
/// (at most 60 degrees a tick), W with sprint, no strafe, no right button,
/// and jumps while it is out of reach. Clicks stay the policy's.
fn steal_keys(c: &Client, deg_per_count: f64, inp: &mut Input) -> bool {
    let (lp, rp) = (&c.lp, &c.rp);
    let potting = rp.item == Item::Potion || c.thrown.iter().any(|e| e.kind == Kind::Potion && !e.mine);
    if lp.inv_open || !(potting || rp.health <= 12.0) {
        return false;
    }
    let (yaw_to, pitch_to) = bot::angles_to(lp.eye(), [rp.pos[0], rp.pos[1] + 1.2, rp.pos[2]]);
    let dyaw = crate::mth::wrap_deg(yaw_to - lp.l.yaw as f64).clamp(-60.0, 60.0);
    let dpitch = (pitch_to - lp.l.pitch as f64).clamp(-60.0, 60.0);
    inp.dx = (dyaw / deg_per_count).round() as i32;
    inp.dy = (dpitch / deg_per_count).round() as i32;
    inp.forward = 1;
    inp.strafe = 0;
    inp.sprint_key = true;
    inp.use_item = false;
    inp.use_again = false;
    let hdist = (rp.pos[0] - lp.l.body.pos[0]).hypot(rp.pos[2] - lp.l.body.pos[2]);
    inp.jump = hdist > 3.2 && lp.l.body.on_ground;
    true
}

pub fn new_match(p: &Params, rng: &mut Rng, mode: Mode) -> (Match, [f64; 2], BotParams) {
    let (m, ping, bot, _) = new_duel(p, rng, mode);
    (m, ping, bot)
}

/// A duel with newly drawn conditions, and the lesson it starts as (0:
/// none; see `Params::lessons`).
pub fn new_duel(p: &Params, rng: &mut Rng, mode: Mode) -> (Match, [f64; 2], BotParams, u8) {
    let mut world = World::arena(p.floor_y, p.arena_half, p.walls);
    let lim = (p.arena_half - 2.0).max(1.0);
    // Drawn only when lessons are on, so that duels without them come out
    // as they always did.
    let offered = if mode == Mode::Bot { 511 } else { 495 };
    let lesson = if p.lesson_prob > 0.0 && p.lessons & offered != 0 && rng.chance(p.lesson_prob) {
        let mut on: Vec<u8> = (0..9u8).filter(|k| (p.lessons & offered) >> k & 1 != 0).collect();
        if p.extreme_share >= 0.0 && on.iter().any(|&k| k >= 4) && on.iter().any(|&k| k < 4) {
            let extreme = rng.chance(p.extreme_share);
            on.retain(|&k| (k >= 4) == extreme);
        }
        1 + on[(rng.next_u64() % on.len() as u64) as usize]
    } else {
        0
    };
    // A duel against the hardest bot there is (`Params::insane_prob`):
    // drawn only when asked for.
    let insane = mode == Mode::Bot && lesson == 0 && p.insane_prob > 0.0 && rng.chance(p.insane_prob);
    let (d_lo, d_hi) = match lesson {
        0 => (p.start_dist_min, p.start_dist_max),
        5 | 6 => (2.5, 5.0),
        8 => (3.5, 7.0),
        9 => (2.0, 4.5),
        _ => (p.lesson_dist[0], p.lesson_dist[1]),
    };
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
    if lesson == 7 {
        // In a corner, as near the two walls as a hit would push it, with
        // the other player on the side the arena is on.
        let h = p.arena_half;
        let side = |rng: &mut Rng| if rng.chance(0.5) { 1.0 } else { -1.0 };
        let (sx, sz) = (side(rng), side(rng));
        a = [sx * (h - rng.range(0.4, 2.0)), p.floor_y, sz * (h - rng.range(0.4, 2.0))];
        let d = rng.range(2.0, 4.0);
        let ang = (-sz).atan2(-sx) + rng.range(-0.6, 0.6);
        b = [a[0] + d * ang.cos(), p.floor_y, a[2] + d * ang.sin()];
    }
    let face = |from: V3, to: V3, rng: &mut Rng| -> f32 {
        let (yaw, _) = bot::angles_to(from, to);
        (yaw + rng.range(-p.start_yaw_noise, p.start_yaw_noise)) as f32
    };
    let yaw_a = face(a, b, rng);
    let pitch_a = rng.range(-8.0, 8.0) as f32;
    let yaw_b = face(b, a, rng);
    let pitch = [pitch_a, rng.range(-8.0, 8.0) as f32];
    // The lessons that start with the view turned away: drawn only for
    // them, and from where the other player is, whatever the views of a
    // duel start as. The other one looks at the first, near enough.
    let (yaw_a, yaw_b) = if lesson == 8 || lesson == 9 {
        let off = if lesson == 8 {
            180.0 + rng.range(-25.0, 25.0)
        } else {
            rng.range(70.0, 180.0) * if rng.chance(0.5) { 1.0 } else { -1.0 }
        };
        let to_b = bot::angles_to(a, b).0;
        let to_a = bot::angles_to(b, a).0 + rng.range(-15.0, 15.0);
        (crate::mth::wrap_deg(to_b + off) as f32, to_a as f32)
    } else {
        (yaw_a, yaw_b)
    };
    // Terrain, drawn only when asked for.
    if p.terrain_prob > 0.0 && rng.chance(p.terrain_prob) {
        // Drawn only when asked for, so that terrain comes out as it did.
        let gap = p.terrain_gap_prob > 0.0 && rng.chance(p.terrain_gap_prob);
        crate::terrain::build(&mut world, rng, &[a, b], p.terrain_amount, gap);
    }
    if p.web_prob > 0.0 && rng.chance(p.web_prob) {
        let trap = p.web_trap_prob > 0.0 && rng.chance(p.web_trap_prob);
        crate::terrain::add_webs(&mut world, rng, &[a, b], p.web_amount, trap);
    }
    let spawns = [Spawn { pos: a, yaw: yaw_a, pitch: pitch[0] }, Spawn { pos: b, yaw: yaw_b, pitch: pitch[1] }];
    let mut ping = [0.0; 2];
    let mut net = NetCfg { up_us: [0; 2], down_us: [0; 2], jitter_us: [0; 2] };
    for i in 0..2 {
        let (lo, hi) = if i == 0 && p.agent_ping_min_ms >= 0.0 {
            (p.agent_ping_min_ms, p.agent_ping_max_ms)
        } else if insane {
            // A bot on the server itself, or a player close to it.
            (0.0, 60.0)
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
    if lesson == 6 {
        // Hits that hardly move anybody, on the ground or in the air.
        let hg = rng.range(0.0, 0.15);
        let vg = rng.range(0.0, 0.2);
        kb = KbProfile::VANILLA;
        plugin = Some(KbPlugin {
            h_ground: hg,
            h_air: hg * rng.range(0.8, 1.1),
            h_sprint: rng.range(0.0, 0.15),
            v_ground: vg,
            v_air: vg * rng.range(0.6, 1.0),
            v_sprint: rng.range(0.0, 0.05),
            stop_sprint: rng.chance(0.7),
            y_limit: -1.0,
            v_after_limit: 0.0,
        });
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
    let stocked = (real && p.refill_prob > 0.0 && rng.chance(p.refill_prob)) || lesson == 3 || lesson == 4;
    let mut scfg = scfg;
    scfg.hunger |= hungry;
    if real && p.pot_strict_prob > 0.0 && rng.chance(p.pot_strict_prob) {
        scfg.potion_strict = p.pot_strict.clamp(0.0, 1.0) as f32;
    }
    scfg.throw_carry = p.throw_carry_prob > 0.0 && rng.chance(p.throw_carry_prob);
    // Cornered with no pearl to leave with, most of the time.
    let stuck = lesson == 7 && rng.chance(0.7);
    let seed = rng.next_u64();
    let mut m = Match::new(world, spawns, p.speed_amp, kb, scfg, ccfg, net, [0, c1, sv], seed);
    m.s.plugin = plugin;
    m.s.pearl_guard = true;
    for i in 0..2 {
        let pearls = if p.pearls_who == 0 || p.pearls_who as usize == i + 1 { pearls } else { 0 };
        let pearls = if stuck && i == 0 { 0 } else { pearls };
        m.s.p[i].pearls = pearls;
        m.c[i].lp.pearls = pearls;
        if real {
            let mut n = level(p.potions, rng).clamp(0, 7) as u8;
            // The first player is the one a lesson is for; where nobody
            // is moved by a hit, both start hurt and with potions, so that
            // the one that uses them better is the one that is left.
            let lesson = if i == 0 || lesson == 6 { lesson } else { 0 };
            let timed_i = timed && (p.speed_who == 0 || p.speed_who as usize == i + 1 || lesson == 1 || lesson == 4);
            let mut stowed = 0u8;
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
                // Drawn only when asked for, so that duels without it come
                // out as they always did.
                if cups > 0 && lesson != 1 && p.drinks_stowed > 0.0 && rng.chance(p.drinks_stowed) {
                    stowed = cups;
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
            if stocked && (p.refill_who == 0 || p.refill_who as usize == i + 1 || lesson == 3 || lesson == 4) {
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
            if stowed > 0 {
                // After whatever else the rows hold.
                let mut rows = m.s.p[i].rows;
                for cell in rows.0.iter_mut().filter(|c| **c == 0).take(stowed as usize) {
                    *cell = 2;
                }
                m.s.p[i].rows = rows;
                m.c[i].lp.rows = rows;
            }
            let hurt = (5..=8).contains(&lesson);
            if hurt {
                n = n.max(3);
            }
            m.s.p[i].potions = potion_mask(n);
            m.c[i].lp.potions = potion_mask(n);
            if lesson == 3 || hurt {
                let lo = if hurt { 4.0 } else { 6.0 };
                let h = (rng.range(lo, 12.0) * 2.0).round() as f32 / 2.0;
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
    if insane {
        m.c[1].edge = Edge { reach: rng.range(3.0, 3.4), kb_taken: rng.range(0.0, 0.4), backtrack: 0 };
    } else if rng.chance(p.opp_edge_prob) {
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
    if lesson == 5 {
        // The opponent of the extreme lesson: hits hardly move it, and it
        // is far faster than anything on foot.
        let amp = 2 + (rng.next_u64() % 8) as i8;
        m.c[1].edge.kb_taken = rng.range(0.0, 0.2);
        let (sp, lp) = (&mut m.s.p[1], &mut m.c[1].lp);
        sp.l.attr.speed_amp = amp;
        sp.speed_ticks = FOR_GOOD;
        lp.speed_amp = amp;
        lp.l.attr.speed_amp = amp;
        lp.speed_ticks = FOR_GOOD;
    }
    if lesson == 7 {
        // Hitting the other player does not clear the way out.
        m.c[1].edge.kb_taken = m.c[1].edge.kb_taken.min(rng.range(0.2, 0.7));
    }
    let mut bot = if insane {
        BotParams { presser: true, perfect_pot: true, max_turn: 180.0, aim_noise: 0.1, ..BotParams::level(4) }
    } else if mode == Mode::Bot {
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
    // Drawn only when asked for, so that duels without runners come out as
    // they always did.
    if p.bot_runner > 0.0 {
        bot.runner = rng.chance(p.bot_runner);
    }
    if p.bot_presser > 0.0 && !insane {
        bot.presser = !bot.runner && rng.chance(p.bot_presser);
    }
    if insane {
        bot.runner = false;
    }
    bot.circler = lesson == 5;
    bot.carry = m.s.cfg.throw_carry;
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
            ag: [0, 1].map(|a| Agent::new([ping[a], ping[1 - a]], 20.0, [0.0; 3])),
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
            landed: None,
            pearl_kill: [0; 2],
            since_block: [1000; 2],
            pot_since: [1000; 2],
            was_open: [false; 2],
            view_was: [[0.0; 2]; 2],
            turn_last: [[0.0; 2]; 2],
            turn_age: [[0; 2]; 2],
            lesson,
            pot_back: [true; 2],
            off_run: [0; 2],
            steal: false,
            overridden: false,
            teach: [-1; ACT_HEADS],
            over: false,
            moments: std::collections::VecDeque::new(),
            losses: Vec::new(),
            strength_at: -1,
            drop_next: -1,
            gank: Gank::NONE,
        };
        e.begin();
        e
    }

    fn moment(&self) -> Moment {
        Moment {
            m: self.m.clone(),
            pending: self.pending,
            at: self.ep_len - self.ticks_left,
            gank: self.gank,
            bot: self.bot,
            bot_state: self.bot_state,
            ag: self.ag.clone(),
            combo: self.combo,
            ping_ms: self.ping_ms,
            landed: self.landed,
            pearl_kill: self.pearl_kill,
            since_block: self.since_block,
            pot_since: self.pot_since,
            pot_back: self.pot_back,
            off_run: self.off_run,
            steal: self.steal,
            was_open: self.was_open,
            view_was: self.view_was,
            turn_last: self.turn_last,
            turn_age: self.turn_age,
            lesson: self.lesson,
        }
    }

    /// Goes back to a kept moment, as the start of a new episode.
    fn resume(&mut self, k: Moment) {
        self.m = k.m;
        self.pending = k.pending;
        self.bot = k.bot;
        self.bot_state = k.bot_state;
        self.ag = k.ag;
        self.combo = k.combo;
        self.ping_ms = k.ping_ms;
        self.landed = k.landed;
        self.pearl_kill = k.pearl_kill;
        self.since_block = k.since_block;
        self.pot_since = k.pot_since;
        self.pot_back = k.pot_back;
        self.off_run = k.off_run;
        self.steal = k.steal;
        self.was_open = k.was_open;
        self.view_was = k.view_was;
        self.turn_last = k.turn_last;
        self.turn_age = k.turn_age;
        self.lesson = k.lesson;
        let p = &self.params;
        self.ep_len = self.rng.range(p.ep_ticks_min, p.ep_ticks_max.max(p.ep_ticks_min)).round() as i32;
        self.ticks_left = self.ep_len;
        self.rew_acc = [0.0; 2];
        self.max_combo = self.combo;
        self.ep = Stats::default();
        self.ep.retries = 1.0;
        self.over = false;
        self.moments.clear();
        // A Strength already taken is in the server's state; none comes
        // later. The ganks go on as they were: a drill stays a drill.
        self.strength_at = -1;
        self.drop_next = -1;
        self.gank = k.gank.shifted(k.at);
        self.ep.drills = self.gank.drill as u8 as f64;
        if self.gank.drill {
            self.drill_length();
        }
        for a in 0..2 {
            self.phi[a] = self.potential(a).0;
        }
    }

    /// The second player's Strength and the ganks of the first, on the
    /// tick of the duel that just ended.
    fn change_damage(&mut self) {
        let now = self.ep_len - self.ticks_left;
        if self.strength_at >= 0 && now >= self.strength_at {
            let top = self.params.opp_strength_max.round().clamp(1.0, 3.0) as u64;
            self.m.s.p[1].strength = (self.rng.next_u64() % (top + 1)) as i32 - 1;
            self.strength_at = now + self.rng.range(100.0, 500.0).round() as i32;
        }
        if self.drop_next >= 0 && now >= self.drop_next {
            let h = self.m.s.p[0].health;
            let (lo, hi) = (self.params.drop_min, self.params.drop_max.max(self.params.drop_min));
            let amount = (self.rng.range(lo, hi) as f32).min(h - 2.0);
            if amount >= 2.0 {
                let a = self.rng.range(0.0, std::f64::consts::TAU);
                self.m.s.sudden = Some((0, amount, [a.cos(), a.sin()]));
                self.drop_next = now + self.rng.range(300.0, 900.0).round() as i32;
            } else {
                self.drop_next = now + 20;
            }
        }
        if self.gank.drill {
            self.drill_tick(now);
        } else if now < self.gank.end {
            // One hit reaches the server a tick; of those due, the hardest,
            // and the others come on the next ticks.
            let g = &mut self.gank;
            let due = (0..g.n).filter(|&k| now >= g.hit[k]).max_by(|&a, &b| g.damage[a].total_cmp(&g.damage[b]));
            if let Some(k) = due {
                self.m.s.gank = Some((0, g.damage[k], g.dir[k]));
                g.hit[k] = now + self.rng.range(10.0, 16.0).round() as i32;
            }
        } else if self.gank.next >= 0 && now >= self.gank.next {
            self.gank.end = now + self.rng.range(40.0, 120.0).round() as i32;
            let top = self.params.gank_max.round().clamp(1.0, GANK_MAX as f64) as u64;
            self.gank.n = 1 + (self.rng.next_u64() % top) as usize;
            let sword = self.m.s.cfg.damage;
            for k in 0..self.gank.n {
                self.gank.hit[k] = now + if k == 0 { 0 } else { self.rng.range(0.0, 16.0).round() as i32 };
                let amp = (self.rng.next_u64() % 3) as i32 - 1;
                self.gank.damage[k] = if amp < 0 { sword } else { (sword as f64 * (1.0 + 1.3 * (amp + 1) as f64)) as f32 };
                let a = self.rng.range(0.0, std::f64::consts::TAU);
                self.gank.dir[k] = [a.cos(), a.sin()];
            }
            self.gank.next = self.gank.end + self.rng.range(200.0, 800.0).round() as i32;
        }
    }

    /// A drill is no longer than `drill_seconds`, give or take a quarter
    /// (the same in `begin`).
    fn drill_length(&mut self) {
        let cap = (self.params.drill_seconds * 20.0 * self.rng.range(0.75, 1.25)).round().max(20.0) as i32;
        self.ep_len = self.ep_len.min(cap);
        self.ticks_left = self.ep_len;
    }

    /// A tick of a drill (see `Params::drill_prob`): a wave starts, or the
    /// players of the one under way run at the first player and the
    /// hardest of those in reach whose hit is due hits it.
    fn drill_tick(&mut self, now: i32) {
        let me = self.m.s.p[0].l.body.pos;
        let edge = self.m.world.half - 0.3;
        if now < self.gank.end {
            let g = &mut self.gank;
            let mut best: Option<usize> = None;
            for k in 0..g.n {
                let (dx, dz) = (g.pos[k][0] - me[0], g.pos[k][1] - me[2]);
                let d = (dx * dx + dz * dz).sqrt();
                if d > DRILL_CLOSE {
                    let step = DRILL_SPEED.min(d - DRILL_CLOSE) / d;
                    g.pos[k][0] -= dx * step;
                    g.pos[k][1] -= dz * step;
                }
                let (dx, dz) = (g.pos[k][0] - me[0], g.pos[k][1] - me[2]);
                let d = (dx * dx + dz * dz).sqrt();
                if d > 1e-6 {
                    g.dir[k] = [dx / d, dz / d];
                }
                if d <= DRILL_REACH && now >= g.hit[k] && best.map_or(true, |b| g.damage[k] > g.damage[b]) {
                    best = Some(k);
                }
            }
            if let Some(k) = best {
                self.m.s.gank = Some((0, g.damage[k], g.dir[k]));
                g.hit[k] = now + self.rng.range(10.0, 16.0).round() as i32;
            }
        } else if self.gank.next >= 0 && now >= self.gank.next {
            self.gank.end = now + self.rng.range(100.0, 300.0).round() as i32;
            self.gank.n = self.params.drill_gankers.round().clamp(1.0, GANK_MAX as f64) as usize;
            let top = self.params.drill_strength_max.round().clamp(0.0, 3.0) as u64;
            let amp = (self.rng.next_u64() % (top + 1)) as i32 - 1;
            let sword = self.m.s.cfg.damage;
            let damage = if amp < 0 { sword } else { (sword as f64 * (1.0 + 1.3 * (amp + 1) as f64)) as f32 };
            for k in 0..self.gank.n {
                self.gank.hit[k] = now + if k == 0 { 0 } else { self.rng.range(0.0, 16.0).round() as i32 };
                self.gank.damage[k] = damage;
                let a = self.rng.range(0.0, std::f64::consts::TAU);
                let d = self.rng.range(2.0, 5.0);
                self.gank.pos[k] = [(me[0] + d * a.cos()).clamp(-edge, edge), (me[2] + d * a.sin()).clamp(-edge, edge)];
                self.gank.dir[k] = [a.cos(), a.sin()];
            }
            self.gank.next = self.gank.end + self.rng.range(20.0, 80.0).round() as i32;
        }
    }

    fn begin(&mut self) {
        let p = &self.params;
        self.ep_len = self.rng.range(p.ep_ticks_min, p.ep_ticks_max.max(p.ep_ticks_min)).round() as i32;
        self.ticks_left = self.ep_len;
        self.strength_at = -1;
        if p.opp_strength_prob > 0.0 && self.rng.chance(p.opp_strength_prob) {
            self.strength_at = self.rng.range(0.0, self.ep_len as f64 * 2.0 / 3.0).round() as i32;
        }
        self.drop_next = -1;
        if p.drop_prob > 0.0 && self.rng.chance(p.drop_prob) {
            self.drop_next = self.rng.range(0.0, self.ep_len as f64 / 2.0).round() as i32;
        }
        self.gank = Gank::NONE;
        let drill = p.drill_prob > 0.0
            && !self.m.s.cfg.infinite_health
            && (self.mode == Mode::Bot || p.drill_self)
            && self.rng.chance(p.drill_prob);
        if drill {
            self.gank.drill = true;
            self.gank.next = self.rng.range(0.0, 60.0).round() as i32;
            self.m.s.p[1].immortal = true;
            self.m.s.p[1].no_kb = true;
            // No longer than `drill_seconds`, give or take a quarter.
            let cap = (p.drill_seconds * 20.0 * self.rng.range(0.75, 1.25)).round().max(20.0) as i32;
            self.ep_len = self.ep_len.min(cap);
            self.ticks_left = self.ep_len;
        } else if p.gank_prob > 0.0 && self.rng.chance(p.gank_prob) {
            self.gank.next = self.rng.range(0.0, self.ep_len as f64 / 2.0).round() as i32;
        }
        self.rew_acc = [0.0; 2];
        self.combo = [0; 2];
        self.max_combo = [0; 2];
        self.ep = Stats::default();
        self.ep.drills = self.gank.drill as u8 as f64;
        self.bot_state = BotState::default();
        self.landed = None;
        self.pearl_kill = [0; 2];
        self.since_block = [1000; 2];
        self.pot_since = [1000; 2];
        self.was_open = [false; 2];
        self.pot_back = [true; 2];
        self.off_run = [0; 2];
        self.steal = self.mode == Mode::SelfPlay && p.league_steal > 0.0 && self.rng.chance(p.league_steal);
        self.over = false;
        self.view_was = [0, 1].map(|a| [self.m.c[a].lp.l.yaw, self.m.c[a].lp.l.pitch]);
        self.turn_last = [[0.0; 2]; 2];
        self.turn_age = [[0; 2]; 2];
        let own = self.rng.range(p.cps_min, p.cps_max.max(p.cps_min));
        let (lo, hi) = if p.opp_cps_min < 0.0 { (p.cps_min, p.cps_max) } else { (p.opp_cps_min, p.opp_cps_max) };
        let other = self.rng.range(lo, hi.max(lo));
        let cps = [own.max(1.0), other.max(1.0)];
        self.ag = [0, 1].map(|a| Agent::new([self.ping_ms[a], self.ping_ms[1 - a]], cps[a], self.m.c[a].lp.l.body.pos));
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
    /// and the thrower pays for the part of its potion the game would have
    /// given it nothing for (what a strict duel takes on top is lost
    /// health already and is not charged again).
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
            self.rew_acc[t] -= p.w_waste * (POTION_HEAL - s.would[t]).max(0.0) / hh;
            self.rew_acc[t] -= p.w_gift * s.healed[1 - t] / hh;
            self.rew_acc[t] -= p.w_miss * (POTION_HEAL - s.offered[t]).clamp(0.0, MISS_POINTS) / hh;
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
                        self.rew_acc[a] -= self.params.w_win + self.params.w_death;
                        self.ep.deaths[a] += 1.0;
                        if a == 0 && self.gank.drill {
                            self.ep.drill_deaths += 1.0;
                        }
                        if self.m.s.p[a].l.body.pos[1] < self.m.world.void_y {
                            self.ep.void_falls[a] += 1.0;
                        }
                    } else {
                        self.rew_acc[a] += self.params.w_win;
                        if self.pearl_kill[a] > 0 {
                            self.rew_acc[a] += self.params.w_pearl_cost;
                            self.pearl_kill[a] = 0;
                        }
                    }
                }
            }
        }
        if self.m.s.hits.is_empty() {
            return;
        }
        if let Some((thrower, _)) = self.landed.take() {
            let first = self.m.s.hits[0];
            let w = self.params.w_pearl_next;
            if first.attacker == thrower {
                self.ep.pearl_won[thrower] += 1.0;
                self.rew_acc[thrower] += w;
            } else {
                self.ep.pearl_lost[thrower] += 1.0;
                self.rew_acc[thrower] -= w;
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
            let run = self.combo[h.attacker].min(self.params.combo_cap.max(0.0) as u32) as f32 * self.params.w_combo;
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

    /// What the agents are told about this duel.
    fn duel(&self) -> Duel {
        let cfg = &self.m.s.cfg;
        Duel {
            arena_half: self.params.arena_half,
            floor_y: self.params.floor_y,
            center: [0.0, 0.0],
            real_health: !cfg.infinite_health,
            hit_health: cfg.hit_health(),
            pearl_cooldown: cfg.pearl_cooldown,
            drink_window_s: self.params.drink_window_s,
        }
    }

    /// What agent `id` can know from its own client.
    pub fn write_obs(&self, id: usize, o: &mut [f32]) {
        self.ag[id].write_obs(&self.m.c[id], &self.m.world, &self.m.ccfg, &self.duel(), o);
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
        let have = (sp.speed_ticks.max(0) as f32 / self.duel().drink_window().max(1) as f32).min(1.0);
        let fast = p.w_speed * (have + (1.0 - have) * done(Item::Drink));
        let fed = p.w_fed * if sp.food.level > 6 { 1.0 } else { done(Item::Food) };
        // The hotbar as the player's own client has it: out of healing
        // potions with some in the inventory, and out of Speed and of
        // potions of it with one in the inventory.
        let lp = &c.lp;
        let dry = (lp.potions == 0 && lp.rows.count(Item::Potion) > 0) as u8
            + (lp.drinks == 0 && lp.speed_amp < 0 && lp.rows.count(Item::Drink) > 0) as u8;
        let stock = -p.w_stock * dry as f32;
        let health = if p.w_hp_pot != 0.0 {
            let k = p.hp_pot_scale.max(0.1);
            let low = |h: f32| -(-h.max(0.0) / k).exp();
            p.w_hp_pot * (low(sp.health) - low(self.m.s.p[1 - id].health))
        } else {
            0.0
        };
        (p.w_aim * aim as f32 + p.w_range * range as f32 + fast + fed + stock + health, ang)
    }

    /// Is the other player within attack reach of `id`, in `id`'s own view?
    fn in_reach(&self, id: usize) -> bool {
        let c = &self.m.c[id];
        let eye = c.lp.eye();
        let ebb = c.rp.bb.expand(0.1, 0.1, 0.1);
        dist(eye, ebb.closest_point(eye)) <= 3.0
    }

    /// Statistics of the episode under way.
    pub fn episode(&self) -> &Stats {
        &self.ep
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
        // Fighting on hungry with steak at hand, and slow with a potion of
        // Speed to drink.
        let sp = &self.m.s.p[id];
        if p.w_hungry != 0.0 && sp.steak > 0 && sp.food.level < FOOD_TO_HEAL {
            let short = ((FOOD_TO_HEAL - sp.food.level) as f32 / (FOOD_TO_HEAL - FOOD_TO_SPRINT) as f32).min(1.0);
            r -= p.w_hungry * (0.5 + 0.5 * short);
        }
        if p.w_slow != 0.0 && sp.speed_ticks <= 0 && (sp.drinks != 0 || sp.rows.count(Item::Drink) > 0) {
            r -= p.w_slow;
        }
        // In the hurt time after a hit the player can take no other, nor
        // its knockback: a moment for the inventory even next to the
        // opponent.
        let safe = self.m.s.p[id].hurt_resistant_time > 10;
        if p.w_inv_near != 0.0 && self.m.c[id].lp.inv_open && !safe {
            let (a, b) = (&self.m.s.p[id].l.body.pos, &self.m.s.p[1 - id].l.body.pos);
            if (a[0] - b[0]).hypot(a[2] - b[2]) < p.inv_near_dist {
                r -= p.w_inv_near;
            }
        }
        if out.moved == Some(Item::Drink) {
            let lp = &self.m.c[id].lp;
            let left = self.m.s.p[id].speed_ticks;
            if lp.drinks.count_ones() >= 2 || left > self.duel().drink_window() {
                r -= p.w_speed_refill;
                self.ep.speed_waste[id] += 1.0;
            }
        }
        if self.m.c[id].lp.inv_open && safe {
            self.ep.inv_hurt[id] += 1.0;
        }
        // A turn of the view that takes back the one before it.
        let view = [self.m.c[id].lp.l.yaw, self.m.c[id].lp.l.pitch];
        for k in 0..2 {
            let d = view[k] - self.view_was[id][k];
            if d == 0.0 {
                self.turn_age[id][k] = (self.turn_age[id][k] + 1).min(1000);
                continue;
            }
            let last = self.turn_last[id][k];
            if last * d < 0.0 && self.turn_age[id][k] <= SHAKE_TICKS {
                let undone = last.abs().min(d.abs());
                r -= p.w_shake * undone.min(SHAKE_DEG) / SHAKE_DEG;
                self.ep.shake[id] += undone as f64;
            }
            self.turn_last[id][k] = d;
            self.turn_age[id][k] = 0;
        }
        self.view_was[id] = view;
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
        // The ticks a potion thrown with a turn keeps the view turned are the
        // throw itself: what counts is the time to the crosshair after it.
        let turned = self.ag[id].turned();
        if out.threw_potion {
            self.ep.potions[id] += 1.0;
            if self.pot_since[id] <= 3 {
                self.ep.pot_double[id] += 1.0;
            }
            self.ep.pot_health[id] += self.m.c[id].lp.health as f64;
            // The ticks spent looking away to aim it, unless they were
            // already counted after the potion before.
            if self.pot_back[id] {
                let before = self.off_run[id].min(POT_TIME_TICKS);
                self.ep.pot_away[id] += before as f64;
                self.rew_acc[id] -= p.w_pot_time * before as f32;
            }
            self.pot_since[id] = 0;
            self.pot_back[id] = false;
        }
        if !self.pot_back[id] && !turned {
            if off && self.pot_since[id] < 40 {
                self.ep.pot_away[id] += 1.0;
                self.ep.pot_after[id] += 1.0;
                if self.pot_since[id] < POT_TIME_TICKS {
                    self.rew_acc[id] -= p.w_pot_time;
                }
            } else {
                self.pot_back[id] = true;
            }
        }
        self.off_run[id] = if off && !turned { self.off_run[id] + 1 } else { 0 };
        if off {
            self.ep.off[id] += 1.0;
        }
        if self.pot_since[id] < 1000 {
            self.pot_since[id] += 1;
        }
        // Drinking and eating.
        if self.m.c[id].lp.consuming() {
            self.ep.sip_ticks[id] += 1.0;
            if !self.ag[id].was_consuming {
                self.ep.sips[id] += 1.0;
            }
        }
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
            self.rew_acc[id] -= self.params.w_pearl_cost;
            self.pearl_kill[id] = (self.params.pearl_kill_s * 20.0).round().max(1.0) as i32;
            let o = &self.m.s.p[1 - id];
            let potting = o.in_hand().0 == Item::Potion || self.m.s.thrown.iter().any(|t| t.thrower == 1 - id && t.kind == Kind::Potion && !t.dead);
            let (a, b) = (&self.m.s.p[id].l.body.pos, &o.l.body.pos);
            let apart = (a[0] - b[0]).hypot(a[2] - b[2]) > 3.0;
            if potting || self.combo[1 - id] >= 2 || (self.combo[id] >= 2 && apart) {
                self.ep.pearl_sense[id] += 1.0;
            }
        }
        let duel = self.duel();
        self.ag[id].ticked(&self.m.c[id], &duel, inp, out);
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
        if self.params.retry_prob > 0.0 {
            // A death of the first player: the moment two to four seconds
            // before it is kept for retries.
            if how == 1 && !self.m.s.p[0].alive() && self.moments.len() >= 3 {
                let back = (self.rng.next_u64() % (self.moments.len() as u64 - 2)) as usize;
                let k = self.moments[back].clone();
                if self.losses.len() >= LOSSES {
                    let i = (self.rng.next_u64() % LOSSES as u64) as usize;
                    self.losses.swap_remove(i);
                }
                self.losses.push((k, RETRIES));
            }
            self.moments.clear();
            if !self.losses.is_empty() && self.rng.chance(self.params.retry_prob) {
                let i = (self.rng.next_u64() % self.losses.len() as u64) as usize;
                self.losses[i].1 -= 1;
                let k = if self.losses[i].1 == 0 { self.losses.swap_remove(i).0 } else { self.losses[i].0.clone() };
                self.resume(k);
                return;
            }
        }
        let (m, ping, bot, lesson) = new_duel(&self.params, &mut self.rng, self.mode);
        self.m = m;
        self.ping_ms = ping;
        self.bot = bot;
        self.lesson = lesson;
        self.begin();
    }

    /// The agent of a player, to set up a check.
    pub fn agent_mut(&mut self, id: usize) -> &mut Agent {
        &mut self.ag[id]
    }

    /// Apply the pending agent's action and run until an agent has to act
    /// again. Returns that agent's index.
    pub fn step(&mut self, action: &[i32]) -> usize {
        let id = self.pending;
        let duel = self.duel();
        self.ag[id].teacher.on = self.params.teach > 0.0;
        let mut inp = self.ag[id].keys(&self.m.c[id], &self.m.world, &duel, action, &mut self.rng);
        self.overridden = id == 1 && self.steal && !self.ag[1].busy() && inp.inv == 0 && steal_keys(&self.m.c[1], self.m.ccfg.mouse_f1 as f64 * 0.15, &mut inp);
        self.teach = self.ag[id].teacher.teach;
        let out = self.m.client_tick(id, &inp);
        self.account(id, &inp, &out);
        let mut round_done = id == 1;
        let mut next = self.m.advance();
        self.collect_hits();
        if self.mode == Mode::Bot {
            while next == 1 {
                let mut b = bot::act(&self.m.c[1], &self.m.world, &self.m.ccfg, &self.bot, &mut self.bot_state, &mut self.rng);
                if self.params.opp_cps_min >= 0.0 {
                    self.ag[1].limit_clicks(&mut b, &mut self.rng);
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
            for a in 0..2 {
                self.pearl_kill[a] = (self.pearl_kill[a] - 1).max(0);
            }
            self.ticks_left -= 1;
            self.change_damage();
            if self.m.world.terrain().is_some() {
                self.ep.terrain_ticks[0] += 1.0;
                self.ep.terrain_ticks[1] += 1.0;
            }
            if self.over {
                self.finish_episode(1);
            } else if self.ticks_left <= 0 {
                self.finish_episode(2);
            } else if self.params.retry_prob > 0.0 && self.ticks_left % 20 == 0 {
                if self.moments.len() >= MOMENTS {
                    self.moments.pop_front();
                }
                let k = self.moment();
                self.moments.push_back(k);
            }
        }
        self.ag[self.pending].before_tick(&self.m.c[self.pending], &self.m.world);
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
            let inp = &self.ag[id].prev_inp;
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
        o[g + 6] = self.ag[0].cps as f32;
        o[g + 7] = self.ag[1].cps as f32;
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

    /// Per environment, 1 if the keys of the action last made were laid
    /// over by others (`Env::overridden`), else 0.
    pub fn overridden(&self, out: &mut [u8]) {
        for (o, e) in out.iter_mut().zip(&self.envs) {
            *o = e.overridden as u8;
        }
    }

    /// Per environment, what the teacher of the agent that made the last
    /// tick would have chosen on it (`Env::teach`), ACT_HEADS numbers each.
    pub fn teach(&self, out: &mut [i32]) {
        for (o, e) in out.chunks_mut(ACT_HEADS).zip(&self.envs) {
            o.copy_from_slice(&e.teach);
        }
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
