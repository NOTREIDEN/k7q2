//! The packets a duel with swords and ender pearls needs, and a FIFO link
//! with per-packet delay.
use crate::aabb::V3;
use std::collections::VecDeque;

/// What a hotbar slot holds. The sword is in the first slot and the ender
/// pearls in the second; the other seven are empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Sword,
    Pearl,
    Empty,
}

/// Hotbar slot of the sword and of the pearls.
pub const SLOT_SWORD: u8 = 0;
pub const SLOT_PEARL: u8 = 1;

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
    /// C08 block placement on the block under the crosshair. Nothing comes
    /// of it with a sword in hand.
    UseBlock,
    /// C08 block placement with direction 255: use the held item. With a
    /// sword this starts blocking.
    UseItem,
    /// C07 player digging, action RELEASE_USE_ITEM: stop blocking.
    ReleaseUse,
    /// C09 held item change: the hotbar slot now in hand, 0 to 8.
    HeldItem(u8),
}

/// What the server tells a client about an ender pearl in flight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PearlEv {
    /// S0E spawn object, type 65: position in 1/32 block, angles in 1/256 turn.
    Spawn { p: [i32; 3], yaw: i8, pitch: i8 },
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
    /// S1C entity metadata carrying the flags byte: bit 3 is sprinting,
    /// bit 4 is set while the player uses an item (blocks).
    Meta { own: bool, sprinting: bool, using: bool },
    /// S2F set slot for a hotbar slot of the receiving player: a new stack
    /// of `count` items (0 empties the slot). The server sends it for the
    /// sword when a hit has cost it durability, and for the pearls after a
    /// throw.
    SetSlot { slot: u8, count: u8 },
    /// S30 window items: the whole inventory again, every stack a new one.
    /// The server sends it after a throw. `pearls` is how many are left.
    WindowItems { pearls: u8 },
    /// S2F for the stack on the mouse cursor, which follows S30. Nothing
    /// comes of it.
    Cursor,
    /// S04 entity equipment: what the other player now holds. `n` is the
    /// wear of a sword or the size of a stack of pearls.
    Equip { item: Item, n: u32 },
    /// Something about the ender pearl with this entity id.
    Pearl { id: u32, ev: PearlEv },
    /// S13 destroy entities: these pearls are gone (the first `n` ids).
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
