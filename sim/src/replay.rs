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
use crate::client::{Client, ClientCfg, Input};
use crate::mth::Rng;
use crate::net::{C2S, S2C};
use crate::server::{chunk_of, KbProfile, Server, ServerCfg, ServerPlayer};
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

const SPRINT_UUID: &str = "662a6b8d-da3e-4c1c-8813-96ea6097278d";
const SPEED_UUID: &str = "91aeaa56-376b-4498-935b-2f7f68070635";

/// Turn a recorded server-to-client packet into the simulator's packet.
/// `None` means the packet carries nothing the simulator models.
fn s2c(l: &Line, at: usize, own_id: i64) -> Option<S2C> {
    let class = l.words[at].as_str();
    let id = |k: &str| l.i(k) == own_id;
    if class == "S1CPacketEntityMetadata" {
        let list = l.s("field_149378_b").trim_matches(|c| c == '[' || c == ']');
        for e in list.split(';').filter(|e| !e.is_empty()) {
            let p: Vec<&str> = e.split(':').collect();
            if p[0] == "0" {
                let flags: i64 = p[2].parse().unwrap();
                return Some(S2C::Meta { own: id("entityId"), sprinting: flags & 8 != 0 });
            }
        }
        return None;
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
        return if l.i("logicOpcode") == 2 { Some(S2C::Hurt { own: id("entityId") }) } else { None };
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
        "C02PacketUseEntity" => if l.s("action") == "ATTACK" { Some(C2S::Attack) } else { None },
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

fn replay_client(lines: &[Line], world: &World, speed: i8, sens: f32, o: &Options) -> Report {
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
    let mut i = lines.iter().position(|l| l.tag == "CINIT").unwrap() + 1;
    while i < lines.len() {
        let l = &lines[i];
        i += 1;
        match l.tag.as_str() {
            "CP" => {
                if let Some(p) = s2c(l, 1, own_id) {
                    rep.count("packets received", 1);
                    if let S2C::Velocity { own: true, .. } = p {
                        rep.count("knockbacks taken", 1);
                    }
                    if let S2C::PosLook { .. } = p {
                        rep.count("teleports back", 1);
                    }
                    c.handle(p, &mut out);
                }
            }
            // Sent from inside a packet handler, before the tick.
            "CS" => {
                if let Some(p) = c2s(l, 1) {
                    rep.count("packets sent", 1);
                    sent.push(p);
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
                    attack: n(5) != 0,
                    dx: n(6),
                    dy: -n(7),
                    extra_clicks: 0,
                };
                let t = c.tick(world, &inp, &cfg, &mut out);
                rep.count("clicks", inp.attack as usize);
                rep.count("ticks on target", t.pick.entity as usize);
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
                            sent.push(p);
                        }
                    } else if m.tag == "CT" {
                        state = Some(m);
                    } else if m.tag == "CP" || m.tag == "CI" || m.tag == "END" {
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
                let mut ok = true;
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
                ok &= fi("pick", t.pick.entity as i64, st.i("pick"), &mut rep);
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

/// Is this packet about the player that receives it?
fn about_self(p: &S2C) -> bool {
    match p {
        S2C::Velocity { own, .. } | S2C::Hurt { own } | S2C::Meta { own, .. } | S2C::Props { own, .. } => *own,
        S2C::PosLook { .. } => true,
        _ => false,
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
    p.meta_dirty = false;
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
fn replay_server(lines: &[Line], world: &World, speed: i8, o: &Options) -> Report {
    let (resync, verbose) = (o.resync, o.verbose);
    let mut rep = Report::new(verbose);
    let inits: Vec<&Line> = lines.iter().filter(|l| l.tag == "SINIT").collect();
    if inits.is_empty() {
        return rep;
    }
    let n = inits.len();
    let ids: Vec<i64> = inits.iter().map(|l| l.i("id")).collect();
    let mk = |l: Option<&&Line>| match l {
        Some(l) => {
            let mut p = ServerPlayer::new([l.d("x"), l.d("y"), l.d("z")], l.f("yaw"), l.f("pitch"), speed);
            load_server(&mut p, l);
            p
        }
        // Nobody else in the world: park the second player far away.
        None => ServerPlayer::new([1000.5, world.floor_y, 1000.5], 0.0, 0.0, speed),
    };
    let cfg = ServerCfg { infinite_health: true, ..ServerCfg::default() };
    let mut kb = KbProfile::VANILLA;
    if o.wrong_knockback {
        // Proves that the comparison notices a small error.
        kb.horizontal = 0.41;
    }
    let mut sv = Server::new(mk(inits.get(0)), mk(inits.get(1)), kb, cfg);
    let mut rng = Rng::new(1);
    let mut out: Vec<(usize, S2C)> = Vec::new();
    let mut want: Vec<(usize, S2C)> = Vec::new();
    let mut i = lines.iter().rposition(|l| l.tag == "SINIT").unwrap() + 1;
    let mut states: Vec<&Line> = Vec::new();
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
            Some(l) => !states.is_empty() && (l.tag == "SP" || l.tag == "SS" || (l.tag == "ST" && l.words[0] != tick)),
        };
        if flush {
            sv.tick(&mut out);
            rep.count("packets out", want.len());
            rep.count("teleports back", want.iter().filter(|w| matches!(w.1, S2C::PosLook { .. })).count());
            rep.count("hits", sv.hits.len());
            rep.count("sprint hits", sv.hits.iter().filter(|h| h.sprint).count());
            rep.count("critical hits", sv.hits.iter().filter(|h| h.crit).count());
            sv.hits.clear();
            if verbose {
                println!("server tick {tick}");
            }
            let mut ok = true;
            for dest in 0..n {
                for own in [false, true] {
                    if n == 1 && !own {
                        continue; // nobody else is there to be told about
                    }
                    let pick = |v: &Vec<(usize, S2C)>| -> Vec<S2C> {
                        v.iter().filter(|(d, p)| *d == dest && about_self(p) == own).map(|(_, p)| *p).collect()
                    };
                    let (got, exp) = (pick(&out), pick(&want));
                    let name = match (dest, own) {
                        (0, true) => "sent to 0 own",
                        (0, false) => "sent to 0 other",
                        (_, true) => "sent to 1 own",
                        (_, false) => "sent to 1 other",
                    };
                    ok &= rep.note(name, got == exp, || format!("tick {tick}: simulator {got:?}, game {exp:?}"));
                }
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
                    sv.handle(who, p, world, &mut rng, &mut out);
                    handled_any = true;
                }
            }
            "SS" => {
                if l.words[0] == tick && rep.ticks > 0 && last_flushed == tick {
                    continue;
                }
                tick = l.words[0].clone();
                let who = l.words[1].parse::<usize>().unwrap();
                if let Some(p) = s2c(l, 2, ids[who]) {
                    want.push((who, p));
                }
            }
            "ST" => {
                tick = l.words[0].clone();
                states.push(l);
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
    let client = if o.skip_client { Report::new(false) } else { replay_client(&lines, &world, speed, sens, o) };
    let server = if o.skip_server { Report::new(false) } else { replay_server(&lines, &world, speed, o) };
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
