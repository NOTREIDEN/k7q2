//! One game client: the local player (`EntityPlayerSP`), the other player
//! as this client sees it (`EntityOtherPlayerMP`), the ender pearls in
//! flight as it sees them, the crosshair pick
//! (`EntityRenderer.getMouseOver`) and one `Minecraft.runTick`.
use crate::aabb::{dist, Aabb, V3, EYE_HEIGHT};
use crate::living::Living;
use crate::mth;
use crate::net::{
    shift_click, Bar, Item, Kind, Rows, ThrownEv, BLOCK_TICKS, C2S, S2C, SLOT_SWORD, STEAK_FOOD, STEAK_SATURATION, USE_TICKS,
};
use crate::world::{RayHit, World};

/// Added to a slot in `Input::inv`: shift-click it and shut the screen.
pub const INV_SHUT: u8 = 64;

/// Keys and mouse for one client tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    /// W = 1, S = -1.
    pub forward: i8,
    /// A (left) = 1, D (right) = -1, as in `MovementInputFromOptions`.
    pub strafe: i8,
    pub jump: bool,
    pub sprint_key: bool,
    /// One full left click (press and release) inside this tick.
    pub attack: bool,
    /// Further full clicks inside the same tick, after the one above. A
    /// mouse clicked more than twenty times a second sends two in some
    /// ticks.
    pub extra_clicks: u8,
    /// Mouse counts since the previous tick. Positive x turns right,
    /// positive y looks down.
    pub dx: i32,
    pub dy: i32,
    /// The right mouse button is down on this tick. Going from up to down
    /// is a press; with a sword in hand that starts a block, with pearls in
    /// hand it throws one.
    pub use_item: bool,
    /// The right button was let go and pressed again since the previous
    /// tick: a press on this tick although it was down on the last one.
    pub use_again: bool,
    /// A number key pressed on this tick, 1 to 9 (0: none): the hotbar slot
    /// to take in hand. The sword is on 1 and the pearls on 2.
    pub slot_key: u8,
    /// The inventory screen. With the screen shut, anything but 0 opens
    /// it (the inventory key). With it open: 0 shuts it, 1 leaves it open,
    /// 9 to 44 shift-clicks that slot of the window, and 64 more than a
    /// slot shift-clicks it and shuts the screen in the same tick. On every
    /// tick the screen is open at some point, the tick it opens and the tick
    /// it shuts too, the game takes no other key, no button and no mouse
    /// movement, so everything else here is left out.
    pub inv: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct ClientCfg {
    /// `f * f * f * 8` from the sensitivity slider; degrees per count is
    /// this times 0.15.
    pub mouse_f1: f32,
    /// Vanilla 1.8.9 aims the tick-start pick with the head yaw of the
    /// previous tick. `true` uses the current yaw instead, which is what the
    /// common mouse-delay fix does.
    pub mouse_delay_fix: bool,
    /// Knockback enchantment level on the held sword.
    pub kb_ench: i32,
}

impl ClientCfg {
    pub fn with_sensitivity(sens: f32) -> ClientCfg {
        let f = sens * 0.6f32 + 0.2f32;
        ClientCfg { mouse_f1: f * f * f * 8.0f32, mouse_delay_fix: false, kb_ench: 0 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pick {
    /// Crosshair is on the other player within attack reach.
    pub entity: bool,
    /// Crosshair is on a block within block reach (and not on the player).
    pub block: bool,
    /// Distance from the eye to the point where the ray enters the box.
    pub dist: f64,
}

#[derive(Clone, Debug)]
pub struct RemotePlayer {
    pub pos: V3,
    pub prev_pos: V3,
    pub bb: Aabb,
    pub yaw: f32,
    pub pitch: f32,
    pub server_pos: [i32; 3],
    pub target: V3,
    pub target_yaw: f64,
    pub target_pitch: f64,
    pub increments: i32,
    pub on_ground: bool,
    pub sprinting: bool,
    pub hurt_time: i32,
    pub swing_in_progress: bool,
    pub swing_progress_int: i32,
    /// The flag the server sets while this player uses an item: with a
    /// sword in hand, it is shown blocking; with a potion or food, drinking
    /// or eating.
    pub using: bool,
    /// Particles of an effect show around it.
    pub fx: bool,
    /// Its health, which the server tells everybody.
    pub health: f32,
    /// What this player holds, as the server last said: the item, and the
    /// wear of a sword or the size of a stack of pearls.
    pub item: Item,
    pub item_n: u32,
    /// Bookkeeping for observations, not game state.
    pub ticks_since_hurt: i32,
    /// Ticks since this player was last moved by more than three blocks at
    /// once, which is what a pearl landing looks like from outside.
    pub ticks_since_jump: i32,
}

/// An ender pearl or a potion as a client has it: created by the server's
/// spawn packet, flying on its own between the server's corrections, and
/// gone only when the server says so. It hits nothing on the client.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClientThrown {
    pub id: u32,
    pub kind: Kind,
    pub pos: V3,
    pub motion: V3,
    pub server_pos: [i32; 3],
    /// Client ticks since it appeared.
    pub age: i32,
    /// Bookkeeping for observations: it appeared where this client had
    /// just thrown one from, so it is taken to be its own.
    pub mine: bool,
    /// Bookkeeping for observations: the ticks the server's had flown when
    /// it moved as this copy does now, and how far behind it this copy is.
    /// The server says where it was thrown from and how fast, and at the
    /// end of that tick how it moves by then, but not where it is by then:
    /// the copy has the motion of a tick later and stays the first step
    /// of the flight behind. Every ten ticks the server sends how far it
    /// has moved, counted from where it was after that first step, which
    /// the copy adds to where it began: still a step behind (`first`, in
    /// the 32nds of a block positions come in). Those are rounded down:
    /// what that cuts off is in `behind` too, exactly for a throw of this
    /// client's own, whose place it knows, and as half a 32nd for the
    /// rest.
    pub air: i32,
    pub behind: V3,
    pub first: V3,
}

/// Half of the step positions are sent in.
const HALF_STEP: f64 = 1.0 / 64.0;

impl ClientThrown {
    /// `Entity.setPositionAndRotation2`: put it there, and on top of any
    /// block it would be inside of.
    fn set_position2(&mut self, world: &World, p: V3) {
        self.pos = p;
        let w = (0.25f32 / 2.0f32) as f64;
        let bb = Aabb::new([p[0] - w, p[1], p[2] - w], [p[0] + w, p[1] + 0.25f32 as f64, p[2] + w]);
        let mut boxes = Vec::new();
        world.colliding_boxes(&bb.contract(0.03125, 0.0, 0.03125), &mut boxes);
        if !boxes.is_empty() {
            let mut top = 0.0f64;
            for b in &boxes {
                if b.max[1] > top {
                    top = b.max[1];
                }
            }
            self.pos[1] = p[1] + (top - bb.min[1]);
        }
    }

    /// `NetHandlerPlayClient.handleEntityMovement`.
    fn rel_move(&mut self, world: &World, d: [i8; 3]) {
        for a in 0..3 {
            self.server_pos[a] += d[a] as i32;
        }
        let sp = self.server_pos;
        self.set_position2(world, [sp[0] as f64 / 32.0, sp[1] as f64 / 32.0, sp[2] as f64 / 32.0]);
        for a in 0..3 {
            self.behind[a] = sp[a] as f64 / 32.0 + self.first[a] + HALF_STEP - self.pos[a];
        }
    }

    /// `NetHandlerPlayClient.handleEntityTeleport`.
    fn teleport(&mut self, world: &World, sp: [i32; 3]) {
        self.server_pos = sp;
        let d = [sp[0] as f64 / 32.0, sp[1] as f64 / 32.0, sp[2] as f64 / 32.0];
        self.first = [0.0; 3];
        self.behind = [HALF_STEP; 3];
        if (self.pos[0] - d[0]).abs() < 0.03125 && (self.pos[1] - d[1]).abs() < 0.015625 && (self.pos[2] - d[2]).abs() < 0.03125 {
            let p = self.pos;
            self.set_position2(world, p);
        } else {
            self.set_position2(world, d);
        }
    }

    /// Where the server had it when it moved as this copy does now.
    #[inline]
    pub fn server_side(&self) -> V3 {
        [self.pos[0] + self.behind[0], self.pos[1] + self.behind[1], self.pos[2] + self.behind[2]]
    }

    /// `EntityThrowable.onUpdate` as far as it matters on a client. Returns
    /// false for one that has fallen out of the world: the client then
    /// drops it on its own.
    fn tick(&mut self) -> bool {
        let alive = !(self.pos[1] < -64.0);
        for a in 0..3 {
            self.pos[a] += self.motion[a];
        }
        let (f2, f3) = (0.99f32 as f64, self.kind.gravity() as f64);
        for a in 0..3 {
            self.motion[a] *= f2;
        }
        self.motion[1] -= f3;
        self.age += 1;
        self.air += 1;
        alive
    }
}

impl RemotePlayer {
    /// State right after `handleSpawnPlayer` for a player whose true
    /// position is `pos`.
    pub fn spawn(pos: V3, yaw: f32, pitch: f32, on_ground: bool) -> Self {
        let sp = [mth::floor_d(pos[0] * 32.0), mth::floor_d(pos[1] * 32.0), mth::floor_d(pos[2] * 32.0)];
        let p = [sp[0] as f64 / 32.0, sp[1] as f64 / 32.0, sp[2] as f64 / 32.0];
        let by = mth::floor_f(yaw * 256.0f32 / 360.0f32) as i8;
        let bp = mth::floor_f(pitch * 256.0f32 / 360.0f32) as i8;
        let y = (by as i32 * 360) as f32 / 256.0f32;
        let pt = (bp as i32 * 360) as f32 / 256.0f32;
        RemotePlayer {
            pos: p,
            prev_pos: p,
            bb: Aabb::player(p),
            yaw: y,
            pitch: pt,
            server_pos: sp,
            target: p,
            target_yaw: y as f64,
            target_pitch: pt as f64,
            increments: 0,
            on_ground,
            sprinting: false,
            hurt_time: 0,
            swing_in_progress: false,
            swing_progress_int: 0,
            using: false,
            fx: false,
            health: 20.0,
            item: Item::Sword,
            item_n: 0,
            ticks_since_hurt: 1000,
            ticks_since_jump: 1000,
        }
    }

    #[inline]
    fn set_position(&mut self, p: V3) {
        self.pos = p;
        self.bb = Aabb::player(p);
    }

    /// `EntityOtherPlayerMP.setPositionAndRotation2`.
    #[inline]
    fn set_target(&mut self, p: V3, yaw: f32, pitch: f32) {
        self.target = p;
        self.target_yaw = yaw as f64;
        self.target_pitch = pitch as f64;
        self.increments = 3;
    }

    #[inline]
    fn byte_angle(b: i8) -> f32 {
        (b as i32 * 360) as f32 / 256.0f32
    }

    /// `NetHandlerPlayClient.handleEntityMovement`.
    fn rel_move(&mut self, d: [i8; 3], rot: Option<(i8, i8)>, on_ground: bool) {
        for a in 0..3 {
            self.server_pos[a] += d[a] as i32;
        }
        let p = [
            self.server_pos[0] as f64 / 32.0,
            self.server_pos[1] as f64 / 32.0,
            self.server_pos[2] as f64 / 32.0,
        ];
        let (yaw, pitch) = match rot {
            Some((y, p)) => (Self::byte_angle(y), Self::byte_angle(p)),
            None => (self.yaw, self.pitch),
        };
        self.set_target(p, yaw, pitch);
        self.on_ground = on_ground;
    }

    /// `NetHandlerPlayClient.handleEntityTeleport`.
    fn teleport(&mut self, sp: [i32; 3], yaw: i8, pitch: i8, on_ground: bool) {
        self.server_pos = sp;
        let d = [sp[0] as f64 / 32.0, sp[1] as f64 / 32.0, sp[2] as f64 / 32.0];
        let (f, f1) = (Self::byte_angle(yaw), Self::byte_angle(pitch));
        if crate::aabb::dist_sq(self.target, d) > 9.0 {
            self.ticks_since_jump = 0;
        }
        if (self.pos[0] - d[0]).abs() < 0.03125
            && (self.pos[1] - d[1]).abs() < 0.015625
            && (self.pos[2] - d[2]).abs() < 0.03125
        {
            let p = self.pos;
            self.set_target(p, f, f1);
        } else {
            self.set_target(d, f, f1);
        }
        self.on_ground = on_ground;
    }

    /// `EntityLivingBase.swingItem` on the client copy.
    fn swing_item(&mut self) {
        if !self.swing_in_progress || self.swing_progress_int >= 3 || self.swing_progress_int < 0 {
            self.swing_progress_int = -1;
            self.swing_in_progress = true;
        }
    }

    /// One `onUpdate` of `EntityOtherPlayerMP`.
    pub fn tick(&mut self) {
        self.prev_pos = self.pos;
        if self.hurt_time > 0 {
            self.hurt_time -= 1;
        }
        if self.ticks_since_hurt < 1000 {
            self.ticks_since_hurt += 1;
        }
        if self.ticks_since_jump < 1000 {
            self.ticks_since_jump += 1;
        }
        if self.increments > 0 {
            let n = self.increments as f64;
            let d0 = self.pos[0] + (self.target[0] - self.pos[0]) / n;
            let d1 = self.pos[1] + (self.target[1] - self.pos[1]) / n;
            let d2 = self.pos[2] + (self.target[2] - self.pos[2]) / n;
            let mut d3 = self.target_yaw - self.yaw as f64;
            while d3 < -180.0 {
                d3 += 360.0;
            }
            while d3 >= 180.0 {
                d3 -= 360.0;
            }
            self.yaw = (self.yaw as f64 + d3 / n) as f32;
            self.pitch = (self.pitch as f64 + (self.target_pitch - self.pitch as f64) / n) as f32;
            self.increments -= 1;
            self.set_position([d0, d1, d2]);
            self.yaw %= 360.0f32;
            self.pitch %= 360.0f32;
        }
        // updateArmSwingProgress
        if self.swing_in_progress {
            self.swing_progress_int += 1;
            if self.swing_progress_int >= 6 {
                self.swing_progress_int = 0;
                self.swing_in_progress = false;
            }
        } else {
            self.swing_progress_int = 0;
        }
    }
}

#[derive(Clone, Debug)]
pub struct LocalPlayer {
    pub l: Living,
    /// `rotationYawHead`, copied from the yaw once per tick.
    pub yaw_head: f32,
    /// `MovementInput` as left by the previous tick.
    pub mi_forward: f32,
    pub mi_strafe: f32,
    pub mi_jump: bool,
    pub sprinting_ticks_left: i32,
    pub sprint_toggle_timer: i32,
    pub server_sprint_state: bool,
    pub last_reported: V3,
    pub last_yaw: f32,
    pub last_pitch: f32,
    pub position_update_ticks: i32,
    pub hurt_time: i32,
    /// `isUsingItem()`: this client is blocking with the sword, drinking or
    /// eating.
    pub using_item: bool,
    /// The hotbar slot of the stack in use and `itemInUseCount`, which a
    /// client only counts down: the server says when a drink or a meal is
    /// finished.
    pub use_slot: u8,
    pub use_count: i32,
    /// `Minecraft.rightClickDelayTimer`.
    pub right_click_delay: i32,
    /// The right button as it was on the previous tick.
    pub use_key_down: bool,
    /// Counts, for every hotbar slot, the times its stack was replaced by
    /// a new one. A block, a drink or a meal belongs to the stack it was
    /// started with: once that stack has been replaced, the client drops
    /// the use on its own, without telling the server.
    pub gens: [u32; 9],
    pub use_gen: u32,
    /// The using-item flag of this player as the server last sent it.
    pub flag_using: bool,
    /// `inventory.currentItem`: the hotbar slot in hand.
    pub held: u8,
    /// `PlayerControllerMP.currentPlayerItem`: the slot the server was last
    /// told about.
    pub synced: u8,
    /// Ender pearls in the second hotbar slot, as this client counts them.
    pub pearls: u8,
    /// The hotbar slots that hold a splash potion, one bit each.
    pub potions: u16,
    /// Those that hold a potion to drink and those that hold an empty
    /// bottle, and the steaks in the last slot.
    pub drinks: u16,
    pub bottles: u16,
    pub steak: u8,
    /// The three rows of the inventory, whether its screen is open, and the
    /// number of the last click in it (`Container.transactionID`).
    pub rows: Rows,
    pub inv_open: bool,
    pub action: u16,
    /// The Speed effect as this client has it: its amplifier and the ticks
    /// left, counted down here and set again whenever the server says
    /// (-1: no effect), and whether particles show around this player.
    pub speed_amp: i8,
    pub speed_ticks: i32,
    pub fx: bool,
    /// Health, food and saturation as the server last said.
    pub health: f32,
    pub food: i32,
    pub saturation: f32,
    /// Bookkeeping for observations, not game state.
    pub last_kb: V3,
    pub ticks_since_kb: i32,
    pub ticks_since_hurt: i32,
    pub ticks_since_swing: i32,
    pub ticks_since_attack: i32,
    /// Ticks since the server last put this player somewhere.
    pub ticks_since_moved: i32,
    /// Ticks since this client last threw a pearl or a potion, where its
    /// eyes were then, and how many of its throws of each kind have not
    /// shown up yet.
    pub ticks_since_throw: i32,
    pub throw_eye: V3,
    /// Bookkeeping for observations: where the server has what this
    /// client threw last come from, which it sends back less exactly.
    pub throw_from: V3,
    pub throws_pending: u8,
    pub potions_pending: u8,
    /// Potions of the other player this client has seen appear.
    pub potions_seen: u8,
}

impl LocalPlayer {
    pub fn new(pos: V3, yaw: f32, pitch: f32, speed_amp: i8) -> Self {
        let l = Living::new(pos, yaw, pitch, speed_amp);
        LocalPlayer {
            l,
            yaw_head: yaw,
            mi_forward: 0.0,
            mi_strafe: 0.0,
            mi_jump: false,
            sprinting_ticks_left: 0,
            sprint_toggle_timer: 0,
            server_sprint_state: false,
            last_reported: pos,
            last_yaw: yaw,
            last_pitch: pitch,
            position_update_ticks: 0,
            hurt_time: 0,
            using_item: false,
            use_slot: SLOT_SWORD,
            use_count: 0,
            right_click_delay: 0,
            use_key_down: false,
            gens: [0; 9],
            use_gen: 0,
            flag_using: false,
            held: SLOT_SWORD,
            synced: SLOT_SWORD,
            pearls: 0,
            potions: 0,
            drinks: 0,
            bottles: 0,
            steak: 0,
            rows: Rows::default(),
            inv_open: false,
            action: 0,
            speed_amp: speed_amp,
            speed_ticks: if speed_amp >= 0 { crate::server::FOR_GOOD } else { -1 },
            fx: false,
            health: 20.0,
            food: 20,
            saturation: 5.0,
            last_kb: [0.0; 3],
            ticks_since_kb: 1000,
            ticks_since_hurt: 1000,
            ticks_since_swing: 1000,
            ticks_since_attack: 1000,
            ticks_since_moved: 1000,
            ticks_since_throw: 1000,
            throw_eye: pos,
            throw_from: pos,
            throws_pending: 0,
            potions_pending: 0,
            potions_seen: 0,
        }
    }

    /// `EntityPlayerSP.setSprinting`.
    #[inline]
    pub fn set_sprinting(&mut self, s: bool) {
        self.l.sprinting = s;
        self.l.attr.sprint = s;
        self.sprinting_ticks_left = if s { 600 } else { 0 };
    }

    /// `Entity.setAngles(yaw, pitch)`.
    #[inline]
    pub fn set_angles(&mut self, yaw: f32, pitch: f32) {
        self.l.yaw = (self.l.yaw as f64 + yaw as f64 * 0.15) as f32;
        self.l.pitch = (self.l.pitch as f64 - pitch as f64 * 0.15) as f32;
        if self.l.pitch < -90.0 {
            self.l.pitch = -90.0;
        } else if self.l.pitch > 90.0 {
            self.l.pitch = 90.0;
        }
    }

    #[inline]
    pub fn eye(&self) -> V3 {
        let p = self.l.body.pos;
        [p[0], p[1] + EYE_HEIGHT, p[2]]
    }

    /// The hotbar but for the sword.
    #[inline]
    pub fn bar(&self) -> Bar {
        Bar { pearls: self.pearls, potions: self.potions, drinks: self.drinks, bottles: self.bottles, steak: self.steak }
    }

    #[inline]
    pub fn set_bar(&mut self, b: Bar) {
        self.pearls = b.pearls;
        self.potions = b.potions;
        self.drinks = b.drinks;
        self.bottles = b.bottles;
        self.steak = b.steak;
    }

    /// What is in hand.
    #[inline]
    pub fn in_hand(&self) -> Item {
        if self.held == SLOT_SWORD {
            Item::Sword
        } else {
            self.bar().stack(self.held).0
        }
    }

    /// The stack in hand as a packet names it: the item and how many.
    #[inline]
    fn said(&self) -> (Item, u8) {
        if self.held == SLOT_SWORD {
            (Item::Sword, 1)
        } else {
            let (item, n) = self.bar().stack(self.held);
            (item, n as u8)
        }
    }

    /// `setItemInUse(stack, duration)` for the stack in hand.
    #[inline]
    fn start_use(&mut self, ticks: i32) {
        if !(self.using_item && self.use_slot == self.held && self.use_gen == self.gens[self.held as usize]) {
            self.using_item = true;
            self.use_slot = self.held;
            self.use_gen = self.gens[self.held as usize];
            self.use_count = ticks;
        }
    }

    /// Whether the client is drinking or eating: using an item that is not
    /// the sword.
    #[inline]
    pub fn consuming(&self) -> bool {
        self.using_item && self.use_slot != SLOT_SWORD
    }

    /// `PlayerControllerMP.syncCurrentPlayItem`: tell the server which slot
    /// is in hand, if it does not know yet.
    #[inline]
    fn sync_slot(&mut self, out: &mut Vec<C2S>) {
        if self.held != self.synced {
            self.synced = self.held;
            out.push(C2S::HeldItem(self.held));
        }
    }
}

/// What one client tick did, for rewards and statistics.
#[derive(Clone, Copy, Debug, Default)]
pub struct TickOut {
    pub pick: Pick,
    pub swung: bool,
    pub attacked: bool,
    /// The attack was made with the client sprint flag set, so the client
    /// slowed itself down and dropped its own sprint.
    pub sprint_attack: bool,
    pub jumped: bool,
    /// A right click was made (the block started or was asked for again).
    pub used: bool,
    /// An ender pearl was thrown.
    pub threw: bool,
    /// A potion was thrown.
    pub threw_potion: bool,
    /// Left clicks that were swallowed because the client was blocking.
    pub swallowed: u8,
    /// A shift click in the inventory moved a stack: what it was.
    pub moved: Option<Item>,
}

/// Ways a client can depart from the plain game. They stand for what some
/// opponents on real servers have, so that the agent meets them in
/// training. The default is the plain game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    /// Attack reach from the eye, in blocks (the game: 3.0).
    pub reach: f64,
    /// Share of the horizontal knockback the client takes (the game: 1.0).
    pub kb_taken: f64,
    /// The client may still hit the other player where it showed them up
    /// to this many ticks ago (the game: 0). This is what holding back the
    /// other player's movement packets gives.
    pub backtrack: u8,
}

impl Default for Edge {
    fn default() -> Self {
        Edge { reach: 3.0, kb_taken: 1.0, backtrack: 0 }
    }
}

/// Longest backtrack an `Edge` can ask for, in ticks.
pub const MAX_BACKTRACK: usize = 8;

#[derive(Clone, Debug)]
pub struct Client {
    pub lp: LocalPlayer,
    pub rp: RemotePlayer,
    pub edge: Edge,
    /// Where the other player was shown at the picks of the last ticks,
    /// newest first.
    shown: [Aabb; MAX_BACKTRACK],
    /// The pearls and potions in flight this client knows of, oldest first.
    pub thrown: Vec<ClientThrown>,
}

/// `EntityRenderer.getMouseOver(1.0F)` for a world that holds one other
/// player. `yaw` is whichever yaw the caller wants the ray to use.
pub fn pick_with(world: &World, eye: V3, own_bb: &Aabb, pitch: f32, yaw: f32, remote_bb: &Aabb, reach_limit: f64) -> Pick {
    let d0 = 4.5f32 as f64;
    let look = mth::look_vec(pitch, yaw);
    let end = [eye[0] + look[0] * d0, eye[1] + look[1] * d0, eye[2] + look[2] * d0];
    // objectMouseOver from the block ray. In open air this is usually a
    // MISS placed where the ray entered its last cell, and the distance to
    // that point is the limit an entity has to beat.
    let trace = world.ray_trace_blocks(eye, end, true);
    let d1 = match trace {
        RayHit::Block(h) | RayHit::Miss(h) => dist(h, eye),
        RayHit::None => d0,
    };
    let query = own_bb.add_coord(look[0] * d0, look[1] * d0, look[2] * d0).expand(1.0, 1.0, 1.0);
    let mut pointed = false;
    let mut vec33 = eye;
    let mut d2 = d1;
    if remote_bb.intersects(&query) {
        let f1 = 0.1f32 as f64;
        let ebb = remote_bb.expand(f1, f1, f1);
        let mop = ebb.calculate_intercept(eye, end);
        if ebb.is_vec_inside(eye) {
            if d2 >= 0.0 {
                pointed = true;
                vec33 = mop.unwrap_or(eye);
                d2 = 0.0;
            }
        } else if let Some(h) = mop {
            let d3 = dist(eye, h);
            if d3 < d2 || d2 == 0.0 {
                pointed = true;
                vec33 = h;
                d2 = d3;
            }
        }
    }
    let reach = dist(eye, vec33);
    // A player under the crosshair but out of reach turns the pick into a
    // miss, also when there is a block behind.
    let mut too_far = false;
    if pointed && reach > reach_limit {
        pointed = false;
        too_far = true;
    }
    if pointed && (d2 < d1 || trace == RayHit::None) {
        Pick { entity: true, block: false, dist: reach }
    } else {
        Pick { entity: false, block: !too_far && matches!(trace, RayHit::Block(_)), dist: reach }
    }
}

/// Where a pearl or potion at `pos` moving by `motion` per tick stops if it
/// flies on undisturbed, and after how many ticks: its position at the
/// start of the tick in which it reaches a block. That is where a pearl
/// puts its thrower, and where the reach of a potion is measured from.
pub fn thrown_flight(world: &World, kind: Kind, mut pos: V3, mut m: V3) -> (V3, i32) {
    let (f2, f3) = (0.99f32 as f64, kind.gravity() as f64);
    for t in 0..100 {
        let next = [pos[0] + m[0], pos[1] + m[1], pos[2] + m[2]];
        if !world.clear_path(pos, next) {
            if let RayHit::Block(_) = world.ray_trace_blocks(pos, next, false) {
                return (pos, t);
            }
        }
        pos = next;
        for a in 0..3 {
            m[a] *= f2;
        }
        m[1] -= f3;
    }
    (pos, 100)
}

/// `thrown_flight` for a pearl or potion this client sees, from where the
/// server has it.
pub fn pearl_landing(world: &World, e: &ClientThrown) -> (V3, i32) {
    thrown_flight(world, e.kind, e.server_side(), e.motion)
}

/// `thrown_flight` for what a player standing at `feet` with this view
/// would throw, leaving out the small random part of a throw.
pub fn throw_preview(world: &World, kind: Kind, feet: V3, yaw: f32, pitch: f32) -> (V3, i32) {
    let (pos, m) = crate::server::throw_heading(kind, feet, yaw, pitch);
    let v = kind.speed() as f64;
    thrown_flight(world, kind, pos, [m[0] * v, m[1] * v, m[2] * v])
}

/// `throw_preview` for a pearl.
pub fn pearl_preview(world: &World, feet: V3, yaw: f32, pitch: f32) -> (V3, i32) {
    throw_preview(world, Kind::Pearl, feet, yaw, pitch)
}

/// `Minecraft.rightClickMouse`. Returns what was thrown, if anything.
#[inline]
fn right_click(lp: &mut LocalPlayer, pick: &Pick, out: &mut Vec<C2S>) -> Option<Kind> {
    lp.right_click_delay = 4;
    let item = lp.in_hand();
    if pick.entity {
        // Neither interaction does anything, so the item is used as well.
        lp.sync_slot(out);
        out.push(C2S::InteractAt);
        out.push(C2S::Interact);
    } else if pick.block {
        lp.sync_slot(out);
        out.push(C2S::UseBlock(lp.said()));
        if item == Item::Empty {
            return None;
        }
    }
    if item == Item::Empty {
        return None;
    }
    // sendUseItem: the server is told, with the stack as it is before the
    // use, and the item is used here as well.
    lp.sync_slot(out);
    out.push(C2S::UseItem(lp.said()));
    match item {
        Item::Empty | Item::Bottle => None,
        // ItemSword.onItemRightClick -> setItemInUse
        Item::Sword => {
            lp.start_use(BLOCK_TICKS);
            None
        }
        // ItemPotion.onItemRightClick for a potion to drink.
        Item::Drink => {
            lp.start_use(USE_TICKS);
            None
        }
        // ItemFood.onItemRightClick: only with room for food, as far as
        // this client knows.
        Item::Food => {
            if lp.food < 20 {
                lp.start_use(USE_TICKS);
            }
            None
        }
        Item::Pearl => {
            // ItemEnderPearl.onItemRightClick: the client only takes one
            // off its stack. The pearl itself comes from the server.
            lp.pearls -= 1;
            Some(Kind::Pearl)
        }
        Item::Potion => {
            // ItemPotion.onItemRightClick for a splash potion: the stack of
            // one is used up.
            lp.potions &= !(1 << lp.held);
            Some(Kind::Potion)
        }
    }
}

/// `EntityPlayerSP.pushOutOfBlocks`.
#[inline]
fn push_out_of_blocks(world: &World, motion: &mut V3, x: f64, y: f64, z: f64) {
    let (bx, by, bz) = (mth::floor_d(x), mth::floor_d(y), mth::floor_d(z));
    // isOpenBlockSpace: neither this block nor the one above is a full cube.
    let open = |x: i32, z: i32| !world.solid(x, by, z) && !world.solid(x, by + 1, z);
    if open(bx, bz) {
        return;
    }
    let d0 = x - bx as f64;
    let d1 = z - bz as f64;
    let mut i = -1;
    let mut d2 = 9999.0f64;
    if open(bx - 1, bz) && d0 < d2 {
        d2 = d0;
        i = 0;
    }
    if open(bx + 1, bz) && 1.0 - d0 < d2 {
        d2 = 1.0 - d0;
        i = 1;
    }
    if open(bx, bz - 1) && d1 < d2 {
        d2 = d1;
        i = 4;
    }
    if open(bx, bz + 1) && 1.0 - d1 < d2 {
        i = 5;
    }
    let f = 0.1f32;
    match i {
        0 => motion[0] = -f as f64,
        1 => motion[0] = f as f64,
        4 => motion[2] = -f as f64,
        5 => motion[2] = f as f64,
        _ => {}
    }
}

impl Client {
    pub fn new(pos: V3, yaw: f32, pitch: f32, speed_amp: i8, other_pos: V3, other_yaw: f32, other_pitch: f32) -> Self {
        let rp = RemotePlayer::spawn(other_pos, other_yaw, other_pitch, true);
        let shown = [rp.bb; MAX_BACKTRACK];
        Client { lp: LocalPlayer::new(pos, yaw, pitch, speed_amp), rp, edge: Edge::default(), shown, thrown: Vec::new() }
    }

    /// The pick the next tick would make if the mouse did not move.
    pub fn pick(&self, world: &World, cfg: &ClientCfg) -> Pick {
        let yaw = if cfg.mouse_delay_fix { self.lp.l.yaw } else { self.lp.yaw_head };
        self.pick_ray(world, self.lp.l.pitch, yaw)
    }

    /// The pick for a ray with the given angles: against the other player
    /// where this client shows them now and, for a client with a backtrack
    /// edge, where it showed them on its last ticks.
    pub fn pick_ray(&self, world: &World, pitch: f32, yaw: f32) -> Pick {
        let eye = self.lp.eye();
        let own = &self.lp.l.body.bb;
        let now = pick_with(world, eye, own, pitch, yaw, &self.rp.bb, self.edge.reach);
        if now.entity {
            return now;
        }
        for old in self.shown.iter().take(self.edge.backtrack as usize) {
            let p = pick_with(world, eye, own, pitch, yaw, old, self.edge.reach);
            if p.entity {
                return p;
            }
        }
        now
    }

    /// A packet handler of `NetHandlerPlayClient`, run on the main thread
    /// before the next tick.
    ///
    /// The answer to a server teleport is sent from inside the handler, so
    /// it leaves before the packets of the next tick; it goes to `out`.
    pub fn handle(&mut self, world: &World, p: S2C, out: &mut Vec<C2S>) {
        match p {
            S2C::PosLook { pos, yaw, pitch } => {
                // handlePlayerPosLook, then Entity.setPositionAndRotation.
                let lp = &mut self.lp;
                lp.l.body.motion = [0.0; 3];
                lp.l.body.set_position(pos);
                lp.l.yaw = yaw % 360.0f32;
                lp.l.pitch = pitch % 360.0f32;
                lp.ticks_since_moved = 0;
                out.push(C2S::Player {
                    pos: Some([lp.l.body.pos[0], lp.l.body.bb.min[1], lp.l.body.pos[2]]),
                    rot: Some((lp.l.yaw, lp.l.pitch)),
                    on_ground: false,
                });
            }
            S2C::Velocity { own, v } => {
                if own {
                    let k = self.edge.kb_taken;
                    let m = [v[0] as f64 / 8000.0 * k, v[1] as f64 / 8000.0, v[2] as f64 / 8000.0 * k];
                    self.lp.l.body.motion = m;
                    self.lp.last_kb = m;
                    self.lp.ticks_since_kb = 0;
                }
            }
            S2C::Hurt { own } => {
                if own {
                    self.lp.hurt_time = 10;
                    self.lp.ticks_since_hurt = 0;
                } else {
                    self.rp.hurt_time = 10;
                    self.rp.ticks_since_hurt = 0;
                }
            }
            S2C::RelMove { d, on_ground } => self.rp.rel_move(d, None, on_ground),
            S2C::Look { yaw, pitch, on_ground } => self.rp.rel_move([0, 0, 0], Some((yaw, pitch)), on_ground),
            S2C::LookMove { d, yaw, pitch, on_ground } => self.rp.rel_move(d, Some((yaw, pitch)), on_ground),
            S2C::Teleport { p, yaw, pitch, on_ground } => self.rp.teleport(p, yaw, pitch, on_ground),
            S2C::Meta { own, flags, health, fx } => {
                if let Some(f) = fx {
                    if own {
                        self.lp.fx = f;
                    } else {
                        self.rp.fx = f;
                    }
                }
                if let Some((sprinting, using)) = flags {
                    if own {
                        // The flags byte is overwritten without going through
                        // setSprinting, so neither the modifier nor
                        // sprintingTicksLeft changes here.
                        self.lp.l.sprinting = sprinting;
                        self.lp.flag_using = using;
                    } else {
                        self.rp.sprinting = sprinting;
                        self.rp.using = using;
                    }
                }
                // The health is a watched value like the flags, and a
                // player is sent its own as well.
                if let Some(h) = health {
                    if own {
                        self.lp.health = h;
                    } else {
                        self.rp.health = h;
                    }
                }
            }
            // handleUpdateHealth -> EntityPlayerSP.setPlayerSPHealth
            S2C::Health { health, food, saturation } => {
                let lp = &mut self.lp;
                let f = lp.health - health;
                if f <= 0.0 {
                    lp.health = health.clamp(0.0f32, 20.0f32);
                } else {
                    // damageEntity takes the difference off again.
                    lp.health = (lp.health - f).clamp(0.0f32, 20.0f32);
                    lp.hurt_time = 10;
                }
                lp.food = food;
                lp.saturation = saturation;
            }
            // handleSetSlot puts a new stack object into the slot.
            S2C::SetSlot { slot, item, count } => {
                let lp = &mut self.lp;
                if slot < 9 {
                    lp.gens[slot as usize] = lp.gens[slot as usize].wrapping_add(1);
                    if slot != SLOT_SWORD {
                        let mut b = lp.bar();
                        b.put(slot, item, count);
                        lp.set_bar(b);
                    }
                } else if slot < 36 {
                    lp.rows.put(slot as usize - 9, item, count);
                }
            }
            // handleWindowItems puts new stack objects into every slot.
            S2C::WindowItems(b, rows) => {
                for g in self.lp.gens.iter_mut() {
                    *g = g.wrapping_add(1);
                }
                self.lp.set_bar(b);
                self.lp.rows = rows;
            }
            // handleConfirmTransaction: a click the server did not take as
            // the client said is answered, and the inventory it sends
            // along puts things right.
            S2C::Confirm { action, ok } => {
                if !ok {
                    out.push(C2S::Confirm { action });
                }
            }
            // handleEntityStatus 9 -> EntityPlayer.onItemUseFinish, for the
            // stack this client has in use: the drink leaves a bottle, a
            // new stack, in the slot in hand; the steak is one less, and
            // the food it gives is counted here before the server says.
            S2C::UseDone => {
                let lp = &mut self.lp;
                if lp.using_item {
                    let slot = lp.held;
                    let mut b = lp.bar();
                    match b.stack(lp.use_slot).0 {
                        Item::Drink if lp.use_slot != SLOT_SWORD => {
                            b.put(lp.use_slot, Item::Empty, 0);
                            b.put(slot, Item::Bottle, 1);
                            lp.gens[slot as usize] = lp.gens[slot as usize].wrapping_add(1);
                        }
                        Item::Food if lp.use_slot != SLOT_SWORD => {
                            b.steak -= 1;
                            lp.food = (STEAK_FOOD + lp.food).min(20);
                            lp.saturation = (lp.saturation + STEAK_FOOD as f32 * STEAK_SATURATION * 2.0f32).min(lp.food as f32);
                        }
                        _ => {}
                    }
                    lp.set_bar(b);
                    lp.using_item = false;
                }
            }
            // handleEntityEffect and handleRemoveEntityEffect: what the
            // client shows. Its speed comes with the attribute.
            S2C::Effect { amp, ticks, .. } => {
                let lp = &mut self.lp;
                if lp.speed_ticks < 0 || amp > lp.speed_amp {
                    lp.speed_amp = amp;
                    lp.speed_ticks = ticks;
                } else if amp == lp.speed_amp && lp.speed_ticks < ticks {
                    // PotionEffect.combine: the longer of the two. The
                    // client's own count, which runs once per tick, can be
                    // ahead of the server's, which runs once per movement
                    // packet.
                    lp.speed_ticks = ticks;
                }
            }
            S2C::EffectEnd => {
                self.lp.speed_amp = -1;
                self.lp.speed_ticks = -1;
            }
            S2C::Cursor => {}
            // handleEntityEquipment
            S2C::Equip { item, n } => {
                self.rp.item = item;
                self.rp.item_n = n;
            }
            S2C::Thrown { id, ev } => match ev {
                // handleSpawnObject: a pearl or a potion at rest; its speed
                // comes in a packet of its own.
                ThrownEv::Spawn { kind, p, .. } => {
                    let pos = [p[0] as f64 / 32.0, p[1] as f64 / 32.0, p[2] as f64 / 32.0];
                    let lp = &mut self.lp;
                    let pending = match kind {
                        Kind::Pearl => &mut lp.throws_pending,
                        Kind::Potion => &mut lp.potions_pending,
                    };
                    let mine = *pending > 0 && crate::aabb::dist_sq(pos, lp.throw_eye) < 1.0;
                    if mine {
                        *pending -= 1;
                    } else if kind == Kind::Potion {
                        lp.potions_seen = lp.potions_seen.saturating_add(1);
                    }
                    let mut behind = [HALF_STEP; 3];
                    if mine {
                        let cut = [lp.throw_from[0] - pos[0], lp.throw_from[1] - pos[1], lp.throw_from[2] - pos[2]];
                        if cut.iter().all(|c| (0.0..=2.0 * HALF_STEP).contains(c)) {
                            behind = cut;
                        }
                    }
                    self.thrown.push(ClientThrown { id, kind, pos, motion: [0.0; 3], server_pos: p, age: 0, mine, air: 0, behind, first: [0.0; 3] });
                }
                ThrownEv::Meta => {}
                ev => {
                    if let Some(e) = self.thrown.iter_mut().find(|e| e.id == id) {
                        match ev {
                            ThrownEv::Velocity(v) => {
                                if e.age == 0 && e.air == 0 && e.motion != [0.0; 3] {
                                    // Told twice before it moved here: the
                                    // server's has made the step in between.
                                    e.air = 1;
                                    for a in 0..3 {
                                        let (from, was) = (e.pos[a] + e.behind[a], e.server_pos[a] as f64 / 32.0);
                                        e.first[a] = ((from + e.motion[a]) * 32.0).floor() / 32.0 - was;
                                        e.behind[a] += e.motion[a];
                                    }
                                }
                                e.motion = [v[0] as f64 / 8000.0, v[1] as f64 / 8000.0, v[2] as f64 / 8000.0];
                            }
                            ThrownEv::RelMove { d, .. } | ThrownEv::LookMove { d, .. } => e.rel_move(world, d),
                            ThrownEv::Look { .. } => e.rel_move(world, [0, 0, 0]),
                            ThrownEv::Teleport { p, .. } => e.teleport(world, p),
                            ThrownEv::Spawn { .. } | ThrownEv::Meta => {}
                        }
                    }
                }
            },
            // handleDestroyEntities
            S2C::Destroy { ids, n } => {
                let gone = &ids[..n as usize];
                self.thrown.retain(|e| !gone.contains(&e.id));
            }
            S2C::Props { own, base, speed_amp, sprint } => {
                if own {
                    self.lp.l.attr.base = base;
                    self.lp.l.attr.speed_amp = speed_amp;
                    self.lp.l.attr.sprint = sprint;
                }
            }
            S2C::Swing => self.rp.swing_item(),
        }
    }

    /// One `Minecraft.runTick`: mouse, pick, click, then the entity updates.
    pub fn tick(&mut self, world: &World, inp: &Input, cfg: &ClientCfg, out: &mut Vec<C2S>) -> TickOut {
        let mut t = TickOut::default();

        // The inventory screen. `GuiInventory` lets the part of `runTick`
        // that reads keys and buttons run, but the screen has taken every
        // event before it, and opening the screen let every key go
        // (`setIngameNotInFocus`), so a tick with the screen open is a tick
        // with no key, no button and no mouse movement. The tick on which
        // it shuts is one too: the key that shuts it is read by the screen.
        let was_open = self.lp.inv_open;
        let (click, shut) = match inp.inv {
            _ if !was_open => (0, false),
            0 => (0, true),
            s @ 9..=44 => (s, false),
            s @ 73..=108 => (s - INV_SHUT, true),
            _ => (0, false),
        };
        let opens = !was_open && inp.inv != 0;
        let idle = Input::default();
        let inp = if was_open || opens { &idle } else { inp };

        if self.lp.right_click_delay > 0 {
            self.lp.right_click_delay -= 1;
        }
        // playerController.updateController: a slot taken in hand on the
        // previous tick is reported now, unless a click already did.
        self.lp.sync_slot(out);

        // GuiScreen.handleInput, the mouse first and then the keys. A shift
        // click is `PlayerControllerMP.windowClick`: the next number, the
        // move made here at once, and both sent. The key that shuts the
        // screen is `EntityPlayerSP.closeScreen`.
        if click != 0 {
            let lp = &mut self.lp;
            lp.action = lp.action.wrapping_add(1);
            let (mut bar, before) = (lp.bar(), lp.bar());
            let said = shift_click(&mut bar, &mut lp.rows, click);
            // A stack that moved is a new stack where it lands.
            for k in 1..9u8 {
                if bar.stack(k) != before.stack(k) {
                    lp.gens[k as usize] = lp.gens[k as usize].wrapping_add(1);
                }
            }
            lp.set_bar(bar);
            out.push(C2S::Click { slot: click, said, action: lp.action });
            t.moved = said.map(|s| s.0);
        }
        if shut {
            out.push(C2S::CloseInv);
            self.lp.inv_open = false;
        }

        // Mouse movement accumulated since the previous tick.
        if inp.dx != 0 || inp.dy != 0 {
            self.lp.set_angles(inp.dx as f32 * cfg.mouse_f1, -(inp.dy as f32 * cfg.mouse_f1));
        }

        // getMouseOver(1.0F)
        let ray_yaw = if cfg.mouse_delay_fix { self.lp.l.yaw } else { self.lp.yaw_head };
        t.pick = self.pick_ray(world, self.lp.l.pitch, ray_yaw);
        self.shown.copy_within(0..MAX_BACKTRACK - 1, 1);
        self.shown[0] = self.rp.bb;
        let lp = &mut self.lp;

        // The number keys.
        if (1..=9).contains(&inp.slot_key) {
            lp.held = inp.slot_key - 1;
        }
        // The inventory key: C16, then the screen. What follows in this
        // tick finds every key let go, so an item in use is let go of.
        if opens {
            out.push(C2S::OpenInv);
            lp.inv_open = true;
        }

        // clickMouse. The button is released again inside the same tick, so
        // leftClickCounter is back at zero before the next one.
        // Every click of the tick runs clickMouse against the same pick. A
        // click into the air sets leftClickCounter, which swallows the
        // clicks that follow it in this tick.
        let clicks = if inp.attack { 1 + inp.extra_clicks } else { 0 };
        let press = inp.use_item && (!lp.use_key_down || inp.use_again);
        lp.use_key_down = inp.use_item;
        // A left click on a block starts breaking it, and until the end of
        // the tick a right click does nothing.
        let mut hitting_block = false;
        if lp.using_item {
            // Blocking: letting go of the button ends it, and every click
            // and press of this tick is thrown away.
            if !inp.use_item {
                lp.sync_slot(out);
                out.push(C2S::ReleaseUse);
                lp.using_item = false;
            }
            t.swallowed = clicks;
        } else {
            for _ in 0..clicks {
                out.push(C2S::Swing);
                t.swung = true;
                lp.ticks_since_swing = 0;
                if t.pick.block {
                    hitting_block = true;
                    continue;
                }
                if !t.pick.entity {
                    break;
                }
                lp.sync_slot(out);
                out.push(C2S::Attack);
                t.attacked = true;
                lp.ticks_since_attack = 0;
                // attackTargetEntityWithCurrentItem on the client: the copy
                // of the other player always reports a successful hit, with
                // whatever is in hand.
                let ench = if lp.held == SLOT_SWORD { cfg.kb_ench } else { 0 };
                let i = ench + if lp.l.sprinting { 1 } else { 0 };
                if i > 0 {
                    lp.l.body.motion[0] *= 0.6;
                    lp.l.body.motion[2] *= 0.6;
                    lp.set_sprinting(false);
                    t.sprint_attack = true;
                }
            }
            if press && !hitting_block {
                let k = right_click(lp, &t.pick, out);
                t.threw |= k == Some(Kind::Pearl);
                t.threw_potion |= k == Some(Kind::Potion);
                t.used = true;
            }
        }
        // The button held down clicks again once the delay has run out.
        if inp.use_item && lp.right_click_delay == 0 && !lp.using_item && !hitting_block {
            let k = right_click(lp, &t.pick, out);
            t.threw |= k == Some(Kind::Pearl);
            t.threw_potion |= k == Some(Kind::Potion);
            t.used = true;
        }
        if t.threw || t.threw_potion {
            lp.ticks_since_throw = 0;
            lp.throw_eye = lp.eye();
            lp.throw_from = crate::server::throw_heading(Kind::Potion, lp.last_reported, lp.last_yaw, lp.last_pitch).0;
            if t.threw {
                lp.throws_pending = lp.throws_pending.saturating_add(1);
            }
            if t.threw_potion {
                lp.potions_pending = lp.potions_pending.saturating_add(1);
            }
        }

        // --- EntityPlayerSP.onUpdate ---
        // EntityPlayer.onUpdate: the item in use has to be the very stack
        // that is in the hand.
        if lp.using_item {
            if lp.held == lp.use_slot && lp.use_gen == lp.gens[lp.held as usize] {
                lp.use_count -= 1;
            } else {
                lp.using_item = false;
            }
        }
        // updatePotionEffects: the client counts along.
        if lp.speed_ticks > 0 {
            lp.speed_ticks -= 1;
        }
        // onEntityUpdate
        if lp.hurt_time > 0 {
            lp.hurt_time -= 1;
        }
        // EntityPlayerSP.onLivingUpdate
        if lp.sprinting_ticks_left > 0 {
            lp.sprinting_ticks_left -= 1;
            if lp.sprinting_ticks_left == 0 {
                lp.set_sprinting(false);
            }
        }
        if lp.sprint_toggle_timer > 0 {
            lp.sprint_toggle_timer -= 1;
        }
        let f = 0.8f32;
        let flag2 = lp.mi_forward >= f;
        // MovementInputFromOptions.updatePlayerMoveState
        lp.mi_strafe = 0.0;
        lp.mi_forward = 0.0;
        if inp.forward > 0 {
            lp.mi_forward += 1.0;
        }
        if inp.forward < 0 {
            lp.mi_forward -= 1.0;
        }
        if inp.strafe > 0 {
            lp.mi_strafe += 1.0;
        }
        if inp.strafe < 0 {
            lp.mi_strafe -= 1.0;
        }
        lp.mi_jump = inp.jump;
        if lp.using_item {
            lp.mi_strafe *= 0.2f32;
            lp.mi_forward *= 0.2f32;
            lp.sprint_toggle_timer = 0;
        }
        // pushOutOfBlocks at four points around the feet. They are inside
        // the player's own box, so this only acts on a player that has got
        // into a wall (which a server teleport next to one can do).
        let w = 0.6f32 as f64 * 0.35;
        let (px, py, pz) = (lp.l.body.pos[0], lp.l.body.bb.min[1] + 0.5, lp.l.body.pos[2]);
        push_out_of_blocks(world, &mut lp.l.body.motion, px - w, py, pz + w);
        push_out_of_blocks(world, &mut lp.l.body.motion, px - w, py, pz - w);
        push_out_of_blocks(world, &mut lp.l.body.motion, px + w, py, pz - w);
        push_out_of_blocks(world, &mut lp.l.body.motion, px + w, py, pz + w);
        // Sprinting takes more than three drumsticks of food.
        let flag3 = lp.food as f32 > 6.0f32;
        if lp.l.body.on_ground && !flag2 && lp.mi_forward >= f && !lp.l.sprinting && flag3 && !lp.using_item {
            if lp.sprint_toggle_timer <= 0 && !inp.sprint_key {
                lp.sprint_toggle_timer = 7;
            } else {
                lp.set_sprinting(true);
            }
        }
        if !lp.l.sprinting && lp.mi_forward >= f && flag3 && !lp.using_item && inp.sprint_key {
            lp.set_sprinting(true);
        }
        if lp.l.sprinting && (lp.mi_forward < f || lp.l.body.collided_h || !flag3) {
            lp.set_sprinting(false);
        }
        // EntityPlayer.onLivingUpdate -> EntityLivingBase.onLivingUpdate.
        // updateEntityActionState copies the input and the head yaw.
        lp.yaw_head = lp.l.yaw;
        let (s, fw, j) = (lp.mi_strafe, lp.mi_forward, lp.mi_jump);
        t.jumped = lp.l.living_update(world, s, fw, j, true);
        lp.l.player_post();

        // onUpdateWalkingPlayer
        let flag = lp.l.sprinting;
        if flag != lp.server_sprint_state {
            out.push(C2S::Sprint(flag));
            lp.server_sprint_state = flag;
        }
        let pos = lp.l.body.pos;
        let min_y = lp.l.body.bb.min[1];
        let d0 = pos[0] - lp.last_reported[0];
        let d1 = min_y - lp.last_reported[1];
        let d2 = pos[2] - lp.last_reported[2];
        let d3 = (lp.l.yaw - lp.last_yaw) as f64;
        let d4 = (lp.l.pitch - lp.last_pitch) as f64;
        let moved = d0 * d0 + d1 * d1 + d2 * d2 > 9.0E-4 || lp.position_update_ticks >= 20;
        let turned = d3 != 0.0 || d4 != 0.0;
        let on_ground = lp.l.body.on_ground;
        out.push(C2S::Player {
            pos: if moved { Some([pos[0], min_y, pos[2]]) } else { None },
            rot: if turned { Some((lp.l.yaw, lp.l.pitch)) } else { None },
            on_ground,
        });
        lp.position_update_ticks += 1;
        if moved {
            lp.last_reported = [pos[0], min_y, pos[2]];
            lp.position_update_ticks = 0;
        }
        if turned {
            lp.last_yaw = lp.l.yaw;
            lp.last_pitch = lp.l.pitch;
        }

        if lp.ticks_since_kb < 1000 {
            lp.ticks_since_kb += 1;
        }
        if lp.ticks_since_hurt < 1000 {
            lp.ticks_since_hurt += 1;
        }
        if lp.ticks_since_swing < 1000 {
            lp.ticks_since_swing += 1;
        }
        if lp.ticks_since_attack < 1000 {
            lp.ticks_since_attack += 1;
        }
        if lp.ticks_since_moved < 1000 {
            lp.ticks_since_moved += 1;
        }
        if lp.ticks_since_throw < 1000 {
            lp.ticks_since_throw += 1;
            if lp.ticks_since_throw > 60 {
                // A throw the server refused never shows up.
                lp.throws_pending = 0;
                lp.potions_pending = 0;
            }
        }

        // The other player's entity updates after the local one, then the
        // pearls and potions.
        self.rp.tick();
        self.thrown.retain_mut(|e| e.tick());
        t
    }
}
