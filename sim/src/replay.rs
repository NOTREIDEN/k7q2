//! Replays a recording of the real game (made by the rig in `parity/`) and
//! compares the simulator with it, tick by tick and bit for bit.
//!
//! A recording holds two independent halves. The client half lists, for
//! every client tick, the packets the real client handled before the tick,
//! the keys and mouse applied, the packets it sent and its state afterwards.
//! The server half does the same for server ticks and both players. Each
//! side of the simulator is fed the recorded packets of its side and must
//! reproduce the recorded state and the recorded outgoing packets, so the
//! two halves need no common clock.
use crate::aabb::Aabb;
use crate::agent::{self, Agent, Duel, ACT_HEADS, OBS_DIM, VIEW_HEAD, VIEW_THROWN};
use crate::client::{Client, ClientCfg, Input};
use crate::mth::{JavaRandom, Rng};
use crate::net::{Bar, Item, Kind, Rows, ThrownEv, C2S, S2C, SLOT_SWORD};
use crate::server::{chunk_of, feather_points, prot_points, Food, KbProfile, Server, ServerCfg, ServerPlayer, MAX_HEALTH};
use crate::world::World;
use std::collections::HashMap;

struct Line {
    tag: String,
    words: Vec<String>,
    kv: HashMap<String, String>,
}

impl Line {
    fn parse(s: &str) -> Line {
        let mut it = s.split_whitespace();
        let tag = it.next().unwrap_or("").to_string();
        let words: Vec<String> = it.map(|w| w.to_string()).collect();
        let mut kv = HashMap::new();
        for w in &words {
            if let Some(i) = w.find('=') {
                kv.insert(w[..i].to_string(), w[i + 1..].to_string());
            }
        }
        Line { tag, words, kv }
    }
    fn s(&self, k: &str) -> &str {
        self.kv.get(k).map(|s| s.as_str()).unwrap_or_else(|| panic!("missing field {k} in {} line", self.tag))
    }
    fn has(&self, k: &str) -> bool {
        self.kv.contains_key(k)
    }
    fn d(&self, k: &str) -> f64 {
        f64::from_bits(u64::from_str_radix(self.s(k), 16).unwrap())
    }
    fn f(&self, k: &str) -> f32 {
        f32::from_bits(u32::from_str_radix(self.s(k), 16).unwrap())
    }
    fn i(&self, k: &str) -> i64 {
        self.s(k).parse().unwrap()
    }
    fn b(&self, k: &str) -> bool {
        self.s(k) != "0"
    }
}

/// The numbers an agent was shown when the first recordings with one were made.
const OBS_RECORDED: usize = 151;
const SPRINT_UUID: &str = "662a6b8d-da3e-4c1c-8813-96ea6097278d";
const SPEED_UUID: &str = "91aeaa56-376b-4498-935b-2f7f68070635";

/// The entity ids of the pearls and potions a recording has shown so far,
/// in the order they appeared. The simulator numbers its own in that order.
#[derive(Default)]
struct PearlIds(Vec<i64>);

impl PearlIds {
    fn get(&self, id: i64) -> Option<u32> {
        self.0.iter().rposition(|&e| e == id).map(|i| i as u32)
    }
    fn add(&mut self, id: i64) -> u32 {
        match self.get(id) {
            Some(i) => i,
            None => {
                self.0.push(id);
                self.0.len() as u32 - 1
            }
        }
    }
}

/// An item stack as the rig writes it: `id/damage/count`, or `-` or `null`.
fn stack(s: &str) -> (Item, u32) {
    let p: Vec<&str> = s.split('/').collect();
    if p.len() != 3 {
        return (Item::Empty, 0);
    }
    match p[0] {
        "276" => (Item::Sword, p[1].parse().unwrap()),
        "368" => (Item::Pearl, p[2].parse().unwrap()),
        // A splash potion of Instant Health II, a potion of Swiftness II
        // to drink, an empty bottle and steak.
        "373" if p[1] == "16421" => (Item::Potion, p[2].parse().unwrap()),
        "373" if p[1] == "8226" => (Item::Drink, p[2].parse().unwrap()),
        "374" => (Item::Bottle, p[2].parse().unwrap()),
        "364" => (Item::Food, p[2].parse().unwrap()),
        other => panic!("an item the simulator does not know: {other}"),
    }
}

/// The hotbar but for the sword, from the nine stacks of the hotbar as the
/// rig writes them.
fn bar_of(hotbar: &str) -> Bar {
    let mut b = Bar::default();
    for (k, e) in hotbar.split(',').enumerate().take(9).skip(1) {
        let (item, n) = stack(e);
        b.put(k as u8, item, n as u8);
    }
    b
}

/// The three rows above the hotbar, as the rig writes them in a state
/// line: for each slot what `Rows` holds for it.
fn rows_of(codes: &str) -> Rows {
    let mut r = Rows::default();
    for (k, e) in codes.split(',').enumerate().take(27) {
        r.0[k] = e.parse().unwrap();
    }
    r
}

/// The whole of the player's own window as an S30 is written: the nine
/// stacks of the hotbar, or the three rows and then the hotbar.
fn window_of(stacks: &str) -> (Bar, Rows) {
    let all: Vec<&str> = stacks.split(',').collect();
    if all.len() < 36 {
        return (bar_of(stacks), Rows::default());
    }
    let mut r = Rows::default();
    for k in 0..27 {
        let (item, n) = stack(all[k]);
        r.put(k, item, n as u8);
    }
    (bar_of(&all[27..36].join(",")), r)
}

/// The stack a client says it has in hand.
fn said(s: &str) -> (Item, u8) {
    match stack(s) {
        (Item::Sword, _) => (Item::Sword, 1),
        (item, n) => (item, n as u8),
    }
}

/// Turn a recorded server-to-client packet into the simulator's packet.
/// `None` means the packet carries nothing the simulator models.
fn s2c(l: &Line, at: usize, own_id: i64, pearls: &mut PearlIds) -> Option<S2C> {
    let class = l.words[at].as_str();
    let id = |k: &str| l.i(k) == own_id;
    if class == "S0EPacketSpawnObject" {
        let kind = match l.i("type") {
            65 => Kind::Pearl,
            73 => Kind::Potion,
            _ => panic!("an object the simulator does not know"),
        };
        let id = pearls.add(l.i("entityId"));
        let p = [l.i("x") as i32, l.i("y") as i32, l.i("z") as i32];
        return Some(S2C::Thrown { id, ev: ThrownEv::Spawn { kind, p, yaw: l.i("yaw") as i8, pitch: l.i("pitch") as i8 } });
    }
    if class == "S13PacketDestroyEntities" {
        let mut ids = [0u32; 8];
        let mut n = 0;
        for e in l.s("entityIDs").split(',').filter(|e| !e.is_empty()) {
            if let Some(i) = pearls.get(e.parse().unwrap()) {
                ids[n] = i;
                n += 1;
            }
        }
        return if n > 0 { Some(S2C::Destroy { ids, n: n as u8 }) } else { None };
    }
    if class == "S30PacketWindowItems" {
        if l.i("windowId") != 0 {
            return None;
        }
        let (bar, rows) = window_of(l.s("itemStacks"));
        return Some(S2C::WindowItems(bar, rows));
    }
    if class == "S04PacketEntityEquipment" {
        if id("entityID") || l.i("equipmentSlot") != 0 {
            return None;
        }
        let (item, n) = stack(l.s("itemStack"));
        return Some(S2C::Equip { item, n });
    }
    // Packets that exist for players and for pearls alike.
    let about = ["entityId", "entityID"].iter().find(|k| l.has(k)).and_then(|k| pearls.get(l.i(k)));
    if let Some(pid) = about {
        let ev = match class {
            "S1CPacketEntityMetadata" => Some(ThrownEv::Meta),
            "S12PacketEntityVelocity" => Some(ThrownEv::Velocity([l.i("motionX") as i32, l.i("motionY") as i32, l.i("motionZ") as i32])),
            "S18PacketEntityTeleport" => Some(ThrownEv::Teleport {
                p: [l.i("posX") as i32, l.i("posY") as i32, l.i("posZ") as i32],
                yaw: l.i("yaw") as i8,
                pitch: l.i("pitch") as i8,
                on_ground: l.b("onGround"),
            }),
            c if c.starts_with("S14PacketEntity") => {
                let d = [l.i("posX") as i8, l.i("posY") as i8, l.i("posZ") as i8];
                let (yaw, pitch, og) = (l.i("yaw") as i8, l.i("pitch") as i8, l.b("onGround"));
                match c {
                    "S14PacketEntity$S15PacketEntityRelMove" => Some(ThrownEv::RelMove { d, on_ground: og }),
                    "S14PacketEntity$S16PacketEntityLook" => Some(ThrownEv::Look { yaw, pitch, on_ground: og }),
                    "S14PacketEntity$S17PacketEntityLookMove" => Some(ThrownEv::LookMove { d, yaw, pitch, on_ground: og }),
                    _ => None,
                }
            }
            _ => None,
        };
        return ev.map(|ev| S2C::Thrown { id: pid, ev });
    }
    if class == "S1CPacketEntityMetadata" {
        let list = l.s("field_149378_b").trim_matches(|c| c == '[' || c == ']');
        let (mut flags, mut health, mut fx) = (None, None, None);
        for e in list.split(';').filter(|e| !e.is_empty()) {
            let p: Vec<&str> = e.split(':').collect();
            if p[0] == "0" {
                let f: i64 = p[2].parse().unwrap();
                flags = Some((f & 8 != 0, f & 16 != 0));
            }
            // Watched value 6 is the health, a float.
            if p[0] == "6" {
                health = Some(f32::from_bits(u32::from_str_radix(p[2], 16).unwrap()));
            }
            // Watched value 7 is the colour of the effect particles, zero
            // without any.
            if p[0] == "7" {
                fx = Some(p[2].parse::<i64>().unwrap() != 0);
            }
        }
        return if flags.is_some() || health.is_some() || fx.is_some() {
            Some(S2C::Meta { own: id("entityId"), flags, health, fx })
        } else {
            None
        };
    }
    if class == "S06PacketUpdateHealth" {
        return Some(S2C::Health { health: l.f("health"), food: l.i("foodLevel") as i32, saturation: l.f("saturationLevel") });
    }
    if class == "S20PacketEntityProperties" {
        let list = l.s("field_149444_b").trim_matches(|c| c == '[' || c == ']');
        for e in list.split(';').filter(|e| !e.is_empty()) {
            let mut parts = e.split(',');
            let head = parts.next().unwrap();
            let (name, base) = head.split_once(':').unwrap();
            if name != "generic.movementSpeed" {
                continue;
            }
            let base = f64::from_bits(u64::from_str_radix(base, 16).unwrap());
            let (mut sprint, mut amp) = (false, -1i8);
            for m in parts {
                let q: Vec<&str> = m.split('/').collect();
                let amount = f64::from_bits(u64::from_str_radix(q[2], 16).unwrap());
                if q[0] == SPRINT_UUID {
                    sprint = true;
                } else if q[0] == SPEED_UUID {
                    amp = (amount / 0.20000000298023224).round() as i8 - 1;
                }
            }
            return Some(S2C::Props { own: id("entityId"), base, speed_amp: amp, sprint });
        }
        return None;
    }
    if class == "S2FPacketSetSlot" {
        // Slot 36 of the player's own inventory is the first hotbar slot,
        // where the sword is, and 37 the second, where the pearls are.
        // Window -1 is the stack on the mouse cursor.
        return match (l.i("windowId"), l.i("slot")) {
            (0, 36) => Some(S2C::SetSlot { slot: SLOT_SWORD, item: Item::Sword, count: 1 }),
            (0, k @ 37..=44) => {
                let (item, n) = stack(l.s("item"));
                Some(S2C::SetSlot { slot: (k - 36) as u8, item, count: n as u8 })
            }
            // The three rows above the hotbar.
            (0, k @ 9..=35) => {
                let (item, n) = stack(l.s("item"));
                Some(S2C::SetSlot { slot: k as u8, item, count: n as u8 })
            }
            (-1, _) => Some(S2C::Cursor),
            _ => None,
        };
    }
    if class == "S32PacketConfirmTransaction" {
        if l.i("windowId") != 0 {
            return None;
        }
        return Some(S2C::Confirm { action: l.i("actionNumber") as u16, ok: l.b("field_148893_c") });
    }
    if class == "S08PacketPlayerPosLook" {
        assert!(l.s("field_179835_f") == "[]", "a relative teleport is not modelled");
        return Some(S2C::PosLook { pos: [l.d("x"), l.d("y"), l.d("z")], yaw: l.f("yaw"), pitch: l.f("pitch") });
    }
    if class == "S12PacketEntityVelocity" {
        return Some(S2C::Velocity {
            own: id("entityID"),
            v: [l.i("motionX") as i32, l.i("motionY") as i32, l.i("motionZ") as i32],
        });
    }
    if class == "S19PacketEntityStatus" {
        return match l.i("logicOpcode") {
            2 => Some(S2C::Hurt { own: id("entityId") }),
            9 if id("entityId") => Some(S2C::UseDone),
            _ => None,
        };
    }
    // Speed is effect 1; the last field says whether its particles show.
    if class == "S1DPacketEntityEffect" {
        return if l.i("effectId") == 1 && id("entityId") {
            Some(S2C::Effect { amp: l.i("amplifier") as i8, ticks: l.i("duration") as i32, shown: l.i("hideParticles") != 0 })
        } else {
            None
        };
    }
    if class == "S1EPacketRemoveEntityEffect" {
        return if l.i("effectId") == 1 && id("entityId") { Some(S2C::EffectEnd) } else { None };
    }
    if class == "S0BPacketAnimation" {
        return if l.i("type") == 0 && !id("entityId") { Some(S2C::Swing) } else { None };
    }
    if class.starts_with("S14PacketEntity") {
        if id("entityId") {
            return None;
        }
        let d = [l.i("posX") as i8, l.i("posY") as i8, l.i("posZ") as i8];
        let (yaw, pitch, og) = (l.i("yaw") as i8, l.i("pitch") as i8, l.b("onGround"));
        return match class {
            "S14PacketEntity$S15PacketEntityRelMove" => Some(S2C::RelMove { d, on_ground: og }),
            "S14PacketEntity$S16PacketEntityLook" => Some(S2C::Look { yaw, pitch, on_ground: og }),
            "S14PacketEntity$S17PacketEntityLookMove" => Some(S2C::LookMove { d, yaw, pitch, on_ground: og }),
            _ => None,
        };
    }
    if class == "S18PacketEntityTeleport" {
        if id("entityId") {
            return None;
        }
        return Some(S2C::Teleport {
            p: [l.i("posX") as i32, l.i("posY") as i32, l.i("posZ") as i32],
            yaw: l.i("yaw") as i8,
            pitch: l.i("pitch") as i8,
            on_ground: l.b("onGround"),
        });
    }
    None
}

/// A recorded client-to-server packet as the simulator's packet.
fn c2s(l: &Line, at: usize) -> Option<C2S> {
    let class = l.words[at].as_str();
    if class.starts_with("C03PacketPlayer") {
        let pos = if l.b("moving") { Some([l.d("x"), l.d("y"), l.d("z")]) } else { None };
        let rot = if l.b("rotating") { Some((l.f("yaw"), l.f("pitch"))) } else { None };
        return Some(C2S::Player { pos, rot, on_ground: l.b("onGround") });
    }
    match class {
        "C0APacketAnimation" => Some(C2S::Swing),
        "C02PacketUseEntity" => match l.s("action") {
            "ATTACK" => Some(C2S::Attack),
            "INTERACT_AT" => Some(C2S::InteractAt),
            "INTERACT" => Some(C2S::Interact),
            _ => None,
        },
        "C08PacketPlayerBlockPlacement" => {
            let st = said(l.s("stack"));
            Some(if l.i("placedBlockDirection") == 255 { C2S::UseItem(st) } else { C2S::UseBlock(st) })
        }
        // Starting and giving up on breaking a block is not modelled.
        "C07PacketPlayerDigging" => if l.s("status") == "RELEASE_USE_ITEM" { Some(C2S::ReleaseUse) } else { None },
        "C09PacketHeldItemChange" => Some(C2S::HeldItem(l.i("slotId") as u8)),
        "C16PacketClientStatus" => if l.s("status") == "OPEN_INVENTORY_ACHIEVEMENT" { Some(C2S::OpenInv) } else { None },
        "C0EPacketClickWindow" => {
            assert!(l.i("windowId") == 0 && l.i("mode") == 1 && l.i("usedButton") == 0, "a click the simulator does not know");
            let said = match stack(l.s("clickedItem")) {
                (Item::Empty, _) => None,
                (item, n) => Some((item, n as u8)),
            };
            Some(C2S::Click { slot: l.i("slotId") as u8, said, action: l.i("actionNumber") as u16 })
        }
        "C0DPacketCloseWindow" => Some(C2S::CloseInv),
        "C0FPacketConfirmTransaction" => Some(C2S::Confirm { action: l.i("uid") as u16 }),
        "C0BPacketEntityAction" => match l.s("action") {
            "START_SPRINTING" => Some(C2S::Sprint(true)),
            "STOP_SPRINTING" => Some(C2S::Sprint(false)),
            _ => None,
        },
        _ => None,
    }
}

fn show_c2s(p: &C2S) -> String {
    match p {
        C2S::Player { pos, rot, on_ground } => format!(
            "Player pos={} rot={} ground={}",
            pos.map(|p| format!("({:.17e},{:.17e},{:.17e})", p[0], p[1], p[2])).unwrap_or("-".into()),
            rot.map(|r| format!("({:?},{:?})", r.0, r.1)).unwrap_or("-".into()),
            on_ground
        ),
        other => format!("{other:?}"),
    }
}

struct Check {
    name: &'static str,
    bad: usize,
    first: Option<String>,
}

/// The result of replaying one half of a recording.
pub struct Report {
    checks: Vec<Check>,
    pub ticks: usize,
    /// Ticks on which anything at all differed from the game.
    pub bad_ticks: usize,
    verbose: bool,
    /// What the recording exercised, for the summary line.
    pub counts: Vec<(&'static str, usize)>,
}

impl Report {
    fn new(verbose: bool) -> Report {
        Report { checks: Vec::new(), ticks: 0, bad_ticks: 0, verbose, counts: Vec::new() }
    }
    fn count(&mut self, name: &'static str, n: usize) {
        match self.counts.iter().position(|c| c.0 == name) {
            Some(i) => self.counts[i].1 += n,
            None => self.counts.push((name, n)),
        }
    }
    fn note(&mut self, name: &'static str, ok: bool, detail: impl FnOnce() -> String) -> bool {
        let c = match self.checks.iter().position(|c| c.name == name) {
            Some(i) => &mut self.checks[i],
            None => {
                self.checks.push(Check { name, bad: 0, first: None });
                self.checks.last_mut().unwrap()
            }
        };
        if !ok {
            c.bad += 1;
            if c.first.is_none() || self.verbose {
                let d = detail();
                if self.verbose {
                    println!("  differs: {name}: {d}");
                }
                if c.first.is_none() {
                    c.first = Some(d);
                }
            }
        }
        ok
    }
    /// One summary line, then one line per kind of difference.
    pub fn describe(&self, title: &str) -> String {
        let seen: Vec<String> = self.counts.iter().map(|c| format!("{} {}", c.1, c.0)).collect();
        let mut s = format!("{title}: {} ticks, {} differ ({})\n", self.ticks, self.bad_ticks, seen.join(", "));
        for c in &self.checks {
            if c.bad > 0 {
                s += &format!("  {:<14} differs on {:>5} ticks; first: {}\n", c.name, c.bad, c.first.as_deref().unwrap_or(""));
            }
        }
        s
    }
    pub fn get(&self, name: &str) -> usize {
        self.counts.iter().find(|c| c.0 == name).map(|c| c.1).unwrap_or(0)
    }
}

fn replay_client(lines: &[Line], world: &World, speed: i8, sens: f32, o: &Options, fmt: i64) -> Report {
    let (resync, verbose) = (o.resync, o.verbose);
    let mut rep = Report::new(verbose);
    let init = match lines.iter().find(|l| l.tag == "CINIT") {
        Some(l) => l,
        None => return rep,
    };
    let own_id = init.i("id");
    let has_remote = init.has("rx");
    let other = if has_remote { [init.d("rx"), init.d("ry"), init.d("rz")] } else { [1000.5, world.floor_y, 1000.5] };
    let (oyaw, opitch) = if has_remote { (init.f("ryaw"), init.f("rpitch")) } else { (0.0, 0.0) };
    let mut c = Client::new([init.d("x"), init.d("y"), init.d("z")], init.f("yaw"), init.f("pitch"), speed, other, oyaw, opitch);
    let load = |c: &mut Client, l: &Line| {
        let lp = &mut c.lp;
        lp.l.body.set_position([l.d("x"), l.d("y"), l.d("z")]);
        lp.l.body.motion = [l.d("mx"), l.d("my"), l.d("mz")];
        lp.l.yaw = l.f("yaw");
        lp.l.pitch = l.f("pitch");
        lp.yaw_head = l.f("hyaw");
        lp.l.body.on_ground = l.b("og");
        lp.l.body.collided_h = l.b("ch");
        lp.l.body.fall_distance = l.f("fall");
        lp.l.sprinting = l.b("spr");
        lp.server_sprint_state = l.b("sss");
        lp.position_update_ticks = l.i("put") as i32;
        lp.last_reported = [l.d("lrx"), l.d("lry"), l.d("lrz")];
        lp.last_yaw = l.f("lryaw");
        lp.last_pitch = l.f("lrpitch");
        lp.sprinting_ticks_left = l.i("stl") as i32;
        lp.sprint_toggle_timer = l.i("stt") as i32;
        lp.l.jump_ticks = l.i("jt") as i32;
        lp.l.jump_factor = l.f("jmf");
        lp.mi_forward = l.f("mif");
        lp.mi_strafe = l.f("mis");
        lp.mi_jump = l.b("mij");
        lp.hurt_time = l.i("ht") as i32;
        if l.has("use") {
            lp.using_item = l.b("use");
            lp.use_gen = lp.gens[lp.held as usize];
            lp.right_click_delay = l.i("rcd") as i32;
            lp.flag_using = l.b("eat");
        }
        if l.has("held") {
            lp.held = l.i("held") as u8;
            lp.synced = l.i("cpi") as u8;
            lp.pearls = l.i("pc") as u8;
            if l.has("pots") {
                lp.potions = l.i("pots") as u16;
            }
            if l.has("dr") {
                lp.drinks = l.i("dr") as u16;
                lp.bottles = l.i("bt") as u16;
                lp.steak = l.i("fd") as u8;
                lp.use_slot = lp.held;
                lp.use_gen = lp.gens[lp.held as usize];
                lp.use_count = l.i("iuc") as i32;
                lp.speed_ticks = l.i("spt") as i32;
                lp.speed_amp = l.i("spa") as i8;
                lp.fx = l.i("fx") != 0;
            }
            if l.has("rows") {
                lp.rows = rows_of(l.s("rows"));
                lp.inv_open = l.b("open");
                lp.action = l.i("tid") as u16;
            }
        }
        if l.has("hp") {
            lp.health = l.f("hp");
            lp.food = l.i("food") as i32;
            lp.saturation = l.f("sat");
        }
    };
    let load_remote = |c: &mut Client, l: &Line| {
        let r = &mut c.rp;
        r.pos = [l.d("rx"), l.d("ry"), l.d("rz")];
        r.prev_pos = r.pos;
        r.bb = Aabb::player(r.pos);
        r.yaw = l.f("ryaw");
        r.pitch = l.f("rpitch");
        r.server_pos = [l.i("rsx") as i32, l.i("rsy") as i32, l.i("rsz") as i32];
        r.increments = l.i("rinc") as i32;
        r.target = [l.d("rtx"), l.d("rty"), l.d("rtz")];
        r.target_yaw = l.d("rtyaw");
        r.target_pitch = l.d("rtpitch");
        r.on_ground = l.b("rog");
        r.sprinting = l.b("rspr");
        r.hurt_time = l.i("rht") as i32;
        r.swing_in_progress = l.b("rswing");
        r.swing_progress_int = l.i("rswp") as i32;
        if l.has("reat") {
            r.using = l.b("reat");
        }
        if l.has("rhp") {
            r.health = l.f("rhp");
        }
        if l.has("rfx") {
            r.fx = l.i("rfx") != 0;
        }
    };
    load(&mut c, init);
    if has_remote {
        load_remote(&mut c, init);
    }
    // The attribute modifiers are not in the snapshot; pick the sprint
    // modifier that reproduces the recorded attribute value.
    c.lp.l.attr.sprint = false;
    if c.lp.l.attr.value() != init.d("attr") {
        c.lp.l.attr.sprint = true;
    }
    let mut cfg = ClientCfg::with_sensitivity(sens);
    cfg.mouse_delay_fix = false;
    let mut out: Vec<C2S> = Vec::new();
    let mut sent: Vec<C2S> = Vec::new();
    let mut ids = PearlIds::default();
    // A recording made with an agent at the keys says so, with the duel the
    // agent was told it is in. An agent here is then shown the same and
    // asked for the same.
    let duel = lines.iter().find(|l| l.tag == "AGENT").map(|l| {
        let d = Duel {
            arena_half: l.d("half"),
            floor_y: l.d("floor"),
            center: if l.has("cx") { [l.d("cx"), l.d("cz")] } else { [0.0, 0.0] },
            real_health: l.b("health"),
            hit_health: l.f("hit"),
            pearl_cooldown: l.i("pearlcd") as i32,
            drink_window_s: l.d("drink"),
        };
        (d, [l.d("ping"), l.d("oping")], l.d("cps"))
    });
    let mut ag = duel.as_ref().map(|(_, ping, cps)| Agent::new(*ping, *cps, c.lp.l.body.pos));
    let mut asked: Option<Input> = None;
    let mut rng = Rng::new(1);
    // Whether what was compared before a tick was the same.
    let mut before_ok = true;
    let mut i = lines.iter().position(|l| l.tag == "CINIT").unwrap() + 1;
    while i < lines.len() {
        let l = &lines[i];
        i += 1;
        match l.tag.as_str() {
            "CP" => {
                if let Some(p) = s2c(l, 1, own_id, &mut ids) {
                    rep.count("packets received", 1);
                    if let S2C::Velocity { own: true, .. } = p {
                        rep.count("knockbacks taken", 1);
                    }
                    if let S2C::PosLook { .. } = p {
                        rep.count("teleports back", 1);
                    }
                    if let S2C::SetSlot { slot: SLOT_SWORD, .. } = p {
                        rep.count("swords sent again", 1);
                    }
                    if let S2C::Health { .. } = p {
                        rep.count("health updates", 1);
                    }
                    if let S2C::Thrown { ev: ThrownEv::Spawn { .. }, .. } = p {
                        rep.count("pearls seen", 1);
                    }
                    if let S2C::Thrown { ev: ThrownEv::RelMove { .. } | ThrownEv::LookMove { .. } | ThrownEv::Teleport { .. }, .. } = p {
                        rep.count("pearl corrections", 1);
                    }
                    c.handle(world, p, &mut out);
                }
            }
            // Sent from inside a packet handler, before the tick.
            "CS" => {
                if let Some(p) = c2s(l, 1) {
                    rep.count("packets sent", 1);
                    if let C2S::Confirm { .. } = p {
                        rep.count("refused clicks answered", 1);
                    }
                    sent.push(p);
                }
            }
            // What the real client held when its packets were in and its
            // tick was to come, as the mod reads it.
            "CV" => {
                let tick = l.words[0].clone();
                let hex = |s: &str| match s.strip_prefix('x') {
                    Some(h) => f64::from_bits(u64::from_str_radix(h, 16).unwrap()),
                    None => s.parse::<f64>().unwrap(),
                };
                let mut want: Vec<f64> = l.s("v").split(',').map(hex).collect();
                let got = agent::view(&c, world, &cfg);
                // The game's numbers of pearls and potions are its own.
                let mut k = VIEW_HEAD;
                while k < want.len() {
                    want[k] = ids.get(want[k] as i64).map(|x| x as f64).unwrap_or(-1.0);
                    k += VIEW_THROWN;
                }
                let off: Vec<usize> = if want.len() != got.len() {
                    vec![VIEW_HEAD - 1]
                } else {
                    (0..got.len()).filter(|&k| got[k].to_bits() != want[k].to_bits()).collect()
                };
                rep.count("views compared", 1);
                rep.count("views with a pearl or a potion", (got.len() > VIEW_HEAD) as usize);
                let same = rep.note("view", off.is_empty(), || {
                    let what: Vec<String> = off
                        .iter()
                        .take(8)
                        .map(|&k| format!("{} {:?} for {:?}", agent::view_name(k), got.get(k).copied().unwrap_or(f64::NAN), want.get(k).copied().unwrap_or(f64::NAN)))
                        .collect();
                    format!("tick {tick}: the simulator has {}", what.join(", "))
                });
                rep.count("views that differ", !same as usize);
                before_ok &= same;
            }
            // What the agent in the real client showed its policy.
            "CO" => {
                if let (Some(a), Some((d, _, _))) = (ag.as_mut(), duel.as_ref()) {
                    let tick = l.words[0].clone();
                    a.before_tick(&c, world);
                    let mut o = [0f32; OBS_DIM];
                    a.write_obs(&c, world, &cfg, d, &mut o);
                    let want: Vec<f32> = l.s("o").split(',').map(|s| f32::from_bits(u32::from_str_radix(s, 16).unwrap())).collect();
                    // A recording made before the agent was shown more has
                    // fewer numbers: the ones it has are compared.
                    let n = want.len().min(OBS_DIM);
                    let mut off: Vec<usize> = (0..n).filter(|&k| want[k].to_bits() != o[k].to_bits()).collect();
                    if want.len() < OBS_RECORDED || want.len() > OBS_DIM {
                        off.push(n.min(OBS_DIM - 1));
                    }
                    rep.count("observations compared", 1);
                    let same = rep.note("observation", off.is_empty(), || {
                        let what: Vec<String> = off.iter().take(8).map(|&k| format!("number {k} {:?} for {:?}", o[k], want.get(k))).collect();
                        format!("tick {tick}: the simulator shows {}", what.join(", "))
                    });
                    rep.count("observations that differ", !same as usize);
                    before_ok &= same;
                }
            }
            // What its policy chose.
            "CA" => {
                if let (Some(a), Some((d, _, _))) = (ag.as_mut(), duel.as_ref()) {
                    let action: Vec<i32> = l.s("a").split(',').map(|s| s.parse().unwrap()).collect();
                    assert!(action.len() <= ACT_HEADS);
                    asked = Some(a.keys(&c, d, &action, &mut rng));
                }
            }
            "CI" => {
                let w = &l.words;
                let n = |k: usize| w[k].parse::<i32>().unwrap();
                let inp = Input {
                    forward: n(1) as i8,
                    strafe: n(2) as i8,
                    jump: n(3) != 0,
                    sprint_key: n(4) != 0,
                    // More than one: that many clicks inside the tick.
                    attack: n(5) != 0,
                    dx: n(6),
                    dy: -n(7),
                    extra_clicks: (n(5).max(1) - 1) as u8,
                    use_item: w.len() > 8 && n(8) != 0,
                    use_again: w.len() > 8 && n(8) == 2,
                    slot_key: if w.len() > 9 { n(9) as u8 } else { 0 },
                    inv: if w.len() > 10 { n(10) as u8 } else { 0 },
                };
                if let Some(k) = asked.take() {
                    let tick = &l.words[0];
                    rep.count("ticks with the agent at the keys", 1);
                    let same = rep.note("keys", k == inp, || format!("tick {tick}: the simulator's agent presses {k:?}, the game's {inp:?}"));
                    rep.count("keys that differ", !same as usize);
                    before_ok &= same;
                }
                let (was_using, was_open) = (c.lp.using_item, c.lp.inv_open);
                let t = c.tick(world, &inp, &cfg, &mut out);
                if let (Some(a), Some((d, _, _))) = (ag.as_mut(), duel.as_ref()) {
                    a.ticked(&c, d, &inp, &t);
                }
                rep.count("clicks", inp.attack as usize + inp.extra_clicks as usize);
                rep.count("ticks with two clicks", (inp.extra_clicks > 0) as usize);
                rep.count("ticks on target", t.pick.entity as usize);
                rep.count("right clicks", t.used as usize);
                rep.count("pearls thrown", t.threw as usize);
                rep.count("potions thrown", t.threw_potion as usize);
                rep.count("ticks with a pearl in flight", c.thrown.iter().any(|e| e.kind == Kind::Pearl) as usize);
                rep.count("ticks with a potion in flight", c.thrown.iter().any(|e| e.kind == Kind::Potion) as usize);
                rep.count("ticks blocking", c.lp.using_item as usize);
                rep.count("clicks lost to a block", t.swallowed as usize);
                // A block that ended without the button being let go: the
                // server was not told.
                rep.count("blocks dropped silently", (was_using && !c.lp.using_item && inp.use_item && inp.inv == 0 && !was_open) as usize);
                rep.count("ticks with the inventory open", (inp.inv != 0 || was_open) as usize);
                rep.count("uses ended by the inventory", (was_using && !c.lp.using_item && inp.inv != 0 && !was_open) as usize);
                // Gather what the real client sent and its state after the tick.
                let mut state = None;
                while i < lines.len() {
                    let m = &lines[i];
                    if m.tag == "CS" {
                        if let Some(p) = c2s(m, 1) {
                            rep.count("packets sent", 1);
                            if p == C2S::Attack {
                                rep.count("attacks", 1);
                            }
                            if let C2S::HeldItem(_) = p {
                                rep.count("slot changes", 1);
                            }
                            if let C2S::Click { said, .. } = p {
                                rep.count("inventory clicks", 1);
                                rep.count("clicks that moved a stack", said.is_some() as usize);
                            }
                            rep.count("inventories opened", (p == C2S::OpenInv) as usize);
                            sent.push(p);
                        }
                    } else if m.tag == "CT" {
                        state = Some(m);
                    } else if m.tag == "CP" || m.tag == "CI" || m.tag == "CV" || m.tag == "CO" || m.tag == "CA" || m.tag == "END" {
                        break;
                    }
                    i += 1;
                }
                let st = match state {
                    Some(s) => s,
                    None => break,
                };
                let tick = l.words[0].clone();
                if verbose {
                    println!("client tick {tick}");
                }
                let lp = &c.lp;
                let mut ok = before_ok;
                before_ok = true;
                let p = lp.l.body.pos;
                let mo = lp.l.body.motion;
                let fd = |name: &'static str, got: f64, want: f64, rep: &mut Report| {
                    rep.note(name, got.to_bits() == want.to_bits(), || format!("tick {tick}: simulator {got:.17e}, game {want:.17e}"))
                };
                ok &= fd("x", p[0], st.d("x"), &mut rep);
                ok &= fd("y", p[1], st.d("y"), &mut rep);
                ok &= fd("z", p[2], st.d("z"), &mut rep);
                ok &= fd("motion x", mo[0], st.d("mx"), &mut rep);
                ok &= fd("motion y", mo[1], st.d("my"), &mut rep);
                ok &= fd("motion z", mo[2], st.d("mz"), &mut rep);
                ok &= fd("yaw", lp.l.yaw as f64, st.f("yaw") as f64, &mut rep);
                ok &= fd("pitch", lp.l.pitch as f64, st.f("pitch") as f64, &mut rep);
                ok &= fd("air control", lp.l.jump_factor as f64, st.f("jmf") as f64, &mut rep);
                ok &= fd("speed attr", lp.l.attr.value(), st.d("attr"), &mut rep);
                let fi = |name: &'static str, got: i64, want: i64, rep: &mut Report| {
                    rep.note(name, got == want, || format!("tick {tick}: simulator {got}, game {want}"))
                };
                ok &= fi("on ground", lp.l.body.on_ground as i64, st.i("og"), &mut rep);
                ok &= fi("sprinting", lp.l.sprinting as i64, st.i("spr"), &mut rep);
                ok &= fi("server sprint", lp.server_sprint_state as i64, st.i("sss"), &mut rep);
                ok &= fi("pos ticks", lp.position_update_ticks as i64, st.i("put"), &mut rep);
                ok &= fi("sprint left", lp.sprinting_ticks_left as i64, st.i("stl"), &mut rep);
                ok &= fi("sprint timer", lp.sprint_toggle_timer as i64, st.i("stt"), &mut rep);
                ok &= fi("hurt time", lp.hurt_time as i64, st.i("ht"), &mut rep);
                ok &= fi("wall contact", lp.l.body.collided_h as i64, st.i("ch"), &mut rep);
                if fmt >= 2 {
                    let kind = if t.pick.entity { 1 } else if t.pick.block { 2 } else { 0 };
                    ok &= fi("pick", kind, st.i("pick"), &mut rep);
                    ok &= fi("blocking", lp.using_item as i64, st.i("use"), &mut rep);
                    ok &= fi("click delay", lp.right_click_delay as i64, st.i("rcd"), &mut rep);
                    ok &= fi("own use flag", lp.flag_using as i64, st.i("eat"), &mut rep);
                    ok &= fd("input forward", lp.mi_forward as f64, st.f("mif") as f64, &mut rep);
                    ok &= fd("input strafe", lp.mi_strafe as f64, st.f("mis") as f64, &mut rep);
                } else {
                    ok &= fi("pick", t.pick.entity as i64, st.i("pick"), &mut rep);
                }
                if fmt >= 3 {
                    ok &= fi("slot in hand", lp.held as i64, st.i("held"), &mut rep);
                    ok &= fi("slot reported", lp.synced as i64, st.i("cpi"), &mut rep);
                    ok &= fi("pearls left", lp.pearls as i64, st.i("pc"), &mut rep);
                    if st.has("pots") {
                        ok &= fi("potions left", lp.potions as i64, st.i("pots"), &mut rep);
                    }
                    if st.has("dr") {
                        ok &= fi("drinks left", lp.drinks as i64, st.i("dr"), &mut rep);
                        ok &= fi("bottles", lp.bottles as i64, st.i("bt"), &mut rep);
                        ok &= fi("steak left", lp.steak as i64, st.i("fd"), &mut rep);
                        if lp.using_item {
                            ok &= fi("ticks of use left", lp.use_count as i64, st.i("iuc"), &mut rep);
                        }
                        ok &= fi("ticks of speed left", lp.speed_ticks as i64, st.i("spt"), &mut rep);
                        ok &= fi("effect particles", lp.fx as i64, (st.i("fx") != 0) as i64, &mut rep);
                        ok &= fi("food", lp.food as i64, st.i("food"), &mut rep);
                        ok &= fd("saturation", lp.saturation as f64, st.f("sat") as f64, &mut rep);
                    }
                    if st.has("rows") {
                        let want = rows_of(st.s("rows"));
                        for k in 0..27 {
                            ok &= fi("inventory rows", lp.rows.0[k] as i64, want.0[k] as i64, &mut rep);
                        }
                        ok &= fi("inventory open", lp.inv_open as i64, st.i("open"), &mut rep);
                        ok &= fi("click number", lp.action as i64, st.i("tid"), &mut rep);
                    }
                    // The pearls and potions in flight, as the game client
                    // has them.
                    let list = st.s("pe").trim_matches(|ch| ch == '[' || ch == ']');
                    let want: Vec<&str> = list.split(';').filter(|e| !e.is_empty()).collect();
                    let hex = |s: &str| f64::from_bits(u64::from_str_radix(s, 16).unwrap());
                    let mut same = want.len() == c.thrown.len();
                    if same {
                        for (e, w) in c.thrown.iter().zip(&want) {
                            let q: Vec<&str> = w.split(':').collect();
                            let pos = [hex(q[1]), hex(q[2]), hex(q[3])];
                            let mo = [hex(q[4]), hex(q[5]), hex(q[6])];
                            let sp: [i32; 3] = [q[7].parse().unwrap(), q[8].parse().unwrap(), q[9].parse().unwrap()];
                            same &= ids.get(q[0].parse().unwrap()) == Some(e.id);
                            same &= (0..3).all(|a| e.pos[a].to_bits() == pos[a].to_bits() && e.motion[a].to_bits() == mo[a].to_bits());
                            same &= e.server_pos == sp;
                        }
                    }
                    ok &= rep.note("pearls", same, || {
                        let got: Vec<String> = c
                            .thrown
                            .iter()
                            .map(|e| {
                                format!(
                                    "{}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{}:{}:{}",
                                    ids.0[e.id as usize],
                                    e.pos[0].to_bits(),
                                    e.pos[1].to_bits(),
                                    e.pos[2].to_bits(),
                                    e.motion[0].to_bits(),
                                    e.motion[1].to_bits(),
                                    e.motion[2].to_bits(),
                                    e.server_pos[0],
                                    e.server_pos[1],
                                    e.server_pos[2]
                                )
                            })
                            .collect();
                        format!("tick {tick}: simulator [{}], game [{}]", got.join(";"), want.join(";"))
                    });
                }
                if fmt >= 4 {
                    ok &= fd("health", lp.health as f64, st.f("hp") as f64, &mut rep);
                    ok &= fd("saturation", lp.saturation as f64, st.f("sat") as f64, &mut rep);
                    ok &= fi("food", lp.food as i64, st.i("food"), &mut rep);
                    if st.has("rhp") {
                        ok &= fd("other health", c.rp.health as f64, st.f("rhp") as f64, &mut rep);
                    }
                }
                ok &= rep.note("packets sent", out == sent, || {
                    format!(
                        "tick {tick}: simulator [{}], game [{}]",
                        out.iter().map(show_c2s).collect::<Vec<_>>().join("; "),
                        sent.iter().map(show_c2s).collect::<Vec<_>>().join("; ")
                    )
                });
                if has_remote && st.has("rx") {
                    let r = &c.rp;
                    ok &= fd("other x", r.pos[0], st.d("rx"), &mut rep);
                    ok &= fd("other y", r.pos[1], st.d("ry"), &mut rep);
                    ok &= fd("other z", r.pos[2], st.d("rz"), &mut rep);
                    ok &= fd("other yaw", r.yaw as f64, st.f("ryaw") as f64, &mut rep);
                    ok &= fd("other pitch", r.pitch as f64, st.f("rpitch") as f64, &mut rep);
                    ok &= fi("other hurt", r.hurt_time as i64, st.i("rht"), &mut rep);
                    ok &= fi("other steps", r.increments as i64, st.i("rinc"), &mut rep);
                    ok &= fi("other sprint", r.sprinting as i64, st.i("rspr"), &mut rep);
                    ok &= fi("other swing", r.swing_in_progress as i64, st.i("rswing"), &mut rep);
                    ok &= fi("other swing t", r.swing_progress_int as i64, st.i("rswp"), &mut rep);
                    if st.has("reat") {
                        ok &= fi("other use flag", r.using as i64, st.i("reat"), &mut rep);
                    }
                }
                rep.ticks += 1;
                out.clear();
                sent.clear();
                if !ok {
                    rep.bad_ticks += 1;
                    if resync {
                        load(&mut c, st);
                        c.lp.l.attr.sprint = false;
                        if c.lp.l.attr.value() != st.d("attr") {
                            c.lp.l.attr.sprint = true;
                        }
                        if has_remote && st.has("rx") {
                            load_remote(&mut c, st);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    rep
}

/// What a packet of the server is about: the player that receives it (0),
/// the other player (1) or the ender pearls (2). The game sends the three
/// kinds from different places, and the order between pearls depends on how
/// a hash set happens to list them, so each kind is compared on its own and
/// the pearl packets pearl by pearl.
fn about(p: &S2C) -> u8 {
    match p {
        S2C::Velocity { own, .. } | S2C::Hurt { own } | S2C::Meta { own, .. } | S2C::Props { own, .. } => !*own as u8,
        S2C::PosLook { .. } | S2C::SetSlot { .. } | S2C::WindowItems { .. } | S2C::Cursor | S2C::Destroy { .. } | S2C::Health { .. } => 0,
        S2C::Confirm { .. } => 0,
        S2C::UseDone | S2C::Effect { .. } | S2C::EffectEnd => 0,
        S2C::Thrown { .. } => 2,
        _ => 1,
    }
}

fn load_server(p: &mut ServerPlayer, l: &Line) {
    p.l.body.set_position([l.d("x"), l.d("y"), l.d("z")]);
    p.l.body.motion = [l.d("mx"), l.d("my"), l.d("mz")];
    p.l.yaw = l.f("yaw");
    p.l.pitch = l.f("pitch");
    p.l.body.on_ground = l.b("og");
    p.l.body.fall_distance = l.f("fall");
    p.l.sprinting = l.b("spr");
    p.l.attr.sprint = false;
    if p.l.attr.value() != l.d("attr") {
        p.l.attr.sprint = true;
    }
    p.l.jump_ticks = l.i("jt") as i32;
    p.l.jump_factor = l.f("jmf");
    p.hurt_resistant_time = l.i("hrt") as i32;
    p.hurt_time = l.i("ht") as i32;
    p.last_damage = l.f("ld");
    p.health = l.f("hp");
    p.velocity_changed = l.b("vc");
    p.is_airborne = l.b("ab");
    p.swing_in_progress = l.b("swing");
    p.swing_progress_int = l.i("swp") as i32;
    if l.has("use") {
        p.using_item = l.b("use");
    }
    if l.has("held") {
        p.held = l.i("held") as u8;
        p.pearls = l.i("pc") as u8;
        if l.has("pots") {
            p.potions = l.i("pots") as u16;
        }
        if l.has("dr") {
            p.drinks = l.i("dr") as u16;
            p.bottles = l.i("bt") as u16;
            p.steak = l.i("fd") as u8;
            p.use_slot = p.held;
            p.use_count = l.i("iuc") as i32;
            p.speed_ticks = (l.i("spt") as i32).max(0);
            p.fx = l.i("fx") != 0;
            p.speed_shown = p.fx;
        }
        if l.has("rows") {
            p.rows = rows_of(l.s("rows"));
        }
        // The wear is recorded for the stack in hand only.
        if p.held == SLOT_SWORD && l.i("sd") >= 0 {
            p.sword_wear = l.i("sd") as u32;
        }
        // The attack damage tells whether the last update saw the sword.
        p.equip = match (l.d("ad") > 1.0, p.in_hand()) {
            (true, _) => (Item::Sword, p.sword_wear),
            (false, (Item::Sword, _)) => (Item::Empty, 0),
            (false, other) => other,
        };
    }
    if l.has("sat") {
        p.food = Food { level: l.i("food") as i32, saturation: l.f("sat"), exhaustion: l.f("exh"), timer: l.i("ft") as i32 };
        p.last_health = l.f("lhp");
        p.last_food = l.i("lfood") as i32;
        p.was_hungry = l.b("wh");
    } else {
        p.last_health = p.health;
    }
    p.dirty = 0;
    p.fx_dirty = false;
    p.meta_dirty = false;
    p.health_dirty = false;
    p.attr_dirty = false;
    if l.has("hm") {
        p.has_moved = l.b("hm");
        p.last_pos = [l.d("lpx"), l.d("lpy"), l.d("lpz")];
        p.net_ticks = l.i("ntc") as i32;
        p.last_move_tick = l.i("lmt") as i32;
        p.chunk = [l.i("ccx") as i32, l.i("ccy") as i32, l.i("ccz") as i32];
    } else {
        p.has_moved = true;
        p.last_pos = p.l.body.pos;
        p.chunk = chunk_of(p.l.body.pos);
    }
    if l.b("tr") {
        p.update_counter = l.i("uc") as i32;
        p.ticks_since_teleport = l.i("tsl") as i32;
        p.enc_pos = [l.i("ex") as i32, l.i("ey") as i32, l.i("ez") as i32];
        p.enc_yaw = l.i("eyaw") as i32;
        p.enc_pitch = l.i("epitch") as i32;
        p.tracker_on_ground = l.b("tog");
    }
}

/// The server half: feed the recorded packets of both players to the
/// simulator's server and compare its state and everything it sends.
fn replay_server(lines: &[Line], world: &World, speed: i8, o: &Options, wear: f32, fmt: i64, head: &Line) -> Report {
    let (resync, verbose) = (o.resync, o.verbose);
    let mut rep = Report::new(verbose);
    let inits: Vec<&Line> = lines.iter().filter(|l| l.tag == "SINIT").collect();
    if inits.is_empty() {
        return rep;
    }
    let n = inits.len();
    let timed = inits[0].has("spt");
    let player_ids: Vec<i64> = inits.iter().map(|l| l.i("id")).collect();
    let mk = |l: Option<&&Line>| match l {
        Some(l) => {
            let mut p = ServerPlayer::new([l.d("x"), l.d("y"), l.d("z")], l.f("yaw"), l.f("pitch"), speed);
            load_server(&mut p, l);
            p
        }
        // Nobody else in the world: park the second player far away.
        None => ServerPlayer::new([1000.5, world.floor_y, 1000.5], 0.0, 0.0, speed),
    };
    let mut cfg = ServerCfg { infinite_health: true, sword_wear: wear, ..ServerCfg::default() };
    if fmt >= 4 {
        // What the players wear and whether a hit costs health.
        cfg.infinite_health = head.i("res") != 0;
        cfg.armor = head.i("armor") as i32;
        cfg.prot = 4 * prot_points(head.i("prot") as i32);
        cfg.feather = feather_points(head.i("ff") as i32);
        cfg.ench_damage = head.i("sharp") as f32 * 1.25f32;
    }
    let mut kb = KbProfile::VANILLA;
    if o.wrong_knockback {
        // Proves that the comparison notices a small error.
        kb.horizontal = 0.41;
    }
    let mut sv = Server::new(mk(inits.get(0)), mk(inits.get(1)), kb, cfg);
    if fmt >= 4 {
        // The rig seeds the generator Protection draws from.
        sv.ench_rand = Some(JavaRandom::new(head.i("eseed")));
    }
    let mut rng = Rng::new(1);
    let mut out: Vec<(usize, S2C)> = Vec::new();
    let mut want: Vec<(usize, S2C)> = Vec::new();
    let mut i = lines.iter().rposition(|l| l.tag == "SINIT").unwrap() + 1;
    let mut states: Vec<&Line> = Vec::new();
    // The game adds a random part to the direction of a throw. The
    // recording has the result, and the simulator is given it; where the
    // pearl starts and the direction without the random part are compared.
    let mut ids = PearlIds::default();
    let mut spawns: Vec<&Line> = Vec::new();
    for l in &lines[i..] {
        if l.tag == "SS" && l.words[2] == "S0EPacketSpawnObject" && ids.get(l.i("entityId")).is_none() {
            ids.add(l.i("entityId"));
            spawns.push(l);
            sv.throw_motions.push_back([l.d("emx"), l.d("emy"), l.d("emz")]);
        }
    }
    let mut ids = PearlIds::default();
    let mut pearl_states: Vec<&Line> = Vec::new();
    let mut thrown = 0usize;
    // Age of each pearl after the previous tick. A pearl whose age stands
    // still was not updated by the game: it was within 32 blocks of a chunk
    // the rig's small world had not loaded. The simulator is told to leave
    // such a pearl alone on that tick too.
    let mut ages: HashMap<i64, i64> = HashMap::new();
    let mut tick = String::new();
    let mut handled_any = false;
    let mut last_flushed = String::new();
    loop {
        // One server tick: its packets in order, then the state lines.
        let done = i >= lines.len();
        let l = if done { None } else { Some(&lines[i]) };
        let flush = match l {
            None => !states.is_empty(),
            // Server lines after the state lines belong to the next tick (or
            // are strays written while the recording was shutting down).
            Some(l) => !states.is_empty() && (l.tag == "SP" || l.tag == "SS" || l.tag == "SH" || (l.tag == "ST" && l.words[0] != tick)),
        };
        if flush {
            sv.frozen.clear();
            for st in &pearl_states {
                let (id, age) = (st.i("id"), st.i("tia"));
                if ages.get(&id).copied().unwrap_or(0) == age {
                    if let Some(k) = ids.get(id) {
                        sv.frozen.push(k);
                    }
                }
            }
            ages = pearl_states.iter().map(|st| (st.i("id"), st.i("tia"))).collect();
            rep.count("pearl ticks outside the loaded world", sv.frozen.len());
            sv.tick(world, &mut rng, &mut out);
            if fmt < 3 {
                // Older recordings did not note what the other player holds.
                out.retain(|(_, p)| !matches!(p, S2C::Equip { .. }));
            }
            if !timed {
                // Nor how long the Speed of the start had been running, on
                // which it depends when the player is told of it again.
                out.retain(|(_, p)| !matches!(p, S2C::Effect { .. }));
                want.retain(|(_, p)| !matches!(p, S2C::Effect { .. }));
            }
            rep.count("packets out", want.len());
            rep.count("teleports back", want.iter().filter(|w| matches!(w.1, S2C::PosLook { .. })).count());
            rep.count("hits", sv.hits.len());
            rep.count("sprint hits", sv.hits.iter().filter(|h| h.sprint).count());
            rep.count("critical hits", sv.hits.iter().filter(|h| h.crit).count());
            rep.count("blocked hits", sv.hits.iter().filter(|h| h.blocked).count());
            rep.count("swords sent again", want.iter().filter(|w| matches!(w.1, S2C::SetSlot { slot: SLOT_SWORD, .. })).count());
            rep.count("hits without the sword", sv.hits.iter().filter(|h| h.damage <= 1.5).count());
            rep.count("health lost, in tenths", sv.p.iter().map(|p| (p.health_lost * 10.0).round() as usize).sum());
            rep.count("health got back", sv.p.iter().map(|p| p.health_gained.round() as usize).sum());
            for p in sv.p.iter_mut() {
                p.health_lost = 0.0;
                p.health_gained = 0.0;
            }
            rep.count("health updates", want.iter().filter(|w| matches!(w.1, S2C::Health { .. })).count());
            rep.count("pearls landed", sv.landings.len());
            rep.count("pearls landed on a player", sv.landings.iter().filter(|l| l.on_player).count());
            rep.count("ticks with a pearl in flight", sv.thrown.iter().any(|e| e.kind == Kind::Pearl) as usize);
            rep.count("ticks with a potion in flight", sv.thrown.iter().any(|e| e.kind == Kind::Potion) as usize);
            rep.count("potions broken", sv.splashes.len());
            rep.count("potions broken on a player", sv.splashes.iter().filter(|s| s.on_player.is_some()).count());
            rep.count("health from potions, in tenths", sv.splashes.iter().map(|s| ((s.healed[0] + s.healed[1]) * 10.0).round() as usize).sum());
            rep.count("potions that healed both", sv.splashes.iter().filter(|s| s.offered[0] > 0.0 && s.offered[1] > 0.0).count());
            sv.splashes.clear();
            sv.hits.clear();
            sv.landings.clear();
            if verbose {
                println!("server tick {tick}");
            }
            let mut ok = true;
            for dest in 0..n {
                for kind in 0..3u8 {
                    if n == 1 && kind == 1 {
                        continue; // nobody else is there to be told about
                    }
                    let pick = |v: &Vec<(usize, S2C)>| -> Vec<S2C> {
                        let mut v: Vec<S2C> = v.iter().filter(|(d, p)| *d == dest && about(p) == kind).map(|(_, p)| *p).collect();
                        if kind == 2 {
                            v.sort_by_key(|p| match p {
                                S2C::Thrown { id, .. } => *id,
                                _ => 0,
                            });
                        }
                        v
                    };
                    let (got, exp) = (pick(&out), pick(&want));
                    let name = match (dest, kind) {
                        (0, 0) => "sent to 0 own",
                        (0, 1) => "sent to 0 other",
                        (0, _) => "sent to 0 pearl",
                        (_, 0) => "sent to 1 own",
                        (_, 1) => "sent to 1 other",
                        (_, _) => "sent to 1 pearl",
                    };
                    ok &= rep.note(name, got == exp, || format!("tick {tick}: simulator {got:?}, game {exp:?}"));
                }
            }
            // Pearls thrown on this tick: where they started, which way
            // they point, and that the random part of the direction is of
            // the size the game gives it.
            while thrown < sv.next_thrown_id as usize && thrown < spawns.len() {
                let sp = spawns[thrown];
                let e = sv.spawned[thrown].clone();
                thrown += 1;
                let fd = |name: &'static str, got: f64, want: f64, rep: &mut Report| {
                    rep.note(name, got.to_bits() == want.to_bits(), || format!("tick {tick}: simulator {got:.17e}, game {want:.17e}"))
                };
                ok &= fd("throw x", e.pos[0], sp.d("ex"), &mut rep);
                ok &= fd("throw y", e.pos[1], sp.d("ey"), &mut rep);
                ok &= fd("throw z", e.pos[2], sp.d("ez"), &mut rep);
                ok &= fd("throw yaw", e.yaw as f64, sp.f("eyaw") as f64, &mut rep);
                ok &= fd("throw pitch", e.pitch as f64, sp.f("epitch") as f64, &mut rep);
                let m = [sp.d("emx"), sp.d("emy"), sp.d("emz")];
                let off = (0..3).map(|a| (m[a] - e.aim[a]).abs()).fold(0.0, f64::max);
                let is_potion = sp.i("type") == 73;
                ok &= rep.note("throw kind", (e.kind == Kind::Potion) == is_potion, || format!("tick {tick}: simulator {:?}, game type {}", e.kind, sp.i("type")));
                ok &= rep.note("throw aim", off < 0.0075 * e.kind.speed() as f64 * 6.0, || {
                    format!("tick {tick}: simulator {:?} before the random part, game {:?} after it", e.aim, m)
                });
            }
            ok &= rep.note("pearls in flight", pearl_states.len() == sv.thrown.len(), || {
                format!("tick {tick}: simulator {}, game {}", sv.thrown.len(), pearl_states.len())
            });
            for st in &pearl_states {
                let e = match ids.get(st.i("id")).and_then(|id| sv.thrown.iter().find(|e| e.id == id)) {
                    Some(e) => e,
                    None => {
                        ok &= rep.note("pearl missing", false, || format!("tick {tick}: the game has pearl {}", st.s("id")));
                        continue;
                    }
                };
                let pid = e.id;
                let fd = |name: &'static str, got: f64, want: f64, rep: &mut Report| {
                    rep.note(name, got.to_bits() == want.to_bits(), || {
                        format!("tick {tick} pearl {pid}: simulator {got:.17e}, game {want:.17e}")
                    })
                };
                ok &= fd("pearl x", e.pos[0], st.d("x"), &mut rep);
                ok &= fd("pearl y", e.pos[1], st.d("y"), &mut rep);
                ok &= fd("pearl z", e.pos[2], st.d("z"), &mut rep);
                ok &= fd("pearl motion x", e.motion[0], st.d("mx"), &mut rep);
                ok &= fd("pearl motion y", e.motion[1], st.d("my"), &mut rep);
                ok &= fd("pearl motion z", e.motion[2], st.d("mz"), &mut rep);
                ok &= fd("pearl yaw", e.yaw as f64, st.f("yaw") as f64, &mut rep);
                ok &= fd("pearl pitch", e.pitch as f64, st.f("pitch") as f64, &mut rep);
                ok &= fd("pearl sent mx", e.last_motion[0], st.d("tmx"), &mut rep);
                ok &= fd("pearl sent my", e.last_motion[1], st.d("tmy"), &mut rep);
                ok &= fd("pearl sent mz", e.last_motion[2], st.d("tmz"), &mut rep);
                let fi = |name: &'static str, got: i64, want: i64, rep: &mut Report| {
                    rep.note(name, got == want, || format!("tick {tick} pearl {pid}: simulator {got}, game {want}"))
                };
                ok &= fi("pearl thrower", e.thrower as i64, st.i("th"), &mut rep);
                if st.has("kind") {
                    ok &= fi("pearl or potion", (e.kind == Kind::Potion) as i64, st.i("kind"), &mut rep);
                }
                ok &= fi("pearl age", e.ticks_in_air as i64, st.i("tia"), &mut rep);
                ok &= fi("pearl count", e.update_counter as i64, st.i("uc"), &mut rep);
                ok &= fi("pearl tele", e.ticks_since_teleport as i64, st.i("tsl"), &mut rep);
                ok &= fi("pearl tr x", e.enc_pos[0] as i64, st.i("ex"), &mut rep);
                ok &= fi("pearl tr y", e.enc_pos[1] as i64, st.i("ey"), &mut rep);
                ok &= fi("pearl tr z", e.enc_pos[2] as i64, st.i("ez"), &mut rep);
                ok &= fi("pearl tr yaw", e.enc_yaw as i64, st.i("eyaw"), &mut rep);
                ok &= fi("pearl tr pitch", e.enc_pitch as i64, st.i("epitch"), &mut rep);
            }
            for st in &states {
                let w = st.i("who") as usize;
                let p = &sv.p[w];
                let fd = |name: &'static str, got: f64, want: f64, rep: &mut Report| {
                    rep.note(name, got.to_bits() == want.to_bits(), || {
                        format!("tick {tick} player {w}: simulator {got:.17e}, game {want:.17e}")
                    })
                };
                let b = &p.l.body;
                ok &= fd("x", b.pos[0], st.d("x"), &mut rep);
                ok &= fd("y", b.pos[1], st.d("y"), &mut rep);
                ok &= fd("z", b.pos[2], st.d("z"), &mut rep);
                ok &= fd("motion x", b.motion[0], st.d("mx"), &mut rep);
                ok &= fd("motion y", b.motion[1], st.d("my"), &mut rep);
                ok &= fd("motion z", b.motion[2], st.d("mz"), &mut rep);
                ok &= fd("yaw", p.l.yaw as f64, st.f("yaw") as f64, &mut rep);
                ok &= fd("pitch", p.l.pitch as f64, st.f("pitch") as f64, &mut rep);
                ok &= fd("air control", p.l.jump_factor as f64, st.f("jmf") as f64, &mut rep);
                ok &= fd("speed attr", p.l.attr.value(), st.d("attr"), &mut rep);
                ok &= fd("fall distance", b.fall_distance as f64, st.f("fall") as f64, &mut rep);
                ok &= fd("last damage", p.last_damage as f64, st.f("ld") as f64, &mut rep);
                let fi = |name: &'static str, got: i64, want: i64, rep: &mut Report| {
                    rep.note(name, got == want, || format!("tick {tick} player {w}: simulator {got}, game {want}"))
                };
                ok &= fi("on ground", b.on_ground as i64, st.i("og"), &mut rep);
                ok &= fi("sprinting", p.l.sprinting as i64, st.i("spr"), &mut rep);
                ok &= fi("no-damage time", p.hurt_resistant_time as i64, st.i("hrt"), &mut rep);
                ok &= fi("hurt time", p.hurt_time as i64, st.i("ht"), &mut rep);
                ok &= fi("velocity flag", p.velocity_changed as i64, st.i("vc"), &mut rep);
                ok &= fi("airborne flag", p.is_airborne as i64, st.i("ab"), &mut rep);
                ok &= fi("jump ticks", p.l.jump_ticks as i64, st.i("jt"), &mut rep);
                ok &= fi("swing", p.swing_in_progress as i64, st.i("swing"), &mut rep);
                ok &= fi("swing time", p.swing_progress_int as i64, st.i("swp"), &mut rep);
                if st.has("blk") {
                    ok &= fi("blocking", p.blocking() as i64, st.i("blk"), &mut rep);
                    ok &= fi("use flag", p.using_item as i64, st.i("eat"), &mut rep);
                }
                if st.has("held") {
                    ok &= fi("slot in hand", p.held as i64, st.i("held"), &mut rep);
                    ok &= fi("pearls left", p.pearls as i64, st.i("pc"), &mut rep);
                    if st.has("pots") {
                        ok &= fi("potions left", p.potions as i64, st.i("pots"), &mut rep);
                    }
                    if st.has("dr") {
                        ok &= fi("drinks left", p.drinks as i64, st.i("dr"), &mut rep);
                        ok &= fi("bottles", p.bottles as i64, st.i("bt"), &mut rep);
                        ok &= fi("steak left", p.steak as i64, st.i("fd"), &mut rep);
                        if p.using_item {
                            ok &= fi("ticks of use left", p.use_count as i64, st.i("iuc"), &mut rep);
                        }
                        ok &= fi("ticks of speed left", if p.speed_ticks > 0 { p.speed_ticks as i64 } else { -1 }, st.i("spt"), &mut rep);
                        ok &= fi("effect particles", p.fx as i64, (st.i("fx") != 0) as i64, &mut rep);
                    }
                    if st.has("rows") {
                        let want = rows_of(st.s("rows"));
                        for k in 0..27 {
                            ok &= fi("inventory rows", p.rows.0[k] as i64, want.0[k] as i64, &mut rep);
                        }
                        ok &= fi("clicks count", p.can_click as i64, st.i("cc"), &mut rep);
                    }
                    if p.held == SLOT_SWORD {
                        ok &= fi("sword wear", p.sword_wear as i64, st.i("sd"), &mut rep);
                    }
                    let ad = if p.equip.0 == Item::Sword { sv.cfg.damage as f64 } else { 1.0 };
                    ok &= fd("attack damage", ad, st.d("ad"), &mut rep);
                }
                if st.has("sat") {
                    ok &= fd("health", p.health as f64, st.f("hp") as f64, &mut rep);
                    ok &= fi("food", p.food.level as i64, st.i("food"), &mut rep);
                    // With infinite health the rig keeps the players fed
                    // with the Saturation effect, and the simulator leaves
                    // the food alone.
                    if !sv.cfg.infinite_health {
                        ok &= fd("saturation", p.food.saturation as f64, st.f("sat") as f64, &mut rep);
                        ok &= fd("exhaustion", p.food.exhaustion as f64, st.f("exh") as f64, &mut rep);
                        ok &= fi("food timer", p.food.timer as i64, st.i("ft"), &mut rep);
                    }
                    ok &= fd("health told", p.last_health as f64, st.f("lhp") as f64, &mut rep);
                    ok &= fi("food told", p.last_food as i64, st.i("lfood"), &mut rep);
                }
                if st.has("hm") {
                    ok &= fi("has moved", p.has_moved as i64, st.i("hm"), &mut rep);
                    ok &= fd("last x", p.last_pos[0], st.d("lpx"), &mut rep);
                    ok &= fd("last y", p.last_pos[1], st.d("lpy"), &mut rep);
                    ok &= fd("last z", p.last_pos[2], st.d("lpz"), &mut rep);
                    ok &= fi("net ticks", p.net_ticks as i64, st.i("ntc"), &mut rep);
                    ok &= fi("last move", p.last_move_tick as i64, st.i("lmt"), &mut rep);
                    ok &= fi("chunk x", p.chunk[0] as i64, st.i("ccx"), &mut rep);
                    ok &= fi("chunk y", p.chunk[1] as i64, st.i("ccy"), &mut rep);
                    ok &= fi("chunk z", p.chunk[2] as i64, st.i("ccz"), &mut rep);
                }
                if st.b("tr") {
                    ok &= fi("tracker count", p.update_counter as i64, st.i("uc"), &mut rep);
                    ok &= fi("tracker tele", p.ticks_since_teleport as i64, st.i("tsl"), &mut rep);
                    ok &= fi("tracker x", p.enc_pos[0] as i64, st.i("ex"), &mut rep);
                    ok &= fi("tracker y", p.enc_pos[1] as i64, st.i("ey"), &mut rep);
                    ok &= fi("tracker z", p.enc_pos[2] as i64, st.i("ez"), &mut rep);
                    ok &= fi("tracker yaw", p.enc_yaw as i64, st.i("eyaw"), &mut rep);
                    ok &= fi("tracker pitch", p.enc_pitch as i64, st.i("epitch"), &mut rep);
                    ok &= fi("tracker ground", p.tracker_on_ground as i64, st.i("tog"), &mut rep);
                }
            }
            rep.ticks += 1;
            if !ok {
                rep.bad_ticks += 1;
                if resync {
                    for st in &states {
                        let w = st.i("who") as usize;
                        load_server(&mut sv.p[w], st);
                    }
                }
            }
            out.clear();
            want.clear();
            states.clear();
            pearl_states.clear();
            last_flushed = tick.clone();
        }
        let l = match l {
            Some(l) => l,
            None => break,
        };
        i += 1;
        match l.tag.as_str() {
            "SP" => {
                if l.words[0] == tick && rep.ticks > 0 && last_flushed == tick {
                    continue;
                }
                tick = l.words[0].clone();
                let who = l.words[1].parse::<usize>().unwrap();
                if let Some(p) = c2s(l, 2) {
                    rep.count("packets in", 1);
                    if p == C2S::Attack {
                        rep.count("attacks", 1);
                    }
                    if let C2S::UseItem(_) = p {
                        rep.count(
                            match sv.p[who].in_hand().0 {
                                Item::Pearl => "pearls thrown",
                                Item::Potion => "potions thrown",
                                Item::Drink => "drinks begun",
                                Item::Food => "clicks with steak",
                                Item::Bottle => "clicks with a bottle",
                                _ => "blocks",
                            },
                            1,
                        );
                    }
                    if let C2S::HeldItem(_) = p {
                        rep.count("slot changes", 1);
                    }
                    rep.count("inventories opened", (p == C2S::OpenInv) as usize);
                    let click = if let C2S::Click { slot, .. } = p { Some(slot) } else { None };
                    let (moved, could, using) = (sv.p[who].clicks_done, sv.p[who].can_click, sv.p[who].using_item);
                    sv.handle(who, p, world, &mut rng, &mut out);
                    if let Some(slot) = click {
                        let did = sv.p[who].clicks_done > moved;
                        rep.count("inventory clicks", 1);
                        rep.count("potions taken from the rows", (did && slot < 36) as usize);
                        rep.count("stacks put away", (did && slot >= 36) as usize);
                        rep.count("clicks that moved nothing", (could && !did) as usize);
                        rep.count("clicks refused", (could && !sv.p[who].can_click) as usize);
                        rep.count("clicks not looked at", !could as usize);
                        rep.count("clicks on the item in use", (did && using && sv.p[who].use_moved) as usize);
                    }
                    handled_any = true;
                }
            }
            "SS" => {
                if l.words[0] == tick && rep.ticks > 0 && last_flushed == tick {
                    continue;
                }
                tick = l.words[0].clone();
                let who = l.words[1].parse::<usize>().unwrap();
                if let Some(p) = s2c(l, 2, player_ids[who], &mut ids) {
                    want.push((who, p));
                }
            }
            "ST" => {
                tick = l.words[0].clone();
                states.push(l);
            }
            "SE" => pearl_states.push(l),
            // The rig gave a player its health back, before the packets of
            // this tick.
            "SH" => {
                let who = l.words[1].parse::<usize>().unwrap();
                sv.p[who].set_health(MAX_HEALTH);
                rep.count("refills by the rig", 1);
            }
            _ => {}
        }
    }
    let _ = handled_any;
    rep
}


#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// After a tick that differs, continue from the recorded state, so that
    /// one difference is not counted again on every later tick.
    pub resync: bool,
    /// Print every tick and every difference.
    pub verbose: bool,
    /// Use a slightly wrong knockback number: the replay must then fail.
    /// This is how the comparison itself is tested.
    pub wrong_knockback: bool,
    pub skip_client: bool,
    pub skip_server: bool,
}

pub struct Outcome {
    pub client: Report,
    pub server: Report,
}

impl Outcome {
    pub fn identical(&self) -> bool {
        self.client.bad_ticks == 0 && self.server.bad_ticks == 0
    }
    pub fn describe(&self) -> String {
        self.client.describe("client") + &self.server.describe("server")
    }
}

/// Replay the text of one recording.
pub fn replay(text: &str, o: &Options) -> Result<Outcome, String> {
    let lines: Vec<Line> = text.lines().map(Line::parse).collect();
    let head = lines.iter().find(|l| l.tag == "HEADER").ok_or("no HEADER line")?;
    if let Some(f) = lines.iter().find(|l| l.tag == "FAIL") {
        return Err(format!("the recording failed: {}", f.words.join(" ")));
    }
    if !lines.iter().any(|l| l.tag == "END") {
        return Err("the recording is cut short".into());
    }
    let speed = head.i("speed") as i8;
    let sens = head.f("sensitivity");
    let blocks: Vec<[i32; 3]> = lines
        .iter()
        .filter(|l| l.tag == "BLOCK")
        .map(|l| [l.words[0].parse().unwrap(), l.words[1].parse().unwrap(), l.words[2].parse().unwrap()])
        .collect();
    let world = World::with_blocks(4.0, &blocks);
    // Recordings made before blocking was modelled say nothing about the
    // sword being sent again, so the simulator must not send it either.
    let fmt = if head.has("fmt") { head.i("fmt") } else { 1 };
    let wear = if fmt >= 2 { head.i("wear") as f32 } else { 0.0 };
    let client = if o.skip_client { Report::new(false) } else { replay_client(&lines, &world, speed, sens, o, fmt) };
    let server = if o.skip_server { Report::new(false) } else { replay_server(&lines, &world, speed, o, wear, fmt, head) };
    Ok(Outcome { client, server })
}

/// Read a recording from a file, plain or compressed with gzip.
pub fn read_trace(path: &str) -> Result<String, String> {
    if path.ends_with(".gz") {
        let out = std::process::Command::new("gzip").args(["-dc", path]).output().map_err(|e| format!("gzip: {e}"))?;
        if !out.status.success() {
            return Err(format!("gzip could not read {path}"));
        }
        String::from_utf8(out.stdout).map_err(|e| e.to_string())
    } else {
        std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
    }
}
