//! The server side of a duel: `NetHandlerPlayServer` packet handlers, the
//! parts of `EntityPlayerMP` that decide hits and knockback, the ender
//! pearls and potions in flight (`EntityEnderPearl`, `EntityPotion`), and
//! the `EntityTrackerEntry` that tells each client where the other player
//! and those are.
use crate::aabb::{dist_sq, Aabb, V3, EYE_HEIGHT};
use crate::living::Living;
use crate::mth::{self, JavaRandom, Rng};
use crate::net::{
    shift_click, Bar, Item, Kind, Rows, ThrownEv, Tool, BLOCK_TICKS, C2S, DRINK_AMP, DRINK_TICKS, ROD_MAX_WEAR, S2C, SLOT_SWORD, STEAK_FOOD,
    STEAK_SATURATION, USE_TICKS,
};
use crate::world::{RayHit, World};
use std::collections::VecDeque;

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
    /// Both players have Resistance V and Saturation, as on a practice
    /// server's combo arena: no hit costs health and nobody gets hungry.
    pub infinite_health: bool,
    pub fall_damage: bool,
    /// Armour points worn (`getTotalArmorValue`): 20 for a full diamond set.
    pub armor: i32,
    /// What the armour's Protection adds up to against a hit, a
    /// suffocation or a fall: see `prot_points`.
    pub prot: i32,
    /// What Feather Falling adds to that against a fall, which is also what
    /// an ender pearl costs: see `feather_points`.
    pub feather: i32,
    /// Running, jumping, hitting and being hit use up food.
    pub hunger: bool,
    /// The game rule `naturalRegeneration`: a player with 18 food or more
    /// gets half a heart back every 80 of its ticks.
    pub natural_regen: bool,
    /// `maxHurtResistantTime`: a player can be hit again once half of it
    /// has passed (the game: 20, a hit every 10 ticks). Servers lower it.
    pub max_no_damage: i32,
    /// Chance that a hit costs the sword a point of durability: 1 for a
    /// plain sword, 0.25 with Unbreaking III, 0 for an unbreakable one.
    /// The server then sends the attacker its sword again, and a client
    /// that gets it while blocking stops blocking on its own side only.
    pub sword_wear: f32,
    /// Server ticks a player has to wait between two ender pearls. The game
    /// has no wait (0); servers add one with a plugin, usually 16 seconds.
    pub pearl_cooldown: i32,
    /// A lesson, not the game: the share of what the game gives that a
    /// healing potion gives its own thrower unless it offered all eight
    /// points (it hit the thrower or broke within a quarter of a block of
    /// its feet). 1 is the game.
    pub potion_strict: f32,
    /// A newer server under a 1.8 client (1.9 and on, as most are behind
    /// ViaVersion): a pearl or potion takes on the motion its thrower last
    /// told the server of, across the ground always and up or down while
    /// it is off the ground (`shootFromRotation` adding the thrower's known
    /// movement). Thrown while running or knocked back, a potion flies on
    /// with the thrower; thrown at the feet while going forward, it lands
    /// ahead. 1.8 itself adds nothing.
    pub throw_carry: bool,
    /// CraftBukkit and every server built on it (Spigot, Paper and the
    /// rest) no longer drop a hit that does no damage ("Don't filter out 0
    /// damage" in `EntityHuman.damageEntity`): a snowball, an egg, a rod's
    /// hook or an ender pearl that hits a player hurts it for nothing, with
    /// the knockback of a hit from wherever its thrower stands, and starts
    /// its invulnerable ticks. The plain game drops such a hit, and they
    /// fly on through.
    pub zero_hits: bool,
    /// The chance that a hit on a player whose sword is up on the server
    /// is not cut by it. 0 is the game. Some servers ignore about half of
    /// the blocks a client holds at the right time, with no rule a player
    /// can see (`Params::block_fail`).
    pub block_fail: f32,
}

impl ServerCfg {
    /// The health a plain hit with the sword costs a player in this kit, on
    /// average over what Protection draws.
    pub fn hit_health(&self) -> f32 {
        let m = self.prot.clamp(0, 25);
        let k = (((m + 1) >> 1) as f32 + (m >> 1) as f32 * 0.5).min(20.0);
        (self.damage + self.ench_damage) * (25 - self.armor) as f32 / 25.0 * (25.0 - k) / 25.0
    }
}

impl Default for ServerCfg {
    fn default() -> Self {
        // Diamond sword in 1.8: 1 (hand) + 7.
        ServerCfg {
            damage: 8.0,
            ench_damage: 0.0,
            kb_ench: 0,
            infinite_health: true,
            fall_damage: true,
            armor: 0,
            prot: 0,
            feather: 0,
            hunger: true,
            natural_regen: true,
            max_no_damage: 20,
            sword_wear: 1.0,
            pearl_cooldown: 0,
            potion_strict: 1.0,
            throw_carry: false,
            zero_hits: false,
            block_fail: 0.0,
        }
    }
}

/// `EnchantmentProtection.calcModifierDamage` for one piece of armour with
/// Protection of this level: 1, 2, 3 and 5 for levels I to IV.
pub fn prot_points(level: i32) -> i32 {
    if level <= 0 {
        return 0;
    }
    let f = (6 + level * level) as f32 / 3.0f32;
    mth::floor_f(f * 0.75f32)
}

/// The same for Feather Falling against a fall: 5, 8, 12 and 18.
pub fn feather_points(level: i32) -> i32 {
    if level <= 0 {
        return 0;
    }
    let f = (6 + level * level) as f32 / 3.0f32;
    mth::floor_f(f * 2.5f32)
}

/// What hurts a player (`DamageSource`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A hit by the other player: a block halves it and armour takes its
    /// share.
    Player(usize),
    /// A fall, which is also what an ender pearl does to its thrower. It
    /// goes past the block and the armour; Protection and Feather Falling
    /// still count.
    Fall,
    /// The head inside a block. Past the block and the armour as well.
    Wall,
    /// Hunger. Nothing lessens it.
    Starve,
    /// A snowball, an egg, a hook or a pearl that hit, thrown by this
    /// player (`causeThrownDamage`): it does no damage, and its knockback
    /// is from where the thrower is.
    Thrown(usize),
}

/// `FoodStats`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Food {
    pub level: i32,
    pub saturation: f32,
    pub exhaustion: f32,
    pub timer: i32,
}

impl Food {
    pub const FULL: Food = Food { level: 20, saturation: 5.0, exhaustion: 0.0, timer: 0 };

    #[inline]
    fn add_exhaustion(&mut self, x: f32) {
        self.exhaustion = (self.exhaustion + x).min(40.0f32);
    }
}

/// An ender pearl or a splash potion in flight (`EntityThrowable`) with its
/// tracker entry.
#[derive(Clone, Debug)]
pub struct Thrown {
    pub id: u32,
    pub kind: Kind,
    pub thrower: usize,
    pub pos: V3,
    pub motion: V3,
    pub yaw: f32,
    pub pitch: f32,
    pub prev_yaw: f32,
    pub prev_pitch: f32,
    pub ticks_in_air: i32,
    pub dead: bool,
    // EntityTrackerEntry for it.
    pub enc_pos: [i32; 3],
    pub enc_yaw: i32,
    pub enc_pitch: i32,
    pub update_counter: i32,
    pub ticks_since_teleport: i32,
    /// Whether the thing was on the ground when its tracker last sent it
    /// whole: only a hook can be, since only a hook moves as entities do.
    pub tracker_on_ground: bool,
    pub last_motion: V3,
    /// The direction and speed of the throw before the game added its
    /// random part.
    pub aim: V3,
    /// For a rod's hook, what it has besides: see `Hook`.
    pub hook: Option<Hook>,
}

/// What `EntityFishHook` keeps besides a thrown thing's position and
/// motion: its box a quarter of a block on each side, moved through the
/// world like any entity's (`moveEntity`, no step up); whether it lies on
/// a block, and the player it has caught, if any.
#[derive(Clone, Debug)]
pub struct Hook {
    pub body: crate::body::Body,
    pub in_ground: bool,
    pub caught: Option<usize>,
}

/// `EntityFishHook.setSize(0.25F, 0.25F)`: half its width, and its height.
const HOOK_HALF: f64 = (0.25f32 / 2.0f32) as f64;
const HOOK_HEIGHT: f64 = 0.25f32 as f64;

/// The box of a hook whose feet are at `p` (`Entity.setPosition`).
#[inline]
pub fn hook_box(p: V3) -> Aabb {
    Aabb::new([p[0] - HOOK_HALF, p[1], p[2] - HOOK_HALF], [p[0] + HOOK_HALF, p[1] + HOOK_HEIGHT, p[2] + HOOK_HALF])
}

/// A snowball, an egg, a hook or a pearl that hit a player and counted
/// (see `ServerCfg::zero_hits`).
#[derive(Clone, Copy, Debug)]
pub struct ThrownHit {
    pub kind: Kind,
    pub thrower: usize,
    pub victim: usize,
}

/// A pearl that came down and moved its thrower.
#[derive(Clone, Copy, Debug)]
pub struct Landing {
    pub thrower: usize,
    /// Where the thrower was put: the pearl's position at the start of the
    /// tick in which it hit.
    pub pos: V3,
    /// It hit the other player rather than a block.
    pub on_player: bool,
    /// Ticks it flew.
    pub ticks: i32,
    /// Health the landing cost the thrower.
    pub dealt: f32,
}

/// A potion that broke.
#[derive(Clone, Copy, Debug)]
pub struct Splash {
    pub thrower: usize,
    /// Where it was at the start of the tick in which it hit: distances to
    /// the players are measured from here.
    pub pos: V3,
    /// The player it hit, if it was not a block.
    pub on_player: Option<usize>,
    /// Ticks it flew.
    pub ticks: i32,
    /// Health it offered each player, and health each one got from it.
    pub offered: [f32; 2],
    pub healed: [f32; 2],
    /// Health each one would have got from it in the game: the same, but
    /// for a thrower in a strict duel.
    pub would: [f32; 2],
}

/// Health a splash potion of Instant Health II gives at full strength.
pub const POTION_HEAL: f32 = 8.0;

/// `EntityPotion.onImpact` and `Potion.affectEntity` for Instant Health II:
/// the health a potion breaking at `at` offers a player whose feet are at
/// `feet`. A player the potion hit itself gets all of it.
#[inline]
pub fn splash_heal(at: V3, feet: V3, direct: bool) -> f32 {
    let d0 = dist_sq(at, feet);
    if !(d0 < 16.0) {
        return 0.0;
    }
    let d1 = if direct { 1.0 } else { 1.0 - d0.sqrt() / 4.0 };
    (d1 * (4 << 1) as f64 + 0.5) as i32 as f32
}

/// `new EntityThrowable(world, thrower)` up to the random part of
/// `setThrowableHeading`: where a pearl or potion thrown by a player
/// standing at `feet` with this view starts, and the direction it is thrown
/// in (length one).
pub fn throw_heading(kind: Kind, feet: V3, yaw: f32, pitch: f32) -> (V3, V3) {
    let rad = mth::PI_F;
    let mut pos = [feet[0], feet[1] + EYE_HEIGHT, feet[2]];
    pos[0] -= (mth::cos(yaw / 180.0f32 * rad) * 0.16f32) as f64;
    pos[1] -= 0.10000000149011612;
    pos[2] -= (mth::sin(yaw / 180.0f32 * rad) * 0.16f32) as f64;
    let f = 0.4f32;
    let mx = (-mth::sin(yaw / 180.0f32 * rad) * mth::cos(pitch / 180.0f32 * rad) * f) as f64;
    let mz = (mth::cos(yaw / 180.0f32 * rad) * mth::cos(pitch / 180.0f32 * rad) * f) as f64;
    let my = (-mth::sin((pitch + kind.pitch_offset()) / 180.0f32 * rad) * f) as f64;
    // setThrowableHeading(x, y, z, getVelocity(), 1.0F)
    let len = mth::sqrt_d(mx * mx + my * my + mz * mz) as f64;
    (pos, [mx / len, my / len, mz / len])
}

/// The deviation of the gaussian `setThrowableHeading` adds to each part
/// of a pearl's or potion's direction (0.0075 as a float).
pub const THROW_SPREAD: f64 = 0.007499999832361937;

/// Damage a pearl does to its thrower on arrival (`DamageSource.fall`).
pub const PEARL_DAMAGE: f32 = 5.0;

pub const MAX_HEALTH: f32 = 20.0;
/// The ticks of a Speed that does not run out within a duel.
pub const FOR_GOOD: i32 = 10_000_000;
/// The longest an effect can be said to last in its packet.
pub const EFFECT_TICKS_SENT: i32 = 32767;
/// The Speed of a potion shows particles.
pub const DRINK_SHOWN: bool = true;

/// Would `World.getEntitiesWithinAABB` find an entity filed under chunk
/// `c` for the box `q`? It looks through the chunks within two blocks of
/// the box and the 16-block slices within two blocks of its height.
#[inline]
fn filed_near(q: &Aabb, c: [i32; 3]) -> bool {
    let near = |lo: f64, hi: f64, v: i32| mth::floor_d((lo - 2.0) / 16.0) <= v && v <= mth::floor_d((hi + 2.0) / 16.0);
    near(q.min[0], q.max[0], c[0])
        && near(q.min[2], q.max[2], c[2])
        && mth::floor_d((q.min[1] - 2.0) / 16.0).clamp(0, 15) <= c[1]
        && c[1] <= mth::floor_d((q.max[1] + 2.0) / 16.0).clamp(0, 15)
}

/// `Entity.isEntityInsideOpaqueBlock` for a standing player at `pos`: is one
/// of eight points around the eyes inside a solid block?
pub fn inside_opaque_block(world: &World, pos: V3) -> bool {
    let width = 0.6f32;
    for i in 0..8 {
        let dy = (((i >> 0) % 2) as f32 - 0.5f32) * 0.1f32;
        let dx = (((i >> 1) % 2) as f32 - 0.5f32) * width * 0.8f32;
        let dz = (((i >> 2) % 2) as f32 - 0.5f32) * width * 0.8f32;
        let j = mth::floor_d(pos[1] + dy as f64 + EYE_HEIGHT);
        let k = mth::floor_d(pos[0] + dx as f64);
        let l = mth::floor_d(pos[2] + dz as f64);
        if world.solid(k, j, l) {
            return true;
        }
    }
    false
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
    /// The victim was blocking with its sword on the server.
    pub blocked: bool,
    /// Damage after the block, before armour.
    pub damage: f32,
    /// Health the hit took: what is left of it after armour and Protection.
    pub dealt: f32,
}

#[derive(Clone, Debug)]
pub struct ServerPlayer {
    pub l: Living,
    pub hurt_resistant_time: i32,
    /// Server ticks on which the other player could take a full hit from
    /// this one, its box within 3 blocks of this one's eyes, and took none;
    /// and attacks of this one that arrived while the other could not take
    /// a full hit yet. Counted for the environment, which takes them.
    pub late_ticks: u32,
    pub hurt_clicks: u32,
    pub last_damage: f32,
    pub hurt_time: i32,
    pub health: f32,
    pub is_airborne: bool,
    pub velocity_changed: bool,
    /// The flags byte changed and has not been sent yet.
    pub meta_dirty: bool,
    /// The same for the health, which is a watched value too.
    pub health_dirty: bool,
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
    /// How far the last movement packet moved this player (none for one
    /// without a place in it, or after a teleport): what `throw_carry`
    /// adds to a throw.
    pub moved: V3,
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
    /// `isUsingItem()`: blocking with the sword, drinking or eating, as far
    /// as the server knows. The using-item flag other players see is the
    /// same thing.
    pub using_item: bool,
    /// The hotbar slot of the stack in use and `itemInUseCount`: the ticks
    /// until the drink or the meal is finished.
    pub use_slot: u8,
    pub use_count: i32,
    /// The slots whose stack is not what the player was last sent: a sword
    /// that lost durability, the bottle a drink left, a steak less. They
    /// are sent on the next tick. Bits 0 to 8 are the hotbar, 9 to 35 the
    /// rows above it.
    pub dirty: u64,
    /// The stack in use was moved by a click in the inventory, so it is no
    /// longer the stack in hand.
    pub use_moved: bool,
    /// Stands for a client that blocks around every hit it takes, without
    /// slowing down or giving up its own hits: the server always has it
    /// blocking.
    pub autoblock: bool,
    /// Damage taken, before armour, since the caller last cleared it.
    pub damage_taken: f32,
    /// Health lost and health got back since the caller last cleared them.
    pub health_lost: f32,
    pub health_gained: f32,
    pub food: Food,
    /// `lastHealth`, `lastFoodLevel` and `wasHungry`: what this player was
    /// last told about itself.
    pub last_health: f32,
    pub last_food: i32,
    pub was_hungry: bool,
    /// `inventory.currentItem`: the hotbar slot in hand.
    pub held: u8,
    /// Ender pearls left in the second hotbar slot.
    pub pearls: u8,
    /// The hotbar slots that hold a splash potion, one bit each.
    pub potions: u16,
    /// Those that hold a potion to drink and those that hold an empty
    /// bottle, and the steaks in the last slot.
    pub drinks: u16,
    pub bottles: u16,
    pub steak: u8,
    /// A rod, snowballs or eggs, if the kit has one (see `Tool`).
    pub tool: Tool,
    /// `fishEntity`: the id of this player's hook while it is out.
    pub fish: Option<u32>,
    /// The three rows of the inventory. `can_click` is false from a click
    /// the server did not take as the client said until the client has
    /// answered (`Container.getCanCraft`); `refused` is the number of that
    /// click. `clicks_done` counts the clicks that moved a stack since the
    /// caller last cleared it.
    pub rows: Rows,
    pub can_click: bool,
    pub refused: u16,
    pub clicks_done: u32,
    /// Ticks of Speed left (0: none; its amplifier is in the attributes)
    /// and whether the effect shows particles.
    pub speed_ticks: i32,
    pub speed_shown: bool,
    /// The watched colour of the effect particles is not zero: other
    /// players see particles around this one. Sent when it changes.
    pub fx: bool,
    pub fx_dirty: bool,
    /// Drinks and meals finished since the caller last cleared them.
    pub drinks_done: u32,
    pub meals_done: u32,
    /// Damage value of the sword: the hits that have cost it durability.
    pub sword_wear: u32,
    /// Amplifier of a Strength effect on this player, -1 for none: the
    /// effect's modifier of `generic.attackDamage` multiplies the total by
    /// `1 + 1.3 (amplifier + 1)` (`PotionAttackDamage`, operation 2).
    pub strength: i32,
    /// `previousEquipment[0]`: what this player held when it was last
    /// updated. The damage of its attacks comes from this item, and the
    /// other player is told when it changes.
    pub equip: (Item, u32),
    /// Pearls and potions that are gone, reported on the next tick
    /// (`destroyedItemsNetCache`).
    pub gone: Vec<u32>,
    /// Ticks until the server lets this player throw the next pearl.
    pub pearl_wait: i32,
    /// Stands for a player that cannot die (a drill): hits on it take no
    /// health, as with Resistance V.
    pub immortal: bool,
    /// Stands for a client that takes no knockback from hits: it is sent
    /// no velocity for them and stays where it is.
    pub no_kb: bool,
    /// Stands for a client with packet Criticals: before each attack it
    /// sends the server a tiny hop off the ground, so the server has it
    /// falling and every hit it makes is a critical one (1.5 times the
    /// sword's damage), with no jump to be seen.
    pub always_crit: bool,
    /// The cooldown plugin leaves this player alone (`pearl_cooldown` is
    /// for everyone else): it may throw a pearl whenever it likes, as the
    /// game itself has it.
    pub no_pearl_cd: bool,
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
            late_ticks: 0,
            hurt_clicks: 0,
            last_damage: 0.0,
            hurt_time: 0,
            health: 20.0,
            is_airborne: false,
            velocity_changed: false,
            meta_dirty: false,
            health_dirty: false,
            attr_dirty: false,
            swing_in_progress: false,
            swing_progress_int: 0,
            update_counter: 1,
            ticks_since_teleport: 0,
            tracker_on_ground: true,
            has_moved: true,
            last_pos: pos,
            moved: [0.0; 3],
            net_ticks: 0,
            last_move_tick: 0,
            chunk: chunk_of(pos),
            client_sprint: false,
            last_ground_y: pos[1],
            touched_top: false,
            using_item: false,
            use_slot: SLOT_SWORD,
            use_count: 0,
            dirty: 0,
            use_moved: false,
            autoblock: false,
            damage_taken: 0.0,
            health_lost: 0.0,
            health_gained: 0.0,
            food: Food::FULL,
            last_health: 20.0,
            last_food: 20,
            was_hungry: false,
            held: SLOT_SWORD,
            pearls: 0,
            potions: 0,
            drinks: 0,
            bottles: 0,
            steak: 0,
            tool: Tool::default(),
            fish: None,
            rows: Rows::default(),
            can_click: true,
            refused: 0,
            clicks_done: 0,
            speed_ticks: if speed_amp >= 0 { FOR_GOOD } else { 0 },
            speed_shown: false,
            fx: false,
            fx_dirty: false,
            drinks_done: 0,
            meals_done: 0,
            sword_wear: 0,
            strength: -1,
            equip: (Item::Sword, 0),
            gone: Vec::new(),
            pearl_wait: 0,
            immortal: false,
            no_kb: false,
            always_crit: false,
            no_pearl_cd: false,
        }
    }

    /// The hotbar but for the sword.
    #[inline]
    pub fn bar(&self) -> Bar {
        Bar { pearls: self.pearls, potions: self.potions, drinks: self.drinks, bottles: self.bottles, steak: self.steak, tool: self.tool }
    }

    #[inline]
    pub fn set_bar(&mut self, b: Bar) {
        self.pearls = b.pearls;
        self.potions = b.potions;
        self.drinks = b.drinks;
        self.bottles = b.bottles;
        self.steak = b.steak;
        self.tool = b.tool;
    }

    /// The stack in a slot: which item, and its wear (a sword) or its size.
    #[inline]
    pub fn stack(&self, slot: u8) -> (Item, u32) {
        if slot == SLOT_SWORD {
            (Item::Sword, self.sword_wear)
        } else {
            self.bar().stack(slot)
        }
    }

    /// The stack in hand.
    #[inline]
    pub fn in_hand(&self) -> (Item, u32) {
        self.stack(self.held)
    }

    /// `EntityPlayer.setItemInUse` / `clearItemInUse` with the flag other
    /// players see.
    pub fn set_using(&mut self, u: bool) {
        let u = u || self.autoblock;
        if self.using_item != u {
            self.using_item = u;
            self.meta_dirty = true;
        }
        if !u {
            self.use_moved = false;
        }
    }

    /// `setItemInUse(stack, duration)` for the stack in hand: nothing
    /// changes when that very stack is in use already.
    fn start_use(&mut self, ticks: i32) {
        if !(self.using_item && self.use_slot == self.held && self.use_count > 0 && !self.use_moved) {
            self.use_slot = self.held;
            self.use_count = ticks;
            self.use_moved = false;
        }
        self.set_using(true);
    }

    /// `isBlocking()`: the item in use is the sword.
    #[inline]
    pub fn blocking(&self) -> bool {
        self.using_item && (self.autoblock || self.use_slot == SLOT_SWORD)
    }

    /// `addPotionEffect` for Speed, with what `EntityPlayerMP` sends: a new
    /// effect, or one combined with the Speed there is. Either way the
    /// modifier of the speed attribute is set again, which sends the
    /// attribute to everybody.
    pub fn add_speed(&mut self, amp: i8, ticks: i32, shown: bool) -> S2C {
        if self.speed_ticks > 0 {
            // PotionEffect.combine
            if amp > self.l.attr.speed_amp {
                self.l.attr.speed_amp = amp;
                self.speed_ticks = ticks;
            } else if amp == self.l.attr.speed_amp && self.speed_ticks < ticks {
                self.speed_ticks = ticks;
            }
        } else {
            self.l.attr.speed_amp = amp;
            self.speed_ticks = ticks;
        }
        self.speed_shown = shown;
        self.attr_dirty = true;
        S2C::Effect { amp: self.l.attr.speed_amp, ticks: self.speed_ticks.min(EFFECT_TICKS_SENT), shown }
    }

    /// `EntityLivingBase.setHealth`: between nothing and twenty, and a
    /// watched value, so a change is sent to everybody.
    pub fn set_health(&mut self, h: f32) {
        let h = h.clamp(0.0f32, MAX_HEALTH);
        if h.to_bits() != self.health.to_bits() {
            if h > self.health {
                self.health_gained += h - self.health;
            } else {
                self.health_lost += self.health - h;
            }
            self.health = h;
            self.health_dirty = true;
        }
    }

    #[inline]
    pub fn alive(&self) -> bool {
        self.health > 0.0
    }

    /// `EntityPlayer.addMovementStat` for a player on dry land: walking and
    /// running along the ground make hungry, by the hundredth of a block.
    fn movement_stat(&mut self, dx: f64, dz: f64) {
        if self.l.body.on_ground {
            let k = mth::round_f(mth::sqrt_d(dx * dx + dz * dz) * 100.0f32);
            if k > 0 {
                if self.l.sprinting {
                    self.food.add_exhaustion(0.099999994f32 * k as f32 * 0.01f32);
                } else {
                    self.food.add_exhaustion(0.01f32 * k as f32 * 0.01f32);
                }
            }
        }
    }

    /// `EntityLivingBase.heal`.
    pub fn heal(&mut self, amount: f32) {
        if self.health > 0.0 {
            self.set_health(self.health + amount);
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
    /// Ender pearls and potions in flight, oldest first.
    pub thrown: Vec<Thrown>,
    pub next_thrown_id: u32,
    /// Pearls that moved their thrower since the caller last drained this.
    pub landings: Vec<Landing>,
    /// Potions that broke since the caller last drained this.
    pub splashes: Vec<Splash>,
    /// Pearls thrown, per player, and potions.
    pub throws: [u32; 2],
    pub potion_throws: [u32; 2],
    /// Snowballs and eggs thrown and hooks cast, per player.
    pub tool_uses: [u32; 2],
    /// Snowballs, eggs, hooks and pearls that hit a player and counted,
    /// since the caller last drained this list.
    pub thrown_hits: Vec<ThrownHit>,
    /// Replaying a recording: the hook's `rand.nextFloat()` draws, which
    /// slow it where it lies on a block.
    pub hook_floats: VecDeque<f32>,
    /// Draws for the random direction of a knockback whose source stands
    /// where the player is, taken before the generator's: a replay fills it
    /// with the game's.
    pub kb_randoms: VecDeque<f64>,
    /// Replaying a recording: the exact motion of the next pearls and
    /// potions thrown, which in the game has a random part.
    pub throw_motions: VecDeque<V3>,
    /// Replaying a recording: every pearl and potion as it was when thrown.
    pub spawned: Vec<Thrown>,
    /// Replaying a recording: pearls and potions the game did not update on
    /// this tick. It leaves out entities close to a part of the world that
    /// is not loaded, which only happens to one that got out of the scene.
    pub frozen: Vec<u32>,
    /// What servers add against pearl glitches: a pearl that would put its
    /// thrower inside a block is lost instead. The plain game has no such
    /// rule, and a pearl that has gone through its own thrower into the
    /// ground does put the thrower there.
    pub pearl_guard: bool,
    /// Replaying a recording: the generator the game drew the random part
    /// of Protection from, at the point the recording starts.
    pub ench_rand: Option<JavaRandom>,
    /// A hit by someone else than the other player, made on the next tick:
    /// the player hit, the damage before the block and the armour, and the
    /// horizontal direction from that player to whoever hits it.
    pub gank: Option<(usize, f32, [f64; 2])>,
    /// A hit that takes this much health at once from this player, past its
    /// armour and its invulnerable ticks, with a knockback along this
    /// direction: Strength or a player it does not see, as sudden as it
    /// gets. Dealt at the start of the next tick.
    pub sudden: Option<(usize, f32, [f64; 2])>,
    /// A server that hides health: what player `i` is sent as the health
    /// of the other (`Params::opp_hp_hide_prob`), None for the real one.
    pub health_shown: [Option<f32>; 2],
}

/// Packets produced by the server: (destination client, packet).
pub type Outbox = Vec<(usize, S2C)>;

impl Server {
    pub fn new(p0: ServerPlayer, p1: ServerPlayer, kb: KbProfile, cfg: ServerCfg) -> Self {
        Server {
            p: [p0, p1],
            kb,
            plugin: None,
            cfg,
            hits: Vec::with_capacity(4),
            attacks_received: [0; 2],
            thrown: Vec::new(),
            next_thrown_id: 0,
            landings: Vec::new(),
            splashes: Vec::new(),
            throws: [0; 2],
            potion_throws: [0; 2],
            tool_uses: [0; 2],
            thrown_hits: Vec::new(),
            hook_floats: VecDeque::new(),
            kb_randoms: VecDeque::new(),
            throw_motions: VecDeque::new(),
            spawned: Vec::new(),
            frozen: Vec::new(),
            gank: None,
            sudden: None,
            pearl_guard: false,
            ench_rand: None,
            health_shown: [None; 2],
        }
    }

    /// A packet handler, run at the start of a server tick in arrival order.
    pub fn handle(&mut self, from: usize, pkt: C2S, world: &World, rng: &mut Rng, out: &mut Outbox) {
        match pkt {
            C2S::Swing => self.swing(from, out),
            C2S::Attack => self.use_entity_attack(from, world, rng, out),
            C2S::Sprint(s) => {
                self.p[from].set_sprinting(s);
                self.p[from].client_sprint = s;
            }
            C2S::Player { pos, rot, on_ground } => self.process_player(from, pos, rot, on_ground, world, rng, out),
            // processUseEntity for the two interactions: nothing comes of
            // them whatever is in hand.
            C2S::InteractAt | C2S::Interact => {}
            // processPlayerBlockPlacement on a block: nothing in the kit
            // does anything to it.
            C2S::UseBlock(said) => self.after_use(from, said, out),
            // With nothing in hand the handler stops at once.
            C2S::UseItem(said) => {
                if self.p[from].in_hand().0 != Item::Empty {
                    self.use_item(from, rng, out);
                    self.after_use(from, said, out);
                }
            }
            // processPlayerDigging RELEASE_USE_ITEM -> stopUsingItem
            C2S::ReleaseUse => self.p[from].set_using(false),
            // processHeldItemChange
            C2S::HeldItem(slot) => {
                if slot < 9 {
                    self.p[from].held = slot;
                }
            }
            // processClientStatus OPEN_INVENTORY_ACHIEVEMENT counts an
            // achievement, and processCloseWindow on the player's own
            // window drops what is on the cursor, which is nothing.
            C2S::OpenInv | C2S::CloseInv => {}
            C2S::Click { slot, said, action } => self.click(from, slot, said, action, out),
            // processConfirmTransaction
            C2S::Confirm { action } => {
                let p = &mut self.p[from];
                if !p.can_click && p.refused == action {
                    p.can_click = true;
                }
            }
        }
    }

    /// `processClickWindow` for a shift click. The move is made, and if it
    /// came out as the client said it is confirmed and the server's copy
    /// of the inventory brought up to date without sending anything, which
    /// loses whatever was about to be sent again. If not, the client is
    /// told so and sent the whole inventory, and no click counts until it
    /// has answered.
    fn click(&mut self, i: usize, slot: u8, said: Option<(Item, u8)>, action: u16, out: &mut Outbox) {
        let p = &mut self.p[i];
        if !p.can_click {
            return;
        }
        let (mut bar, before, rows) = (p.bar(), p.bar(), p.rows);
        let got = shift_click(&mut bar, &mut p.rows, slot);
        p.set_bar(bar);
        if got.is_some() {
            p.clicks_done += 1;
            if p.using_item && bar.stack(p.use_slot) != before.stack(p.use_slot) {
                p.use_moved = true;
            }
        }
        if got == said {
            out.push((i, S2C::Confirm { action, ok: true }));
            p.dirty = 0;
        } else {
            for k in 1..9u8 {
                if bar.stack(k) != before.stack(k) {
                    p.dirty |= 1 << k;
                }
            }
            for k in 0..27 {
                if p.rows.0[k] != rows.0[k] {
                    p.dirty |= 1 << (9 + k);
                }
            }
            p.refused = action;
            p.can_click = false;
            out.push((i, S2C::Confirm { action, ok: false }));
            out.push((i, S2C::WindowItems(p.bar(), p.rows)));
            out.push((i, S2C::Cursor));
        }
    }

    /// The end of `processPlayerBlockPlacement`. Unless the stack in hand
    /// is one that takes time to use (a sword, a potion, food), the handler
    /// brings its copy of the inventory up to date without sending it,
    /// which loses anything that was about to be sent again, and then sends
    /// the slot in hand if it is not what the client said it held.
    fn after_use(&mut self, i: usize, said: (Item, u8), out: &mut Outbox) {
        let p = &mut self.p[i];
        let (item, n) = p.in_hand();
        if item.takes_time() {
            return;
        }
        p.dirty = 0;
        if (item, n as u8) != said {
            out.push((i, S2C::SetSlot { slot: p.held, item, count: n as u8 }));
        }
    }

    /// `processPlayerBlockPlacement` with direction 255: use what is in
    /// hand. The sword blocks; a pearl or a splash potion is thrown; a
    /// drink or a meal begins.
    fn use_item(&mut self, i: usize, rng: &mut Rng, out: &mut Outbox) {
        match self.p[i].in_hand().0 {
            // ItemGlassBottle.onItemRightClick finds no water.
            Item::Empty | Item::Bottle => {}
            // tryUseItem -> ItemSword.onItemRightClick -> setItemInUse
            Item::Sword => self.p[i].start_use(BLOCK_TICKS),
            // ItemPotion.onItemRightClick for a potion to drink.
            Item::Drink => self.p[i].start_use(USE_TICKS),
            // ItemFood.onItemRightClick: only with room for food. Without,
            // tryUseItem sends the whole inventory, as for any stack that
            // takes time to use and is not in use after the click.
            Item::Food => {
                if self.p[i].food.level < 20 {
                    self.p[i].start_use(USE_TICKS);
                } else if !self.p[i].using_item {
                    out.push((i, S2C::WindowItems(self.p[i].bar(), self.p[i].rows)));
                    out.push((i, S2C::Cursor));
                }
            }
            Item::Pearl => {
                if self.cfg.pearl_cooldown > 0 && !self.p[i].no_pearl_cd && self.p[i].pearl_wait > 0 {
                    // The plugin cancels the click and sends the inventory
                    // again, so that the client gets its pearl back.
                    out.push((i, S2C::WindowItems(self.p[i].bar(), self.p[i].rows)));
                    out.push((i, S2C::Cursor));
                    return;
                }
                self.p[i].pearl_wait = if self.p[i].no_pearl_cd { 0 } else { self.cfg.pearl_cooldown };
                // ItemEnderPearl.onItemRightClick
                self.p[i].pearls -= 1;
                self.throws[i] += 1;
                self.spawn_thrown(i, Kind::Pearl, rng, out);
                // tryUseItem: the stack changed, so the whole inventory is
                // sent, unless the server still has the player using an
                // item.
                if !self.p[i].using_item {
                    out.push((i, S2C::WindowItems(self.p[i].bar(), self.p[i].rows)));
                    out.push((i, S2C::Cursor));
                }
            }
            Item::Potion => {
                // ItemPotion.onItemRightClick for a splash potion: the
                // stack of one is used up, and the rest goes as for a pearl.
                self.p[i].potions &= !(1 << self.p[i].held);
                self.potion_throws[i] += 1;
                self.spawn_thrown(i, Kind::Potion, rng, out);
                if !self.p[i].using_item {
                    out.push((i, S2C::WindowItems(self.p[i].bar(), self.p[i].rows)));
                    out.push((i, S2C::Cursor));
                }
            }
            // ItemSnowball / ItemEgg.onItemRightClick: one off the stack,
            // and the rest as for a pearl.
            Item::Snowball | Item::Egg => {
                let kind = if self.p[i].tool.item == Item::Egg { Kind::Egg } else { Kind::Snowball };
                let t = &mut self.p[i].tool;
                t.n -= 1;
                if t.n == 0 {
                    *t = Tool::default();
                }
                self.tool_uses[i] += 1;
                self.spawn_thrown(i, kind, rng, out);
                if !self.p[i].using_item {
                    out.push((i, S2C::WindowItems(self.p[i].bar(), self.p[i].rows)));
                    out.push((i, S2C::Cursor));
                }
            }
            // ItemFishingRod.onItemRightClick: with the hook out, it comes
            // back (`handleHookRetraction`) and the rod wears by what that
            // returns; without, a hook is cast. Either way the arm swings.
            Item::Rod => {
                if self.p[i].fish.is_some() {
                    let wear = self.retract(i);
                    let t = &mut self.p[i].tool;
                    if wear > 0 {
                        // damageItem: the rod breaks once worn past its most.
                        let worn = t.wear as u32 + wear;
                        if worn > ROD_MAX_WEAR as u32 {
                            *t = Tool::default();
                        } else {
                            t.wear = worn as u8;
                        }
                        // tryUseItem: the stack changed.
                        if !self.p[i].using_item {
                            out.push((i, S2C::WindowItems(self.p[i].bar(), self.p[i].rows)));
                            out.push((i, S2C::Cursor));
                        }
                    }
                } else {
                    self.tool_uses[i] += 1;
                    self.spawn_thrown(i, Kind::Hook, rng, out);
                }
                self.swing(i, out);
            }
        }
    }

    /// `EntityLivingBase.swingItem` on the server: the other player is
    /// told, unless the arm is in the first half of a swing already.
    fn swing(&mut self, i: usize, out: &mut Outbox) {
        let p = &mut self.p[i];
        if !p.swing_in_progress || p.swing_progress_int >= 3 || p.swing_progress_int < 0 {
            p.swing_progress_int = -1;
            p.swing_in_progress = true;
            out.push((1 - i, S2C::Swing));
        }
    }

    /// `EntityFishHook.handleHookRetraction` for player `i`'s hook: a
    /// player it has caught is pulled toward the angler, on the server only
    /// (nothing tells that player's client, so all that comes of it is the
    /// motion the server has for the player at its next knockback). The
    /// hook goes. Returns the wear: 3 with a player caught, 2 if it lay on
    /// a block, 0 otherwise.
    fn retract(&mut self, i: usize) -> u32 {
        let Some(id) = self.p[i].fish.take() else { return 0 };
        let Some(k) = self.thrown.iter().position(|e| e.id == id && !e.dead) else { return 0 };
        let (pos, hook) = (self.thrown[k].pos, self.thrown[k].hook.clone());
        let mut wear = 0;
        if let Some(h) = &hook {
            if let Some(v) = h.caught {
                let a = self.p[i].l.body.pos;
                let (d0, d2, d4) = (a[0] - pos[0], a[1] - pos[1], a[2] - pos[2]);
                let d6 = mth::sqrt_d(d0 * d0 + d2 * d2 + d4 * d4) as f64;
                let d8 = 0.1f64;
                let m = &mut self.p[v].l.body.motion;
                m[0] += d0 * d8;
                m[1] += d2 * d8 + mth::sqrt_d(d6) as f64 * 0.08;
                m[2] += d4 * d8;
                wear = 3;
            }
            if h.in_ground {
                wear = 2;
            }
        }
        self.thrown[k].dead = true;
        wear
    }

    /// `EntityPlayerMP.onItemUseFinish`: the player is told, the potion
    /// gives its Speed and leaves a bottle, or the steak is eaten.
    fn finish_use(&mut self, i: usize, out: &mut Outbox) {
        let p = &mut self.p[i];
        let slot = p.held;
        match p.in_hand().0 {
            Item::Drink => {
                out.push((i, S2C::UseDone));
                p.drinks &= !(1 << slot);
                p.bottles |= 1 << slot;
                p.dirty |= 1 << slot;
                p.drinks_done += 1;
                let e = p.add_speed(DRINK_AMP, DRINK_TICKS, DRINK_SHOWN);
                out.push((i, e));
            }
            Item::Food => {
                out.push((i, S2C::UseDone));
                p.steak -= 1;
                p.dirty |= 1 << slot;
                p.meals_done += 1;
                // FoodStats.addStats
                p.food.level = (STEAK_FOOD + p.food.level).min(20);
                p.food.saturation = (p.food.saturation + STEAK_FOOD as f32 * STEAK_SATURATION * 2.0f32).min(p.food.level as f32);
            }
            _ => return,
        }
        p.set_using(false);
    }

    /// `new EntityEnderPearl(world, thrower)` or `new EntityPotion(world,
    /// thrower, stack)` and `spawnEntityInWorld`, which makes the entity
    /// tracker tell both players at once.
    fn spawn_thrown(&mut self, i: usize, kind: Kind, rng: &mut Rng, out: &mut Outbox) {
        let p = &self.p[i];
        let (pos, mut m) = throw_heading(kind, p.l.body.pos, p.l.yaw, p.l.pitch);
        let spread = THROW_SPREAD * 1.0f32 as f64;
        let speed = kind.speed() as f64;
        let aim = [m[0] * speed, m[1] * speed, m[2] * speed];
        for v in m.iter_mut() {
            *v += rng.gaussian() * spread;
            *v *= speed;
        }
        let replaying = !self.throw_motions.is_empty();
        if let Some(exact) = self.throw_motions.pop_front() {
            m = exact;
        }
        let f1 = mth::sqrt_d(m[0] * m[0] + m[2] * m[2]);
        let pyaw = (mth::atan2(m[0], m[2]) * 180.0 / std::f64::consts::PI) as f32;
        let ppitch = (mth::atan2(m[1], f1 as f64) * 180.0 / std::f64::consts::PI) as f32;
        // A newer server's thrown things take on their thrower's movement;
        // a 1.8 hook never does.
        if self.cfg.throw_carry && !replaying && kind != Kind::Hook {
            m[0] += p.moved[0];
            m[2] += p.moved[2];
            if !p.l.body.on_ground {
                m[1] += p.moved[1];
            }
        }
        let id = self.next_thrown_id;
        self.next_thrown_id += 1;
        let e = Thrown {
            id,
            kind,
            thrower: i,
            pos,
            motion: m,
            yaw: pyaw,
            pitch: ppitch,
            prev_yaw: pyaw,
            prev_pitch: ppitch,
            ticks_in_air: 0,
            dead: false,
            enc_pos: [mth::floor_d(pos[0] * 32.0), mth::floor_d(pos[1] * 32.0), mth::floor_d(pos[2] * 32.0)],
            enc_yaw: mth::floor_f(pyaw * 256.0f32 / 360.0f32),
            enc_pitch: mth::floor_f(ppitch * 256.0f32 / 360.0f32),
            update_counter: 0,
            ticks_since_teleport: 0,
            tracker_on_ground: false,
            last_motion: m,
            aim,
            hook: None,
        };
        let mut e = e;
        if kind == Kind::Hook {
            // new EntityFishHook(world, angler): angler.fishEntity = this.
            let mut body = crate::body::Body::new(pos);
            body.bb = hook_box(pos);
            body.motion = m;
            e.hook = Some(Hook { body, in_ground: false, caught: None });
            self.p[i].fish = Some(id);
        }
        if replaying {
            self.spawned.push(e.clone());
        }
        // EntityTrackerEntry.updatePlayerEntity for each player.
        for dest in 0..2 {
            let own = kind == Kind::Hook && dest == i;
            out.push((dest, S2C::Thrown { id, ev: ThrownEv::Spawn { kind, p: e.enc_pos, yaw: e.enc_yaw as i8, pitch: e.enc_pitch as i8, own } }));
            out.push((dest, S2C::Thrown { id, ev: ThrownEv::Meta }));
            out.push((dest, S2C::Thrown { id, ev: ThrownEv::Velocity(encode_velocity(m)) }));
        }
        self.thrown.push(e);
    }

    /// `EntityThrowable.onUpdate` with `EntityEnderPearl.onImpact` or
    /// `EntityPotion.onImpact` for the pearl or potion at index `k`.
    fn thrown_update(&mut self, k: usize, world: &World, rng: &mut Rng, out: &mut Outbox) {
        if self.thrown[k].kind == Kind::Hook {
            self.hook_update(k, world, rng, out);
            return;
        }
        let e = &mut self.thrown[k];
        // Entity.onEntityUpdate
        e.prev_yaw = e.yaw;
        e.prev_pitch = e.pitch;
        if e.pos[1] < -64.0 {
            e.dead = true;
            return;
        }
        e.ticks_in_air += 1;
        let vec3 = e.pos;
        let mut vec31 = [e.pos[0] + e.motion[0], e.pos[1] + e.motion[1], e.pos[2] + e.motion[2]];
        let mut hit_block = false;
        if let RayHit::Block(h) = world.ray_trace_blocks(vec3, vec31, false) {
            vec31 = h;
            hit_block = true;
        }
        // The players the path comes near. The box of a pearl or potion is
        // a quarter of a block wide and high.
        let w = (0.25f32 / 2.0f32) as f64;
        let bb = Aabb::new([e.pos[0] - w, e.pos[1], e.pos[2] - w], [e.pos[0] + w, e.pos[1] + 0.25f32 as f64, e.pos[2] + w]);
        let query = bb.add_coord(e.motion[0], e.motion[1], e.motion[2]).expand(1.0, 1.0, 1.0);
        let (thrower, ticks_in_air, kind) = (e.thrower, e.ticks_in_air, e.kind);
        let mut hit_player: Option<usize> = None;
        let mut d0 = 0.0f64;
        for j in 0..2 {
            let q = &self.p[j];
            if !filed_near(&query, q.chunk) || !q.l.body.bb.intersects(&query) {
                continue;
            }
            if j == thrower && ticks_in_air < 5 {
                continue;
            }
            let f = 0.3f32 as f64;
            if let Some(h) = q.l.body.bb.expand(f, f, f).calculate_intercept(vec3, vec31) {
                let d1 = dist_sq(vec3, h);
                if d1 < d0 || d0 == 0.0 {
                    hit_player = Some(j);
                    d0 = d1;
                }
            }
        }
        if kind == Kind::Potion {
            // EntityPotion.onImpact: it breaks on a block and on any player,
            // its thrower too. Every player whose box is within four blocks
            // to the sides and two up or down of the potion's own, and whose
            // feet are less than four blocks from where the potion was at
            // the start of this tick, gets a share that falls with that
            // distance. A player it hit gets all of it.
            if hit_player.is_some() || hit_block {
                let at = self.thrown[k].pos;
                let area = bb.expand(4.0, 2.0, 4.0);
                let mut sp = Splash { thrower, pos: at, on_player: hit_player, ticks: ticks_in_air, offered: [0.0; 2], healed: [0.0; 2], would: [0.0; 2] };
                for j in 0..2 {
                    let q = &self.p[j];
                    if !filed_near(&area, q.chunk) || !q.l.body.bb.intersects(&area) {
                        continue;
                    }
                    let i = splash_heal(at, q.l.body.pos, hit_player == Some(j));
                    if dist_sq(at, q.l.body.pos) < 16.0 {
                        let before = self.p[j].health;
                        let strict = j == thrower && i < POTION_HEAL && self.cfg.potion_strict < 1.0;
                        self.p[j].heal(if strict { i * self.cfg.potion_strict } else { i });
                        sp.offered[j] = i;
                        sp.healed[j] = self.p[j].health - before;
                        sp.would[j] = if strict { i.min((MAX_HEALTH - before).max(0.0)) } else { sp.healed[j] };
                    }
                }
                self.splashes.push(sp);
                self.thrown[k].dead = true;
            }
        }
        // EntitySnowball / EntityEgg.onImpact: whatever it hits, it breaks;
        // a player it hits is hurt for nothing, its thrower too (an egg's
        // chickens are left out: duel servers do not let them hatch).
        if matches!(kind, Kind::Snowball | Kind::Egg) && (hit_player.is_some() || hit_block) {
            if let Some(j) = hit_player {
                self.zero_hit(j, thrower, kind, rng, out);
            }
            self.thrown[k].dead = true;
        }
        // EntityEnderPearl.onImpact. A pearl goes through its own thrower,
        // and while it does it does not notice a block either.
        let impact = kind == Kind::Pearl
            && match hit_player {
                Some(j) => j != thrower,
                None => hit_block,
            };
        if impact {
            // EntityEnderPearl.onImpact: the player it hit is hurt for
            // nothing first, then the thrower is moved.
            if let Some(j) = hit_player {
                self.zero_hit(j, thrower, kind, rng, out);
            }
            let to = self.thrown[k].pos;
            let (bx, by, bz) = (mth::floor_d(to[0]), mth::floor_d(to[1]), mth::floor_d(to[2]));
            let inside = world.solid(bx, by, bz) || world.solid(bx, by + 1, bz);
            if !(self.pearl_guard && inside) {
                let (yaw, pitch) = (self.p[thrower].l.yaw, self.p[thrower].l.pitch);
                self.set_player_location(thrower, to, yaw, pitch, out);
                self.p[thrower].l.body.fall_distance = 0.0;
                let before = self.p[thrower].health;
                self.attack_entity_from(thrower, PEARL_DAMAGE, Source::Fall, rng, out);
                let dealt = before - self.p[thrower].health;
                self.landings.push(Landing { thrower, pos: to, on_player: hit_player.is_some(), ticks: ticks_in_air, dealt });
            }
            self.thrown[k].dead = true;
        }
        let e = &mut self.thrown[k];
        for a in 0..3 {
            e.pos[a] += e.motion[a];
        }
        let f1 = mth::sqrt_d(e.motion[0] * e.motion[0] + e.motion[2] * e.motion[2]);
        e.yaw = (mth::atan2(e.motion[0], e.motion[2]) * 180.0 / std::f64::consts::PI) as f32;
        e.pitch = (mth::atan2(e.motion[1], f1 as f64) * 180.0 / std::f64::consts::PI) as f32;
        while e.pitch - e.prev_pitch < -180.0f32 {
            e.prev_pitch -= 360.0f32;
        }
        while e.pitch - e.prev_pitch >= 180.0f32 {
            e.prev_pitch += 360.0f32;
        }
        while e.yaw - e.prev_yaw < -180.0f32 {
            e.prev_yaw -= 360.0f32;
        }
        while e.yaw - e.prev_yaw >= 180.0f32 {
            e.prev_yaw += 360.0f32;
        }
        e.pitch = e.prev_pitch + (e.pitch - e.prev_pitch) * 0.2f32;
        e.yaw = e.prev_yaw + (e.yaw - e.prev_yaw) * 0.2f32;
        let (f2, f3) = (0.99f32 as f64, e.kind.gravity() as f64);
        for a in 0..3 {
            e.motion[a] *= f2;
        }
        e.motion[1] -= f3;
    }

    /// A thrown thing of `kind` by `thrower` hits player `v`:
    /// `attackEntityFrom(causeThrownDamage(..), 0)`, which counts only on a
    /// server that lets hits for no damage through. Whether it counted.
    fn zero_hit(&mut self, v: usize, thrower: usize, kind: Kind, rng: &mut Rng, out: &mut Outbox) -> bool {
        let counted = self.attack_entity_from(v, 0.0, Source::Thrown(thrower), rng, out);
        if counted {
            self.thrown_hits.push(ThrownHit { kind, thrower, victim: v });
        }
        counted
    }

    /// `EntityFishHook.onUpdate` on the server, out of water, for the hook
    /// at index `k`. It goes when its angler no longer holds the rod, is
    /// dead or is more than 32 blocks away. While it has a player caught
    /// it sits on that player, at four fifths of its height. Otherwise it
    /// flies as anything thrown does against blocks and players, but moves
    /// through the world as an entity (`moveEntity`), slowed to 0.92 of its
    /// speed each tick in the air and 0.5 on the ground or against a wall,
    /// and falling by 0.04. On a block it stops; on the next tick it comes
    /// loose with a fifth of its speed or less, since the game never notes
    /// which block it lies on, and its ticks in the air start over.
    fn hook_update(&mut self, k: usize, world: &World, rng: &mut Rng, out: &mut Outbox) {
        {
            let e = &mut self.thrown[k];
            e.prev_yaw = e.yaw;
            e.prev_pitch = e.pitch;
            if e.pos[1] < -64.0 {
                e.dead = true;
                return;
            }
        }
        let angler = self.thrown[k].thrower;
        let a = &self.p[angler];
        if a.health <= 0.0 || a.in_hand().0 != Item::Rod || dist_sq(self.thrown[k].pos, a.l.body.pos) > 1024.0 {
            self.thrown[k].dead = true;
            return;
        }
        let mut hk = self.thrown[k].hook.take().expect("a hook without its state");
        if let Some(v) = hk.caught {
            let b = &self.p[v].l.body;
            // Position only: the box stays where the hook last moved it.
            self.thrown[k].pos = [b.pos[0], b.bb.min[1] + 1.8f32 as f64 * 0.8, b.pos[2]];
            self.thrown[k].hook = Some(hk);
            return;
        }
        let e = &mut self.thrown[k];
        if hk.in_ground {
            hk.in_ground = false;
            for a in 0..3 {
                let r = self.hook_floats.pop_front().unwrap_or_else(|| rng.f64() as f32);
                e.motion[a] *= (r * 0.2f32) as f64;
            }
            e.ticks_in_air = 0;
        } else {
            e.ticks_in_air += 1;
        }
        let vec31 = e.pos;
        let mut vec3 = [e.pos[0] + e.motion[0], e.pos[1] + e.motion[1], e.pos[2] + e.motion[2]];
        let mut hit_block = false;
        if let RayHit::Block(h) = world.ray_trace_blocks(vec31, vec3, false) {
            vec3 = h;
            hit_block = true;
        }
        let query = hk.body.bb.add_coord(e.motion[0], e.motion[1], e.motion[2]).expand(1.0, 1.0, 1.0);
        let ticks_in_air = e.ticks_in_air;
        let mut hit_player: Option<usize> = None;
        let mut d0 = 0.0f64;
        for j in 0..2 {
            let q = &self.p[j];
            if !filed_near(&query, q.chunk) || !q.l.body.bb.intersects(&query) {
                continue;
            }
            if j == angler && ticks_in_air < 5 {
                continue;
            }
            let f = 0.3f32 as f64;
            if let Some(h) = q.l.body.bb.expand(f, f, f).calculate_intercept(vec31, vec3) {
                let d1 = dist_sq(vec31, h);
                if d1 < d0 || d0 == 0.0 {
                    hit_player = Some(j);
                    d0 = d1;
                }
            }
        }
        if let Some(j) = hit_player {
            if self.zero_hit(j, angler, Kind::Hook, rng, out) {
                hk.caught = Some(j);
            }
        } else if hit_block {
            hk.in_ground = true;
        }
        let e = &mut self.thrown[k];
        if !hk.in_ground {
            let b = &mut hk.body;
            b.motion = e.motion;
            b.move_entity_step(world, e.motion[0], e.motion[1], e.motion[2], false, 0.0);
            e.motion = b.motion;
            e.pos = b.pos;
            let f5 = mth::sqrt_d(e.motion[0] * e.motion[0] + e.motion[2] * e.motion[2]);
            e.yaw = (mth::atan2(e.motion[0], e.motion[2]) * 180.0 / std::f64::consts::PI) as f32;
            e.pitch = (mth::atan2(e.motion[1], f5 as f64) * 180.0 / std::f64::consts::PI) as f32;
            while e.pitch - e.prev_pitch < -180.0f32 {
                e.prev_pitch -= 360.0f32;
            }
            while e.pitch - e.prev_pitch >= 180.0f32 {
                e.prev_pitch += 360.0f32;
            }
            while e.yaw - e.prev_yaw < -180.0f32 {
                e.prev_yaw -= 360.0f32;
            }
            while e.yaw - e.prev_yaw >= 180.0f32 {
                e.prev_yaw += 360.0f32;
            }
            e.pitch = e.prev_pitch + (e.pitch - e.prev_pitch) * 0.2f32;
            e.yaw = e.prev_yaw + (e.yaw - e.prev_yaw) * 0.2f32;
            let f6 = if b.on_ground || b.collided_h { 0.5f32 } else { 0.92f32 };
            // No water anywhere: d10 is 0, so d11 is -1.
            e.motion[1] += Kind::Hook.gravity() as f64 * -1.0;
            for a in 0..3 {
                e.motion[a] *= f6 as f64;
            }
            b.motion = e.motion;
            b.pos = e.pos;
            b.bb = hook_box(e.pos);
        }
        self.thrown[k].hook = Some(hk);
    }

    /// `EntityTrackerEntry.updatePlayerList` for the thrown thing at index
    /// `k`: update frequency 10 (5 for a hook), with velocity updates, sent
    /// to both players.
    fn thrown_tracker_update(&mut self, k: usize, out: &mut Outbox) {
        let e = &mut self.thrown[k];
        let id = e.id;
        let mut send = |ev: ThrownEv| {
            for dest in 0..2 {
                out.push((dest, S2C::Thrown { id, ev }));
            }
        };
        if e.update_counter % e.kind.track_every() == 0 {
            e.ticks_since_teleport += 1;
            let k0 = mth::floor_d(e.pos[0] * 32.0);
            let j1 = mth::floor_d(e.pos[1] * 32.0);
            let k1 = mth::floor_d(e.pos[2] * 32.0);
            let l1 = mth::floor_f(e.yaw * 256.0f32 / 360.0f32);
            let i2 = mth::floor_f(e.pitch * 256.0f32 / 360.0f32);
            let j2 = k0 - e.enc_pos[0];
            let k2 = j1 - e.enc_pos[1];
            let ii = k1 - e.enc_pos[2];
            let flag = j2.abs() >= 4 || k2.abs() >= 4 || ii.abs() >= 4 || e.update_counter % 60 == 0;
            let flag1 = (l1 - e.enc_yaw).abs() >= 4 || (i2 - e.enc_pitch).abs() >= 4;
            let mut pkt = None;
            let og = e.hook.as_ref().is_some_and(|h| h.body.on_ground);
            if e.update_counter > 0 {
                let in_range = |v: i32| (-128..128).contains(&v);
                if in_range(j2) && in_range(k2) && in_range(ii) && e.ticks_since_teleport <= 400 && e.tracker_on_ground == og {
                    if !flag || !flag1 {
                        if flag {
                            pkt = Some(ThrownEv::RelMove { d: [j2 as i8, k2 as i8, ii as i8], on_ground: og });
                        } else if flag1 {
                            pkt = Some(ThrownEv::Look { yaw: l1 as i8, pitch: i2 as i8, on_ground: og });
                        }
                    } else {
                        pkt = Some(ThrownEv::LookMove { d: [j2 as i8, k2 as i8, ii as i8], yaw: l1 as i8, pitch: i2 as i8, on_ground: og });
                    }
                } else {
                    e.tracker_on_ground = og;
                    e.ticks_since_teleport = 0;
                    pkt = Some(ThrownEv::Teleport { p: [k0, j1, k1], yaw: l1 as i8, pitch: i2 as i8, on_ground: og });
                }
            }
            let d = [e.motion[0] - e.last_motion[0], e.motion[1] - e.last_motion[1], e.motion[2] - e.last_motion[2]];
            let d3 = 0.02f64;
            let d4 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            if d4 > d3 * d3 || (d4 > 0.0 && e.motion == [0.0; 3]) {
                e.last_motion = e.motion;
                send(ThrownEv::Velocity(encode_velocity(e.motion)));
            }
            if let Some(p) = pkt {
                send(p);
            }
            if flag {
                e.enc_pos = [k0, j1, k1];
            }
            if flag1 {
                e.enc_yaw = l1;
                e.enc_pitch = i2;
            }
        }
        e.update_counter += 1;
    }

    /// `NetHandlerPlayServer.setPlayerLocation` with no relative flags.
    fn set_player_location(&mut self, i: usize, pos: V3, yaw: f32, pitch: f32, out: &mut Outbox) {
        let p = &mut self.p[i];
        p.has_moved = false;
        p.last_pos = pos;
        p.moved = [0.0; 3];
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

        self.on_update_entity(i, world, rng, out);

        let hunger = self.cfg.hunger && !self.cfg.infinite_health;
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
            // EntityPlayer.jump
            if hunger {
                p.food.add_exhaustion(if p.l.sprinting { 0.8f32 } else { 0.2f32 });
            }
        }
        p.l.body.move_entity(world, d11, d12, d13, false);
        p.l.body.on_ground = pkt_ground;
        // The vertical difference is thrown away by a condition that is
        // always true in the original.
        let e11 = d[0] - p.l.body.pos[0];
        let e13 = d[2] - p.l.body.pos[2];
        let flag1 = e11 * e11 + 0.0 + e13 * e13 > 0.0625; // "moved wrongly!"
        p.set_position_and_rotation(d, f1, f2);
        if hunger {
            p.movement_stat(d[0] - last[0], d[2] - last[2]);
        }
        let flag2 = !world.collides(&p.l.body.bb.contract(f3, f3, f3));
        if flag && (flag1 || !flag2) {
            self.set_player_location(i, last, f1, f2, out);
            return;
        }
        p.l.body.on_ground = pkt_ground;
        p.moved = [d[0] - last[0], d[1] - last[1], d[2] - last[2]];
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
                        self.attack_entity_from(i, dmg as f32, Source::Fall, rng, out);
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
    fn on_update_entity(&mut self, i: usize, world: &World, rng: &mut Rng, out: &mut Outbox) {
        // EntityPlayer.onUpdate: an item is in use only while it is the one
        // in hand, and a drink or a meal is finished when its time is up.
        if self.p[i].using_item {
            if self.p[i].held == self.p[i].use_slot && !self.p[i].use_moved {
                self.p[i].use_count -= 1;
                if self.p[i].use_count == 0 {
                    self.finish_use(i, out);
                }
            } else {
                self.p[i].set_using(false);
            }
        }
        // EntityLivingBase.onEntityUpdate: a player whose head is inside a
        // block suffocates. A pearl that lands against a ceiling or close
        // to a wall puts its thrower there.
        let hunger = self.cfg.hunger && !self.cfg.infinite_health;
        if inside_opaque_block(world, self.p[i].l.body.pos) {
            self.attack_entity_from(i, 1.0, Source::Wall, rng, out);
        }
        // Fallen into the void: a duel server ends the duel there, long
        // before the game's own floor of the world.
        if self.p[i].l.body.pos[1] < world.void_y {
            self.p[i].health = 0.0;
        }
        {
            let p = &mut self.p[i];
            if p.hurt_time > 0 {
                p.hurt_time -= 1;
            }
            // updatePotionEffects: the Speed runs out, and every thirty
            // seconds of it the player is told how much is left.
            if p.speed_ticks > 0 {
                p.speed_ticks -= 1;
                if p.speed_ticks == 0 {
                    // onFinishedPotionEffect
                    p.l.attr.speed_amp = -1;
                    p.attr_dirty = true;
                    out.push((i, S2C::EffectEnd));
                } else if p.speed_ticks % 600 == 0 {
                    let ticks = p.speed_ticks.min(EFFECT_TICKS_SENT);
                    out.push((i, S2C::Effect { amp: p.l.attr.speed_amp, ticks, shown: p.speed_shown }));
                }
            }
            // updatePotionMetadata
            let fx = p.speed_ticks > 0 && p.speed_shown;
            if fx != p.fx {
                p.fx = fx;
                p.fx_dirty = true;
            }
            // EntityLivingBase.onUpdate: when the stack in hand is not the
            // one last seen, the other player is told, and the attack
            // damage follows the new item from here on.
            let now = p.in_hand();
            if now != p.equip {
                p.equip = now;
                out.push((1 - i, S2C::Equip { item: now.0, n: now.1 }));
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
            let before = p.l.body.pos;
            p.l.living_update(world, 0.0, 0.0, false, false);
            // EntityPlayer.moveEntityWithHeading counts the distance moved.
            if hunger {
                let now = p.l.body.pos;
                p.movement_stat(now[0] - before[0], now[2] - before[2]);
            }
        }
        // collideWithNearbyEntities: exists only on the server in 1.8.
        let j = 1 - i;
        let q = self.p[i].l.body.bb.expand(0.20000000298023224, 0.0, 0.20000000298023224);
        // getEntitiesInAABBexcluding looks through the chunks within two
        // blocks of the box, where a player is filed by its position at the
        // last world tick.
        let filed = filed_near(&q, self.p[j].chunk);
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
        // The end of EntityPlayer.onUpdate, then of
        // EntityPlayerMP.onUpdateEntity.
        if !self.cfg.infinite_health {
            self.food_update(i, rng, out);
        }
        let p = &mut self.p[i];
        let hungry = p.food.saturation == 0.0;
        if p.health != p.last_health || p.last_food != p.food.level || hungry != p.was_hungry {
            out.push((i, S2C::Health { health: p.health, food: p.food.level, saturation: p.food.saturation }));
            p.last_health = p.health;
            p.last_food = p.food.level;
            p.was_hungry = hungry;
        }
    }

    /// `FoodStats.onUpdate` on normal difficulty.
    fn food_update(&mut self, i: usize, rng: &mut Rng, out: &mut Outbox) {
        let hunger = self.cfg.hunger;
        let regen = self.cfg.natural_regen;
        let p = &mut self.p[i];
        if p.food.exhaustion > 4.0f32 {
            p.food.exhaustion -= 4.0f32;
            if p.food.saturation > 0.0 {
                p.food.saturation = (p.food.saturation - 1.0f32).max(0.0f32);
            } else {
                p.food.level = (p.food.level - 1).max(0);
            }
        }
        if regen && p.food.level >= 18 && p.health > 0.0 && p.health < MAX_HEALTH {
            p.food.timer += 1;
            if p.food.timer >= 80 {
                p.heal(1.0);
                if hunger {
                    p.food.add_exhaustion(3.0f32);
                }
                p.food.timer = 0;
            }
        } else if p.food.level <= 0 {
            p.food.timer += 1;
            if p.food.timer >= 80 {
                let starve = p.health > 1.0;
                p.food.timer = 0;
                if starve {
                    self.attack_entity_from(i, 1.0, Source::Starve, rng, out);
                }
            }
        } else {
            p.food.timer = 0;
        }
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
        // generic.attackDamage carries the sword's bonus only once an
        // update has seen the sword in hand; the enchantments are read from
        // what is in hand now.
        if (self.p[v].hurt_resistant_time as f32) > self.cfg.max_no_damage as f32 / 2.0f32 {
            self.p[a].hurt_clicks += 1;
        }
        let sword_now = self.p[a].in_hand().0 == Item::Sword;
        let mut f = if self.p[a].equip.0 == Item::Sword { self.cfg.damage } else { 1.0 };
        if self.p[a].strength >= 0 {
            f = (f as f64 * (1.0 + 1.3 * (self.p[a].strength + 1) as f64)) as f32;
        }
        let f1 = if sword_now { self.cfg.ench_damage } else { 0.0 };
        let ench = if sword_now { self.cfg.kb_ench } else { 0 };
        let sprinting = self.p[a].l.sprinting;
        let i = ench + sprinting as i32;
        if f > 0.0 || f1 > 0.0 {
            let crit = (self.p[a].l.body.fall_distance > 0.0 && !self.p[a].l.body.on_ground) || self.p[a].always_crit;
            if crit && f > 0.0 {
                f *= 1.5;
            }
            f += f1;
            let saved = self.p[v].l.body.motion;
            let n_hits = self.hits.len();
            let flag2 = self.attack_entity_from(v, f, Source::Player(a), rng, out);
            if flag2 {
                // ItemSword.hitEntity -> damageItem, for the item in hand.
                let wear = self.cfg.sword_wear;
                if sword_now && (wear >= 1.0 || (wear > 0.0 && rng.f64() < wear as f64)) {
                    self.p[a].sword_wear += 1;
                    self.p[a].dirty |= 1 << SLOT_SWORD;
                }
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
                // The game adds this at the very end; nothing in between
                // reads it.
                if self.cfg.hunger && !self.cfg.infinite_health {
                    self.p[a].food.add_exhaustion(0.3f32);
                }
                if self.p[v].velocity_changed && self.p[v].no_kb {
                    self.p[v].velocity_changed = false;
                    self.p[v].l.body.motion = saved;
                } else if self.p[v].velocity_changed {
                    let mut m = self.p[v].l.body.motion;
                    let mut sprint_kb = i > 0;
                    if let (Some(k), true) = (self.plugin, self.hits.len() > n_hits) {
                        // The plugin replaces the velocity in the event
                        // the server raises before sending it.
                        let (vel, sp) = self.plugin_velocity(&k, a, v, sprinting, ench, rng);
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
    /// server-side sprint flag as it was when the hit was made, and `ench`
    /// the Knockback level of what it held.
    fn plugin_velocity(&mut self, k: &KbPlugin, a: usize, v: usize, server_sprint: bool, ench: i32, rng: &mut Rng) -> (V3, bool) {
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
        let level = ench + sprint as i32;
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
    fn attack_entity_from(&mut self, v: usize, amount: f32, src: Source, rng: &mut Rng, out: &mut Outbox) -> bool {
        let counts_zero = self.cfg.zero_hits && matches!(src, Source::Thrown(_));
        if self.p[v].health <= 0.0 || (amount == 0.0 && !counts_zero) {
            return false;
        }
        let mut flag = true;
        let mut dealt = (0.0, 0.0);
        let max = self.cfg.max_no_damage;
        if (self.p[v].hurt_resistant_time as f32) > max as f32 / 2.0f32 {
            if amount <= self.p[v].last_damage {
                return false;
            }
            let extra = amount - self.p[v].last_damage;
            self.damage_entity(v, extra, src, rng);
            self.p[v].last_damage = amount;
            flag = false;
        } else {
            self.p[v].last_damage = amount;
            self.p[v].hurt_resistant_time = max;
            dealt = self.damage_entity(v, amount, src, rng);
            self.p[v].hurt_time = 10;
        }
        if flag {
            // setEntityState(this, 2): tracking players first, then self.
            out.push((1 - v, S2C::Hurt { own: false }));
            out.push((v, S2C::Hurt { own: true }));
            // setBeenAttacked
            self.p[v].velocity_changed = true;
            if let Source::Player(a) = src {
                let mut d1 = self.p[a].l.body.pos[0] - self.p[v].l.body.pos[0];
                let mut d0 = self.p[a].l.body.pos[2] - self.p[v].l.body.pos[2];
                while d1 * d1 + d0 * d0 < 1.0E-4 {
                    let mut r = || self.kb_randoms.pop_front().unwrap_or_else(|| rng.f64());
                    d1 = (r() - r()) * 0.01;
                    d0 = (r() - r()) * 0.01;
                }
                self.knock_back(v, d1, d0);
                self.hits.push(Hit {
                    attacker: a,
                    victim: v,
                    sprint: false,
                    crit: false,
                    kb: [0.0; 3],
                    dist: 0.0,
                    blocked: self.p[v].blocking(),
                    damage: dealt.0,
                    dealt: dealt.1,
                });
            } else if let Source::Thrown(a) = src {
                // The knockback of a thrown thing is from its thrower.
                let mut d1 = self.p[a].l.body.pos[0] - self.p[v].l.body.pos[0];
                let mut d0 = self.p[a].l.body.pos[2] - self.p[v].l.body.pos[2];
                while d1 * d1 + d0 * d0 < 1.0E-4 {
                    let mut r = || self.kb_randoms.pop_front().unwrap_or_else(|| rng.f64());
                    d1 = (r() - r()) * 0.01;
                    d0 = (r() - r()) * 0.01;
                }
                self.knock_back(v, d1, d0);
            }
        }
        true
    }

    /// A hit on `v` by a third player standing along `dir`, as the game
    /// makes it: the same invulnerable ticks, block, armour and Protection
    /// as a hit by the other player, the knockback away from `dir`, sent at
    /// once to the player hit as an attacking player's hit sends it. It is
    /// not one of the duel's hits.
    fn gank_hit(&mut self, v: usize, amount: f32, dir: [f64; 2], rng: &mut Rng, out: &mut Outbox) {
        let p = &self.p[v];
        if p.health <= 0.0 || amount <= 0.0 {
            return;
        }
        let saved = p.l.body.motion;
        let max = self.cfg.max_no_damage;
        let src = Source::Player(1 - v);
        if (p.hurt_resistant_time as f32) > max as f32 / 2.0f32 {
            if amount <= p.last_damage {
                return;
            }
            let extra = amount - p.last_damage;
            self.damage_entity(v, extra, src, rng);
            self.p[v].last_damage = amount;
            return;
        }
        self.p[v].last_damage = amount;
        self.p[v].hurt_resistant_time = max;
        self.damage_entity(v, amount, src, rng);
        self.p[v].hurt_time = 10;
        out.push((1 - v, S2C::Hurt { own: false }));
        out.push((v, S2C::Hurt { own: true }));
        self.knock_back(v, dir[0], dir[1]);
        let enc = encode_velocity(self.p[v].l.body.motion);
        out.push((v, S2C::Velocity { own: true, v: enc }));
        self.p[v].velocity_changed = false;
        self.p[v].l.body.motion = saved;
    }

    /// See `sudden`.
    fn sudden_hit(&mut self, v: usize, amount: f32, dir: [f64; 2], out: &mut Outbox) {
        if self.p[v].health <= 0.0 || amount <= 0.0 {
            return;
        }
        let saved = self.p[v].l.body.motion;
        let h = self.p[v].health;
        self.p[v].set_health(h - amount);
        self.p[v].last_damage = self.p[v].last_damage.max(amount);
        self.p[v].hurt_resistant_time = self.cfg.max_no_damage;
        self.p[v].hurt_time = 10;
        out.push((1 - v, S2C::Hurt { own: false }));
        out.push((v, S2C::Hurt { own: true }));
        self.knock_back(v, dir[0], dir[1]);
        let enc = encode_velocity(self.p[v].l.body.motion);
        out.push((v, S2C::Velocity { own: true, v: enc }));
        self.p[v].velocity_changed = false;
        self.p[v].l.body.motion = saved;
    }

    /// `EntityPlayer.damageEntity`: the block, the armour, Protection, then
    /// the health. Returns the damage that is left after the block and the
    /// health that was taken in the end.
    fn damage_entity(&mut self, v: usize, mut amount: f32, src: Source, rng: &mut Rng) -> (f32, f32) {
        let direct = matches!(src, Source::Player(_));
        if direct && self.p[v].blocking() && amount > 0.0 && !(self.cfg.block_fail > 0.0 && rng.chance(self.cfg.block_fail as f64)) {
            amount = (1.0f32 + amount) * 0.5f32;
        }
        let blocked = amount;
        self.p[v].damage_taken += amount;
        if self.cfg.infinite_health || self.p[v].immortal {
            // Resistance V leaves nothing of it.
            return (blocked, 0.0);
        }
        // applyArmorCalculations. The armour's own wear is left out.
        if direct {
            let i = 25 - self.cfg.armor;
            let f = amount * i as f32;
            amount = f / 25.0f32;
        }
        // applyPotionDamageCalculations
        if src != Source::Starve {
            if amount <= 0.0 {
                amount = 0.0;
            } else {
                // EnchantmentHelper.getEnchantmentModifierDamage: half of
                // what the enchantments add up to counts for certain and
                // the other half by chance, drawn anew for every hit.
                let mut m = self.cfg.prot + if src == Source::Fall { self.cfg.feather } else { 0 };
                m = m.clamp(0, 25);
                let bound = (m >> 1) + 1;
                let roll = match self.ench_rand.as_mut() {
                    Some(r) => r.next_int(bound),
                    None => (rng.next_u64() % bound as u64) as i32,
                };
                let mut k = ((m + 1) >> 1) + roll;
                if k > 20 {
                    k = 20;
                }
                if k > 0 {
                    let l = 25 - k;
                    let f1 = amount * l as f32;
                    amount = f1 / 25.0f32;
                }
            }
        }
        if amount != 0.0 {
            // DamageSource.getHungerDamage: nothing for a source that goes
            // past the armour.
            if direct && self.cfg.hunger {
                self.p[v].food.add_exhaustion(0.3f32);
            }
            let before = self.p[v].health;
            self.p[v].set_health(before - amount);
            return (blocked, before - self.p[v].health);
        }
        (blocked, 0.0)
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

    /// The part of a server tick after the packet handlers: the world
    /// update (`EntityPlayerMP.onUpdate` for both players, then every pearl
    /// and potion in the order they were thrown), the entity tracker, then
    /// the network tick.
    pub fn tick(&mut self, world: &World, rng: &mut Rng, out: &mut Outbox) {
        // After the packet handlers: a player still open to a full hit took
        // none on this tick.
        let open = self.cfg.max_no_damage as f32 / 2.0f32;
        for a in 0..2 {
            let v = 1 - a;
            if (self.p[v].hurt_resistant_time as f32) <= open && self.p[v].health > 0.0 && self.p[a].health > 0.0 {
                let e = self.p[a].l.body.pos;
                let eye = [e[0], e[1] + EYE_HEIGHT, e[2]];
                let near = self.p[v].l.body.bb.expand(0.1, 0.1, 0.1).closest_point(eye);
                if dist_sq(eye, near) <= 9.0 {
                    self.p[a].late_ticks += 1;
                }
            }
        }
        if let Some((v, amount, dir)) = self.gank.take() {
            self.gank_hit(v, amount, dir, rng, out);
        }
        if let Some((v, amount, dir)) = self.sudden.take() {
            self.sudden_hit(v, amount, dir, out);
        }
        for (i, p) in self.p.iter_mut().enumerate() {
            if p.hurt_resistant_time > 0 {
                p.hurt_resistant_time -= 1;
            }
            // openContainer.detectAndSendChanges, in the order of the
            // window's slots: the rows, then the hotbar.
            let mut rows = p.dirty >> 9;
            while rows != 0 {
                let k = rows.trailing_zeros() as usize;
                rows &= rows - 1;
                let (item, n) = p.rows.stack(k);
                out.push((i, S2C::SetSlot { slot: 9 + k as u8, item, count: n as u8 }));
            }
            p.dirty &= 0x1ff;
            while p.dirty != 0 {
                let slot = p.dirty.trailing_zeros() as u8;
                p.dirty &= p.dirty - 1;
                let (item, n) = p.stack(slot);
                let count = if slot == SLOT_SWORD { 1 } else { n as u8 };
                out.push((i, S2C::SetSlot { slot, item, count }));
            }
            // destroyedItemsNetCache
            if !p.gone.is_empty() {
                for part in p.gone.chunks(8) {
                    let mut ids = [0u32; 8];
                    ids[..part.len()].copy_from_slice(part);
                    out.push((i, S2C::Destroy { ids, n: part.len() as u8 }));
                }
                p.gone.clear();
            }
            if p.pearl_wait > 0 {
                p.pearl_wait -= 1;
            }
            p.chunk = chunk_of(p.l.body.pos);
        }
        let mut k = 0;
        while k < self.thrown.len() {
            if self.frozen.contains(&self.thrown[k].id) {
                k += 1;
                continue;
            }
            // World.updateEntities: one that died since (a hook that came
            // back) is taken out without its update.
            if !self.thrown[k].dead {
                self.thrown_update(k, world, rng, out);
            }
            if self.thrown[k].dead {
                // World.updateEntities takes a dead entity out at once, and
                // the tracker notes it for both players.
                let id = self.thrown.remove(k).id;
                for p in self.p.iter_mut() {
                    p.gone.push(id);
                    // EntityFishHook.setDead: angler.fishEntity = null.
                    if p.fish == Some(id) {
                        p.fish = None;
                    }
                }
            } else {
                k += 1;
            }
        }
        for i in 0..2 {
            self.tracker_update(i, out);
        }
        for k in 0..self.thrown.len() {
            self.thrown_tracker_update(k, out);
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
        if p.update_counter % 2 == 0 || p.is_airborne || p.meta_dirty || p.health_dirty || p.fx_dirty {
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
            if p.meta_dirty || p.health_dirty || p.fx_dirty {
                let flags = if p.meta_dirty { Some((p.l.sprinting, p.using_item)) } else { None };
                let health = if p.health_dirty { Some(p.health) } else { None };
                let fx = if p.fx_dirty { Some(p.fx) } else { None };
                p.meta_dirty = false;
                p.health_dirty = false;
                p.fx_dirty = false;
                let shown = match self.health_shown[other] {
                    Some(h) => health.map(|_| h),
                    None => health,
                };
                out.push((other, S2C::Meta { own: false, flags, health: shown, fx }));
                out.push((i, S2C::Meta { own: true, flags, health, fx }));
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
