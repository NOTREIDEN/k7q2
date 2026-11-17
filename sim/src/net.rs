//! The packets a sword duel needs, and a FIFO link with per-packet delay.
use crate::aabb::V3;
use std::collections::VecDeque;

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
    /// S2F set slot for the held item of the receiving player. The server
    /// sends it when the sword has lost durability on a hit.
    SetSlot,
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
