//! The packets a duel with swords, ender pearls, potions and food needs, and
//! a FIFO link with per-packet delay.
use crate::aabb::V3;
use std::collections::VecDeque;

/// What a hotbar slot holds. The sword is in the first slot and the ender
/// pearls in the second. Each of the other seven holds one splash potion of
/// Instant Health II (`Potion`), one potion of Swiftness II to drink
/// (`Drink`), the bottle a drink leaves behind, or nothing; the last of
/// them can hold steak instead (`Food`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Sword,
    Pearl,
    Potion,
    Drink,
    Bottle,
    Food,
    Empty,
}

impl Item {
    /// `getMaxItemUseDuration() > 0`: the items a right click starts using
    /// rather than uses up. Every potion counts, a splash potion too.
    #[inline]
    pub fn takes_time(self) -> bool {
        matches!(self, Item::Sword | Item::Potion | Item::Drink | Item::Food)
    }
}

/// Hotbar slot of the sword, of the pearls and of the steak.
pub const SLOT_SWORD: u8 = 0;
pub const SLOT_PEARL: u8 = 1;
pub const SLOT_FOOD: u8 = 8;
/// `getMaxItemUseDuration` of a potion and of food: the ticks a drink or a
/// meal takes. A sword can be held up for an hour.
pub const USE_TICKS: i32 = 32;
pub const BLOCK_TICKS: i32 = 72000;
/// What a potion of Swiftness II gives: Speed II for a minute and a half.
pub const DRINK_AMP: i8 = 1;
pub const DRINK_TICKS: i32 = 1800;
/// What a steak gives: food and the factor of its saturation.
pub const STEAK_FOOD: i32 = 8;
pub const STEAK_SATURATION: f32 = 0.8;

/// The part of the hotbar that changes during a duel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Bar {
    /// Ender pearls in the second slot.
    pub pearls: u8,
    /// The slots that hold a splash potion, a potion to drink and an empty
    /// bottle, one bit each. Any slot but the sword's can.
    pub potions: u16,
    pub drinks: u16,
    pub bottles: u16,
    /// Steaks in the last slot.
    pub steak: u8,
}

impl Bar {
    /// The stack in a slot other than the sword's: which item and how many.
    #[inline]
    pub fn stack(&self, slot: u8) -> (Item, u32) {
        let bit = |m: u16| m >> slot & 1 != 0;
        match slot {
            s if s > 8 || s == SLOT_SWORD => (Item::Empty, 0),
            _ if bit(self.potions) => (Item::Potion, 1),
            _ if bit(self.drinks) => (Item::Drink, 1),
            _ if bit(self.bottles) => (Item::Bottle, 1),
            SLOT_PEARL if self.pearls > 0 => (Item::Pearl, self.pearls as u32),
            SLOT_FOOD if self.steak > 0 => (Item::Food, self.steak as u32),
            _ => (Item::Empty, 0),
        }
    }

    /// Put a stack into a slot other than the sword's.
    #[inline]
    pub fn put(&mut self, slot: u8, item: Item, count: u8) {
        if slot == SLOT_SWORD || slot > 8 {
            return;
        }
        let bit = 1u16 << slot;
        self.potions &= !bit;
        self.drinks &= !bit;
        self.bottles &= !bit;
        if slot == SLOT_PEARL {
            self.pearls = 0;
        }
        if slot == SLOT_FOOD {
            self.steak = 0;
        }
        if count == 0 {
            return;
        }
        match item {
            Item::Potion => self.potions |= bit,
            Item::Drink => self.drinks |= bit,
            Item::Bottle => self.bottles |= bit,
            Item::Pearl if slot == SLOT_PEARL => self.pearls = count,
            Item::Food if slot == SLOT_FOOD => self.steak = count,
            _ => {}
        }
    }

    /// The first hotbar slot with nothing in it, as a shift click looks
    /// for one: from the left, the sword's slot never being free.
    #[inline]
    pub fn free_slot(&self) -> Option<u8> {
        (1..9u8).find(|&s| self.stack(s).0 == Item::Empty)
    }
}

/// The three rows of the inventory above the hotbar, 27 slots from the top
/// left. A slot holds one splash potion, one potion to drink, a stack of
/// empty bottles, or nothing: 0 nothing, 1 a splash potion, 2 a potion to
/// drink, 2 + n a stack of n bottles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rows(pub [u8; 27]);

impl Default for Rows {
    fn default() -> Self {
        Rows([0; 27])
    }
}

/// `getMaxStackSize` of a glass bottle.
pub const BOTTLE_STACK: u8 = 64;

impl Rows {
    #[inline]
    pub fn stack(&self, i: usize) -> (Item, u32) {
        match self.0[i] {
            0 => (Item::Empty, 0),
            1 => (Item::Potion, 1),
            2 => (Item::Drink, 1),
            n => (Item::Bottle, n as u32 - 2),
        }
    }

    #[inline]
    pub fn put(&mut self, i: usize, item: Item, count: u8) {
        self.0[i] = match item {
            _ if count == 0 => 0,
            Item::Potion => 1,
            Item::Drink => 2,
            Item::Bottle => 2 + count.min(BOTTLE_STACK),
            _ => 0,
        };
    }

    /// How many slots hold this item.
    #[inline]
    pub fn count(&self, item: Item) -> u32 {
        (0..27).filter(|&i| self.stack(i).0 == item).count() as u32
    }

    /// The first slot that holds this item, as a slot of the window (9 on).
    #[inline]
    pub fn first(&self, item: Item) -> Option<u8> {
        (0..27).find(|&i| self.stack(i).0 == item).map(|i| i as u8 + 9)
    }
}

/// Slots of the player's own window: 9 to 35 are the three rows, 36 to 44
/// the hotbar.
pub const WINDOW_ROWS: u8 = 9;
pub const WINDOW_BAR: u8 = 36;

/// The shift click that gets a potion of this kind down to the hotbar, as
/// a slot of the window: on the first one in the rows if the hotbar has a
/// free slot, and if it has none, on the first empty bottle in the hotbar,
/// to put that away first. None when the rows hold no such potion or there
/// is no way to make room for it.
pub fn refill_click(bar: &Bar, rows: &Rows, kind: Item) -> Option<u8> {
    let from = rows.first(kind)?;
    if bar.free_slot().is_some() {
        return Some(from);
    }
    (1..9u8).find(|&s| bar.stack(s).0 == Item::Bottle).map(|s| WINDOW_BAR + s)
}

/// A shift click on a slot of the player's own window: `Container.slotClick`
/// with mode 1, which is `ContainerPlayer.transferStackInSlot`. A stack in
/// the rows goes to the hotbar and a stack in the hotbar to the rows
/// (`mergeItemStack`: onto stacks of the same kind first if it stacks,
/// then into the first free slot from the left). Returns what the click
/// returns, the stack as it was before, or nothing when nothing moved. Both
/// sides run this, the client to show the result at once and the server
/// for real, and the server compares what the client says came of it.
///
/// Potions do not stack, so one goes to the first free slot. A bottle from
/// the hotbar joins the first stack of bottles in the rows with room.
/// Clicks the kit has no use for do nothing here: on the sword, the
/// pearls, the steak, and on bottles in the rows.
pub fn shift_click(bar: &mut Bar, rows: &mut Rows, slot: u8) -> Option<(Item, u8)> {
    if (WINDOW_ROWS..WINDOW_BAR).contains(&slot) {
        let i = (slot - WINDOW_ROWS) as usize;
        let (item, _) = rows.stack(i);
        if !matches!(item, Item::Potion | Item::Drink) {
            return None;
        }
        let to = bar.free_slot()?;
        bar.put(to, item, 1);
        rows.put(i, Item::Empty, 0);
        Some((item, 1))
    } else if (WINDOW_BAR + 1..WINDOW_BAR + 9).contains(&slot) {
        let s = slot - WINDOW_BAR;
        let (item, _) = bar.stack(s);
        match item {
            Item::Potion | Item::Drink => {
                let to = (0..27).find(|&i| rows.0[i] == 0)?;
                rows.put(to, item, 1);
            }
            Item::Bottle => {
                let room = (0..27).find(|&i| matches!(rows.stack(i), (Item::Bottle, n) if n < BOTTLE_STACK as u32));
                match room {
                    Some(i) => rows.0[i] += 1,
                    None => {
                        let to = (0..27).find(|&i| rows.0[i] == 0)?;
                        rows.put(to, Item::Bottle, 1);
                    }
                }
            }
            _ => return None,
        }
        bar.put(s, Item::Empty, 0);
        Some((item, 1))
    } else {
        None
    }
}
/// The hotbar slots that can hold a potion, as bits of the masks that say
/// which of them do.
pub const POTION_SLOTS: u16 = 0b1_1111_1100;

/// The first `n` potion slots, as such a mask.
#[inline]
pub fn potion_mask(n: u8) -> u16 {
    (((1u32 << n.min(7)) - 1) << 2) as u16
}

/// What is thrown: an ender pearl or a splash potion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Pearl,
    Potion,
}

impl Kind {
    /// `getGravityVelocity`.
    #[inline]
    pub fn gravity(self) -> f32 {
        match self {
            Kind::Pearl => 0.03f32,
            Kind::Potion => 0.05f32,
        }
    }
    /// `getVelocity`.
    #[inline]
    pub fn speed(self) -> f32 {
        match self {
            Kind::Pearl => 1.5f32,
            Kind::Potion => 0.5f32,
        }
    }
    /// `getInaccuracy`: degrees added to the pitch for the upward part of
    /// the throw only.
    #[inline]
    pub fn pitch_offset(self) -> f32 {
        match self {
            Kind::Pearl => 0.0f32,
            Kind::Potion => -20.0f32,
        }
    }
}

/// Client to server.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum C2S {
    /// C0A animation.
    Swing,
    /// C02 use entity, action ATTACK, on the other player.
    Attack,
    /// C0B entity action START_SPRINTING / STOP_SPRINTING.
    Sprint(bool),
    /// C03 and its subclasses C04, C05, C06.
    Player { pos: Option<V3>, rot: Option<(f32, f32)>, on_ground: bool },
    /// C02 use entity, action INTERACT_AT, on the other player: the first
    /// thing a right click on a player sends. Nothing comes of it with a
    /// sword in hand.
    InteractAt,
    /// C02 use entity, action INTERACT, the second thing it sends.
    Interact,
    /// C08 block placement on the block under the crosshair, with the
    /// stack the client has in hand (how many of what; a sword counts as
    /// one). Nothing in the kit does anything to a block.
    UseBlock((Item, u8)),
    /// C08 block placement with direction 255: use the held item, again
    /// with the stack the client has in hand. With a sword this starts
    /// blocking.
    UseItem((Item, u8)),
    /// C07 player digging, action RELEASE_USE_ITEM: stop blocking.
    ReleaseUse,
    /// C09 held item change: the hotbar slot now in hand, 0 to 8.
    HeldItem(u8),
    /// C16 client status OPEN_INVENTORY_ACHIEVEMENT: what the key that
    /// opens the inventory sends. The server does nothing a duel shows.
    OpenInv,
    /// C0E click window, mode 1 (shift click) with the left button on a
    /// slot of the player's own window: the stack the client says the
    /// click returned, and the number the client gave the click.
    Click { slot: u8, said: Option<(Item, u8)>, action: u16 },
    /// C0D close window.
    CloseInv,
    /// C0F confirm transaction: the client's answer to a click the server
    /// did not accept.
    Confirm { action: u16 },
}

/// What the server tells a client about a pearl or a potion in flight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ThrownEv {
    /// S0E spawn object, type 65 (pearl) or 73 (potion): position in 1/32
    /// block, angles in 1/256 turn.
    Spawn { kind: Kind, p: [i32; 3], yaw: i8, pitch: i8 },
    /// S1C entity metadata sent with the spawn. It carries nothing.
    Meta,
    /// S12 entity velocity, in 1/8000 block per tick.
    Velocity([i32; 3]),
    /// S15, S16, S17, S18 as for a player.
    RelMove { d: [i8; 3], on_ground: bool },
    Look { yaw: i8, pitch: i8, on_ground: bool },
    LookMove { d: [i8; 3], yaw: i8, pitch: i8, on_ground: bool },
    Teleport { p: [i32; 3], yaw: i8, pitch: i8, on_ground: bool },
}

/// Server to client. `own` means the packet is about the receiving player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum S2C {
    /// S12 entity velocity, in 1/8000 block per tick.
    Velocity { own: bool, v: [i32; 3] },
    /// S19 entity status 2 (hurt animation).
    Hurt { own: bool },
    /// S15 relative move of the other player, in 1/32 block.
    RelMove { d: [i8; 3], on_ground: bool },
    /// S16 look of the other player, in 1/256 turn.
    Look { yaw: i8, pitch: i8, on_ground: bool },
    /// S17 relative move and look.
    LookMove { d: [i8; 3], yaw: i8, pitch: i8, on_ground: bool },
    /// S18 teleport, absolute position in 1/32 block.
    Teleport { p: [i32; 3], yaw: i8, pitch: i8, on_ground: bool },
    /// S1C entity metadata: the watched values that changed. `flags` is the
    /// flags byte as (sprinting, using an item): bit 3 is sprinting and
    /// bit 4 is set while the player uses an item (blocks, drinks, eats).
    /// `health` is the player's health, which every client is told, the
    /// player's own too. `fx` is whether particles of an effect now show
    /// around the player.
    Meta { own: bool, flags: Option<(bool, bool)>, health: Option<f32>, fx: Option<bool> },
    /// S06 update health, to the player itself: health, food and saturation.
    Health { health: f32, food: i32, saturation: f32 },
    /// S2F set slot for a slot of the receiving player: a new stack of
    /// `count` items (0 empties the slot). Slots 0 to 8 are the hotbar and
    /// 9 to 35 the rows above it. The server sends it for the sword when a
    /// hit has cost it durability, for the pearls or the potion after a
    /// throw, and for the bottle or the steak after a drink or a meal.
    SetSlot { slot: u8, item: Item, count: u8 },
    /// S30 window items: the whole inventory again, every stack a new one.
    /// The server sends it after a throw, and after a click it did not
    /// accept.
    WindowItems(Bar, Rows),
    /// S32 confirm transaction: the server took the click with this number
    /// as the client said, or did not.
    Confirm { action: u16, ok: bool },
    /// S19 entity status 9, to the player itself: the drink or the meal is
    /// finished.
    UseDone,
    /// S1D entity effect, to the player itself: Speed of this amplifier for
    /// this many ticks, with or without particles.
    Effect { amp: i8, ticks: i32, shown: bool },
    /// S1E remove entity effect, to the player itself: the Speed is over.
    EffectEnd,
    /// S2F for the stack on the mouse cursor, which follows S30. Nothing
    /// comes of it.
    Cursor,
    /// S04 entity equipment: what the other player now holds. `n` is the
    /// wear of a sword or the size of a stack.
    Equip { item: Item, n: u32 },
    /// Something about the pearl or potion with this entity id.
    Thrown { id: u32, ev: ThrownEv },
    /// S13 destroy entities: these pearls and potions are gone (the first
    /// `n` ids).
    Destroy { ids: [u32; 8], n: u8 },
    /// S20 entity properties for `generic.movementSpeed`.
    Props { own: bool, base: f64, speed_amp: i8, sprint: bool },
    /// S0B animation 0 (arm swing) of the other player.
    Swing,
    /// S08 player position and look with no relative flags: the server
    /// puts the player back where it last accepted it.
    PosLook { pos: V3, yaw: f32, pitch: f32 },
}

/// One direction of a TCP connection: packets keep their order, each one
/// becomes visible at its arrival time (microseconds).
#[derive(Clone, Debug)]
pub struct Link<T> {
    q: VecDeque<(i64, T)>,
    last_arrival: i64,
}

impl<T: Copy> Link<T> {
    pub fn new() -> Self {
        Link { q: VecDeque::with_capacity(32), last_arrival: i64::MIN }
    }
    pub fn clear(&mut self) {
        self.q.clear();
        self.last_arrival = i64::MIN;
    }
    pub fn push(&mut self, arrival: i64, p: T) {
        let a = arrival.max(self.last_arrival);
        self.last_arrival = a;
        self.q.push_back((a, p));
    }
    /// Arrival time of the oldest packet still in flight.
    pub fn peek_time(&self) -> Option<i64> {
        self.q.front().map(|e| e.0)
    }
    pub fn pop_ready(&mut self, now: i64) -> Option<T> {
        match self.q.front() {
            Some(&(t, p)) if t <= now => {
                self.q.pop_front();
                Some(p)
            }
            _ => None,
        }
    }
    /// Pop regardless of time (trace-driven replay).
    pub fn pop_any(&mut self) -> Option<T> {
        self.q.pop_front().map(|e| e.1)
    }
    pub fn len(&self) -> usize {
        self.q.len()
    }
}
