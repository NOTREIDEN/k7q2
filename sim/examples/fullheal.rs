//! Brute-force search for throws of one's own splash potion of Instant
//! Health II that give the full eight points every time.
//!
//!   fullheal kb                      print the knockback a real hit gives
//!   fullheal validate N              fast model against the full Match
//!   fullheal search STATE [YAWS]     enumerate plans, robust check, print
//!   fullheal verify STATE PLAN N     Monte Carlo of one plan in the full Match
//!
//! The fast model: the player's own client (Client::tick, bit exact) gives
//! the C03 packets of each tick; the server's potion is EntityThrowable as
//! in server.rs, and the server has the player where the packets of client
//! tick (k + L0 - e_k) put it on the potion's update k (k = 1 is the update
//! of the server tick that handles the click), L0 = 1 if the click packet
//! itself came a server tick late and e_k = 1 if that tick's packet did.
//! With no jitter L0 = e_k = 0; with jitter every pattern is possible.
use pvpsim::aabb::{dist_sq, Aabb, V3};
use pvpsim::client::{Client, ClientCfg, Input};
use pvpsim::mth::Rng;
use pvpsim::net::{potion_mask, Kind, C2S, S2C, SLOT_SWORD};
use pvpsim::server::{splash_heal, throw_heading, KbProfile, ServerCfg, THROW_SPREAD};
use pvpsim::sim::{Match, NetCfg, Spawn, TICK_US};
use pvpsim::world::{RayHit, World};
use std::io::Write;

pub const FLOOR: f64 = 4.0;
thread_local! {
    static PKTS: std::cell::RefCell<Vec<(u32, String)>> = std::cell::RefCell::new(Vec::new());
}
const POT_SLOT: u8 = 2;

// ------------------------------------------------------------------ keys

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Keys {
    pub fw: i8,
    pub st: i8,
    pub sprint: bool,
    pub jump: bool,
}

impl Keys {
    fn code(&self) -> String {
        let mut s = String::new();
        match self.fw {
            1 => s.push('W'),
            -1 => s.push('S'),
            _ => {}
        }
        match self.st {
            1 => s.push('A'),
            -1 => s.push('D'),
            _ => {}
        }
        if self.sprint {
            s.push('r');
        }
        if self.jump {
            s.push('j');
        }
        if s.is_empty() {
            s.push('-');
        }
        s
    }
    fn parse(s: &str) -> Keys {
        let mut k = Keys::default();
        for ch in s.chars() {
            match ch {
                'W' => k.fw = 1,
                'S' => k.fw = -1,
                'A' => k.st = 1,
                'D' => k.st = -1,
                'r' => k.sprint = true,
                'j' => k.jump = true,
                _ => {}
            }
        }
        k
    }
}

/// The movement keys a plan chooses from on a tick: none, the eight
/// directions, and the three with W sprinting.
pub fn key_set() -> Vec<Keys> {
    let mut v = Vec::new();
    for fw in [0i8, 1, -1] {
        for st in [0i8, 1, -1] {
            v.push(Keys { fw, st, sprint: false, jump: false });
            if fw == 1 {
                v.push(Keys { fw, st, sprint: true, jump: false });
            }
        }
    }
    v
}

/// A plan from the tick that turns the view (tick 0): the turn and the
/// pitch it sets (mouse counts), the keys of each tick (tick 1 is the
/// click), and the tick on which the view turns back (0: never).
#[derive(Clone, Debug)]
pub struct Plan {
    pub turn: i32,
    pub dy: i32,
    pub keys: Vec<Keys>,
    pub back: usize,
}

impl Plan {
    pub fn keys_at(&self, t: usize) -> Keys {
        if t < self.keys.len() {
            self.keys[t]
        } else {
            let mut k = *self.keys.last().unwrap();
            k.jump = false;
            k
        }
    }
    pub fn input(&self, t: usize) -> Input {
        let k = self.keys_at(t);
        let mut inp = Input { forward: k.fw, strafe: k.st, jump: k.jump, sprint_key: k.sprint, ..Input::default() };
        if t == 0 {
            inp.dx = self.turn;
            inp.dy = self.dy;
            inp.slot_key = POT_SLOT + 1;
        } else if t == 1 {
            inp.use_item = true;
        } else if t == 2 {
            inp.slot_key = SLOT_SWORD + 1;
        }
        if self.back > 0 && t == self.back {
            inp.dx = -self.turn;
            inp.dy = -self.dy;
        }
        inp
    }
    /// The same plan with the view turned back on tick 2, its keys from
    /// then on turned with it (a turn of a multiple of 45 degrees); a key
    /// that sprints and is no longer forward loses the sprint. None if
    /// the turn is not such a multiple.
    pub fn turned_back(&self) -> Option<Plan> {
        if self.turn % 300 != 0 {
            return None;
        }
        let steps = (self.turn / 300).rem_euclid(8);
        let dirs: [(i8, i8); 8] = [(1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0), (-1, 1), (0, 1), (1, 1)];
        let mut p = self.clone();
        p.back = 2;
        for k in p.keys.iter_mut().skip(2) {
            if k.fw == 0 && k.st == 0 {
                continue;
            }
            let d = dirs.iter().position(|&x| x == (k.fw, k.st)).unwrap() as i32;
            let (fw, st) = dirs[((d + steps) % 8) as usize];
            k.fw = fw;
            k.st = st;
            if fw != 1 {
                k.sprint = false;
            }
        }
        Some(p)
    }
    pub fn text(&self) -> String {
        let ks: Vec<String> = self.keys.iter().map(|k| k.code()).collect();
        format!("{}:{}:{}:{}", self.turn, self.dy, self.back, ks.join(","))
    }
    pub fn parse(s: &str) -> Plan {
        let p: Vec<&str> = s.split(':').collect();
        Plan {
            turn: p[0].parse().unwrap(),
            dy: p[1].parse().unwrap(),
            back: p[2].parse().unwrap(),
            keys: p[3].split(',').map(Keys::parse).collect(),
        }
    }
}

// ------------------------------------------------------------ the duel

/// A duel with player 0 at the origin looking along +x and player 1 far
/// off, both with potions, the links all but instant (a plan sets them).
pub fn base_match(speed_amp: i8, seed: u64) -> Match {
    let world = World::arena(FLOOR, 300.0, false);
    let a = [0.5, FLOOR, 0.5];
    let b = [0.5, FLOOR, 60.5];
    let spawns = [Spawn { pos: a, yaw: -90.0, pitch: 0.0 }, Spawn { pos: b, yaw: 180.0, pitch: 0.0 }];
    let mut scfg = ServerCfg::default();
    scfg.hunger = false;
    let net = NetCfg { up_us: [1, 1], down_us: [1, 1], jitter_us: [0, 0] };
    let mut m = Match::new(world, spawns, speed_amp, KbProfile::VANILLA, scfg, ClientCfg::with_sensitivity(0.5), net, [0, 10_000, 30_000], seed);
    for i in 0..2 {
        m.s.p[i].potions = potion_mask(7);
        m.c[i].lp.potions = potion_mask(7);
        m.s.p[i].health = 4.0;
        m.c[i].lp.health = 4.0;
    }
    m
}

/// The server ticks of `m` with player 0's feet as the server had them
/// for the thrown updates of that tick (debugging).
pub fn step_dbg(m: &mut Match, inp: &Input, log: &mut Vec<(u32, V3, usize)>) {
    let idle = Input::default();
    loop {
        let id = if m.next_client[1] < m.next_client[0] { 1 } else { 0 };
        if m.next_server <= m.next_client[id] {
            let t = m.next_server;
            let mut out = Vec::new();
            loop {
                let a0 = m.c2s[0].peek_time().filter(|&a| a <= t);
                let a1 = m.c2s[1].peek_time().filter(|&a| a <= t);
                let from = match (a0, a1) {
                    (None, None) => break,
                    (Some(_), None) => 0,
                    (None, Some(_)) => 1,
                    (Some(x), Some(y)) => if y < x { 1 } else { 0 },
                };
                let p = m.c2s[from].pop_ready(t).unwrap();
                m.s.handle(from, p, &m.world, &mut m.rng, &mut out);
            }
            log.push((m.server_ticks, m.s.p[0].l.body.pos, m.s.thrown.len()));
            m.s.tick(&m.world, &mut m.rng, &mut out);
            for (dest, p) in out {
                let d = m.net.down_us[dest];
                m.s2c[dest].push(t + d, p);
            }
            m.next_server = t + TICK_US;
            m.server_ticks += 1;
            continue;
        }
        let tt = m.next_client[id];
        let mut o2 = Vec::new();
        while let Some(at) = m.s2c[id].peek_time().filter(|&a| a <= tt) {
            let p = m.s2c[id].pop_ready(tt).unwrap();
            if id == 0 {
                PKTS.with(|v| v.borrow_mut().push((m.client_ticks[0], format!("{:?}", p).chars().take(60).collect())));
            }
            o2.clear();
            m.c[id].handle(&m.world, p, &mut o2);
            for q in o2.iter() {
                let d = m.net.up_us[id];
                m.c2s[id].push(at + d, *q);
            }
        }
        if id == 0 {
            m.client_tick(0, inp);
            return;
        } else {
            m.client_tick(1, &idle);
        }
    }
}

/// One tick of client 0 with `inp`, client 1 idle, the server in between.
pub fn step(m: &mut Match, inp: &Input) {
    let idle = Input::default();
    loop {
        let id = m.advance();
        if id == 0 {
            m.client_tick(0, inp);
            return;
        } else {
            m.client_tick(1, &idle);
        }
    }
}

/// The starting states: the duel just before the tick that turns the
/// view, and a note of the player's motion.
pub fn start_state(name: &str) -> Match {
    let mut parts = name.split('_');
    let what = parts.next().unwrap();
    let speed = name.contains("spd");
    let mut m = base_match(if speed { 1 } else { -1 }, 7);
    let run = |m: &mut Match, k: Keys, n: usize| {
        for _ in 0..n {
            step(m, &Input { forward: k.fw, strafe: k.st, jump: k.jump, sprint_key: k.sprint, ..Input::default() });
        }
    };
    match what {
        "still" => run(&mut m, Keys::default(), 10),
        "walk" => run(&mut m, Keys { fw: 1, ..Keys::default() }, 30),
        "sprint" => run(&mut m, Keys { fw: 1, sprint: true, ..Keys::default() }, 30),
        "sprintjump" => {
            // Sprint-jumping: jump held, the tick after landing.
            run(&mut m, Keys { fw: 1, sprint: true, ..Keys::default() }, 20);
            for _ in 0..30 {
                run(&mut m, Keys { fw: 1, sprint: true, jump: true, st: 0 }, 1);
            }
        }
        "kb" => {
            // kb_<h>_<v>_<ticks after>: a hit from the front (the player
            // looks along +x and flies back along -x) of this much
            // knockback, taken standing, then that many ticks with no key.
            let h: f64 = parts.next().unwrap().parse().unwrap();
            let v: f64 = parts.next().unwrap().parse().unwrap();
            let after: usize = parts.next().unwrap().parse().unwrap();
            run(&mut m, Keys::default(), 10);
            let t = m.next_client[0] - 1;
            m.s2c[0].push(t, S2C::Velocity { own: true, v: [(-h * 8000.0) as i32, (v * 8000.0) as i32, 0] });
            run(&mut m, Keys::default(), after);
        }
        "kbrun" => {
            // Sprinting along +x into a hit from the front (pushed back
            // along -x), W still held for the ticks after.
            let h: f64 = parts.next().unwrap().parse().unwrap();
            let v: f64 = parts.next().unwrap().parse().unwrap();
            let after: usize = parts.next().unwrap().parse().unwrap();
            run(&mut m, Keys { fw: 1, sprint: true, ..Keys::default() }, 30);
            let t = m.next_client[0] - 1;
            m.s2c[0].push(t, S2C::Velocity { own: true, v: [(-h * 8000.0) as i32, (v * 8000.0) as i32, 0] });
            run(&mut m, Keys { fw: 1, sprint: true, ..Keys::default() }, after);
        }
        _ => panic!("unknown state {}", name),
    }
    // Up to the tick of client 0, with what has reached it handed over.
    loop {
        let id = m.advance();
        if id == 0 {
            break;
        }
        m.client_tick(1, &Input::default());
    }
    m
}

// ------------------------------------------------------------ fast model

/// What the server has of player 0 after the packets of a client tick.
#[derive(Clone, Copy, Debug)]
pub struct Rep {
    pub feet: V3,
    pub on_ground: bool,
    pub moved: V3,
    pub yaw: f32,
    pub pitch: f32,
    /// The client's own feet (for the distance moved).
    pub own: V3,
    pub threw: bool,
}

/// The packets of `ticks` ticks of the plan, from a copy of the client.
pub fn client_reps(m: &Match, plan: &Plan, ticks: usize) -> Vec<Rep> {
    let mut c: Client = m.c[0].clone();
    let world = &m.world;
    let cfg = m.ccfg;
    let sp = &m.s.p[0];
    let mut feet = sp.l.body.pos;
    let (mut yaw, mut pitch) = (sp.l.yaw, sp.l.pitch);
    let mut out: Vec<C2S> = Vec::with_capacity(8);
    let mut reps = Vec::with_capacity(ticks);
    for t in 0..ticks {
        out.clear();
        let r = c.tick(world, &plan.input(t), &cfg, &mut out);
        let mut moved = [0.0; 3];
        let mut og = sp.l.body.on_ground;
        for p in out.iter() {
            if let C2S::Player { pos, rot, on_ground } = p {
                if let Some(d) = pos {
                    moved = [d[0] - feet[0], d[1] - feet[1], d[2] - feet[2]];
                    feet = *d;
                }
                if let Some((y, pt)) = rot {
                    yaw = *y % 360.0f32;
                    pitch = *pt % 360.0f32;
                }
                og = *on_ground;
            }
        }
        reps.push(Rep { feet, on_ground: og, moved, yaw, pitch, own: c.lp.l.body.pos, threw: r.threw_potion });
    }
    reps
}

#[derive(Clone, Copy, Debug)]
pub struct Up {
    pub pos: V3,
    pub end: V3,
    pub block: bool,
}

/// The potion's first motion: the throw from what the server has after
/// tick 0, `z` the three gaussians, with or without the carried movement.
pub fn throw_motion(r0: &Rep, z: [f64; 3], carry: bool) -> (V3, V3) {
    let (pos, dir) = throw_heading(Kind::Potion, r0.feet, r0.yaw, r0.pitch);
    let spread = THROW_SPREAD * 1.0f32 as f64;
    let speed = Kind::Potion.speed() as f64;
    let mut m = dir;
    for a in 0..3 {
        m[a] += z[a] * spread;
        m[a] *= speed;
    }
    if carry {
        m[0] += r0.moved[0];
        m[2] += r0.moved[2];
        if !r0.on_ground {
            m[1] += r0.moved[1];
        }
    }
    (pos, m)
}

/// The server's updates of a potion until it breaks on a block (or 80).
pub fn potion_path(world: &World, mut pos: V3, mut m: V3, out: &mut Vec<Up>) {
    out.clear();
    let (f2, g) = (0.99f32 as f64, Kind::Potion.gravity() as f64);
    for _ in 0..80 {
        if pos[1] < -64.0 {
            return;
        }
        let mut end = [pos[0] + m[0], pos[1] + m[1], pos[2] + m[2]];
        let mut block = false;
        if let RayHit::Block(h) = world.ray_trace_blocks(pos, end, false) {
            end = h;
            block = true;
        }
        out.push(Up { pos, end, block });
        if block {
            return;
        }
        for a in 0..3 {
            pos[a] += m[a];
        }
        for a in 0..3 {
            m[a] *= f2;
        }
        m[1] -= g;
    }
}

#[inline]
fn direct(u: &Up, f: V3) -> bool {
    let f3 = 0.3f32 as f64;
    Aabb::player(f).expand(f3, f3, f3).calculate_intercept(u.pos, u.end).is_some()
}

#[inline]
pub fn splash_of(at: V3, f: V3) -> f32 {
    let w = (0.25f32 / 2.0f32) as f64;
    let bb = Aabb::new([at[0] - w, at[1], at[2] - w], [at[0] + w, at[1] + 0.25f32 as f64, at[2] + w]);
    let area = bb.expand(4.0, 2.0, 4.0);
    if !Aabb::player(f).intersects(&area) || !(dist_sq(at, f) < 16.0) {
        return 0.0;
    }
    splash_heal(at, f, false)
}

/// Worst heal (and the update it comes on) over the packet patterns.
/// `feet(t)` is what the server has after client tick t.
pub fn robust(path: &[Up], reps: &[Rep], jitter: bool) -> (f32, i32) {
    let mut m = 0;
    robust_m(path, reps, jitter, &mut m)
}

/// `robust`, noting the last tick of `reps` it looked at in `maxi`.
pub fn robust_m(path: &[Up], reps: &[Rep], jitter: bool, maxi: &mut usize) -> (f32, i32) {
    let n = reps.len() as isize;
    let mut feet = |t: isize| {
        let i = t.clamp(0, n - 1) as usize;
        if i > *maxi {
            *maxi = i;
        }
        reps[i].feet
    };
    let mut worst = (8.0f32, 0i32);
    for l0 in 0..=(jitter as isize) {
        let mut res = (0.0f32, path.len() as i32);
        for (i, u) in path.iter().enumerate() {
            let k = i as isize + 1;
            let choices: &[isize] = if jitter { &[0, 1] } else { &[0] };
            let mut all_hit = true;
            let mut low = 8.0f32;
            for &e in choices {
                let f = feet(k + l0 - e);
                let h = if k >= 5 && direct(u, f) {
                    8.0
                } else {
                    all_hit = false;
                    if u.block {
                        splash_of(u.pos, f)
                    } else {
                        0.0
                    }
                };
                low = low.min(h);
            }
            if all_hit {
                res = (8.0, k as i32);
                break;
            }
            if u.block {
                res = (low, k as i32);
                break;
            }
        }
        if res.0 < worst.0 || (res.0 == worst.0 && res.1 > worst.1) {
            worst = res;
        }
    }
    worst
}

/// The gaussians a robust check tries: the middle, 2.5 and 5 along each
/// axis, and radius 5 on the diagonals and the edges.
pub fn z_set() -> Vec<[f64; 3]> {
    let mut v = vec![[0.0; 3]];
    for a in 0..3 {
        for s in [-5.0, -2.5, 2.5, 5.0] {
            let mut z = [0.0; 3];
            z[a] = s;
            v.push(z);
        }
    }
    let d = 5.0 / 3f64.sqrt();
    for sx in [-d, d] {
        for sy in [-d, d] {
            for sz in [-d, d] {
                v.push([sx, sy, sz]);
            }
        }
    }
    let e = 5.0 / 2f64.sqrt();
    for (a, b) in [(0, 1), (0, 2), (1, 2)] {
        for sa in [-e, e] {
            for sb in [-e, e] {
                let mut z = [0.0; 3];
                z[a] = sa;
                z[b] = sb;
                v.push(z);
            }
        }
    }
    v
}

// ------------------------------------------------------------ full Match

#[derive(Clone, Copy, Debug, Default)]
pub struct Trial {
    pub heal: f32,
    pub ticks: i32,
    pub direct: bool,
    pub other: f32,
    pub thrown: bool,
}

/// The network of a trial for player 0: delay to the server (its phase
/// against the server's clock is in it), delay back, jitter of each packet.
#[derive(Clone, Copy, Debug)]
pub struct Net {
    pub up: i64,
    pub down: i64,
    pub jit: i64,
}

/// One plan in a copy of the duel, the randomness from `seed`.
pub fn trial(start: &Match, plan: &Plan, net: Net, carry: bool, seed: u64, ticks: usize) -> Trial {
    trial_hit(start, plan, net, carry, seed, ticks, None)
}

/// `trial`, with a hit's velocity reaching the client before its tick
/// `hit.0` (the server is not told of a hit: only how it moves counts).
pub fn trial_hit(start: &Match, plan: &Plan, net: Net, carry: bool, seed: u64, ticks: usize, hit: Option<(usize, V3)>) -> Trial {
    let mut m = start.clone();
    m.rng = Rng::new(seed);
    m.net.up_us[0] = net.up;
    m.net.down_us[0] = net.down;
    m.net.jitter_us[0] = net.jit;
    m.s.cfg.throw_carry = carry;
    let n0 = m.s.splashes.len();
    let throws0 = m.s.potion_throws[0];
    let mut t = 0;
    while m.s.splashes.len() == n0 && t < ticks + 40 {
        if let Some((ht, v)) = hit {
            if ht == t {
                let at = m.next_client[0] - 1;
                m.s2c[0].push(at, S2C::Velocity { own: true, v: [(v[0] * 8000.0) as i32, (v[1] * 8000.0) as i32, (v[2] * 8000.0) as i32] });
            }
        }
        step(&mut m, &plan.input(t));
        t += 1;
    }
    let thrown = m.s.potion_throws[0] > throws0;
    match m.s.splashes.get(n0) {
        Some(s) => Trial { heal: s.offered[0], ticks: s.ticks, direct: s.on_player == Some(0), other: s.offered[1], thrown },
        None => Trial { thrown, ..Trial::default() },
    }
}

/// What the server is told of player 0 after each tick of the plan, from
/// the full duel with no jitter and the answers coming back `down_us`
/// later: the server's own word on sprinting, which the client takes
/// when it comes, can change how it moves.
pub fn full_reps(start: &Match, plan: &Plan, ticks: usize, up_us: i64, down_us: i64) -> Vec<Rep> {
    let mut m = start.clone();
    m.net.up_us[0] = up_us.max(1);
    m.net.down_us[0] = down_us.max(1);
    m.net.jitter_us[0] = 0;
    let mut reps = Vec::with_capacity(ticks);
    let mut threw0 = m.s.potion_throws[0];
    for t in 0..ticks {
        let before = m.c[0].lp.last_reported;
        step(&mut m, &plan.input(t));
        let lp = &m.c[0].lp;
        let sent = lp.position_update_ticks == 0;
        let moved = if sent { [lp.last_reported[0] - before[0], lp.last_reported[1] - before[1], lp.last_reported[2] - before[2]] } else { [0.0; 3] };
        let threw = m.s.potion_throws[0] > threw0;
        threw0 = m.s.potion_throws[0];
        reps.push(Rep {
            feet: lp.last_reported,
            on_ground: lp.l.body.on_ground,
            moved,
            yaw: lp.last_yaw % 360.0f32,
            pitch: lp.last_pitch % 360.0f32,
            own: lp.l.body.pos,
            threw,
        });
    }
    reps
}

// ------------------------------------------------------------ commands

fn cmd_kb() {
    // A real hit: player 1 next to player 0, sprinting at it, clicking.
    for (sprint, plugin) in [(false, false), (true, false), (false, true), (true, true)] {
        let world = World::arena(FLOOR, 300.0, false);
        let spawns = [Spawn { pos: [0.5, FLOOR, 0.5], yaw: 90.0, pitch: 0.0 }, Spawn { pos: [-2.0, FLOOR, 0.5], yaw: -90.0, pitch: 0.0 }];
        let net = NetCfg { up_us: [1, 1], down_us: [1, 1], jitter_us: [0, 0] };
        let mut m = Match::new(world, spawns, -1, KbProfile::VANILLA, ServerCfg::default(), ClientCfg::with_sensitivity(0.5), net, [0, 10_000, 30_000], 3);
        if plugin {
            m.s.plugin = Some(pvpsim::server::KbPlugin::DEFAULT);
        }
        let mut got = None;
        for t in 0..40 {
            let id = m.advance();
            let inp = if id == 1 {
                Input { forward: if t < 4 { 1 } else { 0 }, sprint_key: sprint, attack: t >= 2 && got.is_none(), ..Input::default() }
            } else {
                Input::default()
            };
            m.client_tick(id, &inp);
            if got.is_none() && m.c[0].lp.ticks_since_kb < 3 {
                got = Some(m.c[0].lp.last_kb);
            }
        }
        println!("sprint {} plugin {} kb {:?}", sprint, plugin, got);
    }
}

fn mix(a: u64) -> u64 {
    let mut z = a.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// The fast model against the full Match on random plans: with no
/// jitter, the potion the server spawned and the heal it gave.
fn cmd_validate(n: usize) {
    let states = ["still", "walk", "sprint", "sprint_spd", "kb_0.9_0.46_0", "kb_0.4_0.36_2", "sprintjump", "kbrun_0.9_0.46_1", "kb_0.2_0.25_0"];
    let ks = key_set();
    let mut bad = 0;
    let mut same = 0;
    let mut clone_same = 0;
    let mut rng = Rng::new(11);
    let mut path = Vec::new();
    for i in 0..n {
        let st = states[i % states.len()];
        let start = start_state(st);
        let mut keys = Vec::new();
        for _ in 0..30 {
            let mut k = ks[(rng.next_u64() % ks.len() as u64) as usize];
            k.jump = rng.chance(0.15);
            keys.push(k);
        }
        let turn = [0, 1200, 600, -600, 300][(rng.next_u64() % 5) as usize];
        let target = -90.0 + rng.f64() * 180.0;
        let dy = ((target - start.c[0].lp.l.pitch as f64) / 0.15).round() as i32;
        let back = if rng.chance(0.5) { 2 + (rng.next_u64() % 3) as usize } else { 0 };
        let plan = Plan { turn, dy, keys, back };
        let carry = rng.chance(0.5);
        let seed = rng.next_u64();
        // The fast model, with gaussians of its own; the full Match is
        // handed the same first motion (its replay queue), carry included.
        let down = 1 + (rng.next_u64() % 150_000) as i64;
        let up = 1 + (rng.next_u64() % 49_000) as i64;
        let reps = full_reps(&start, &plan, 60, up, down);
        let reps_clone = client_reps(&start, &plan, 60);
        let same_client = reps.iter().zip(reps_clone.iter()).all(|(a, b)| a.feet == b.feet);
        if same_client {
            clone_same += 1;
        }
        let mut r2 = Rng::new(seed);
        let z = [r2.gaussian(), r2.gaussian(), r2.gaussian()];
        let (from, m0) = throw_motion(&reps[0], z, carry);
        potion_path(&start.world, from, m0, &mut path);
        let (h, k) = robust(&path, &reps, false);
        let mut m = start.clone();
        m.rng = Rng::new(seed);
        m.net.up_us[0] = up;
        m.net.down_us[0] = down;
        m.s.cfg.throw_carry = carry;
        m.s.throw_motions.push_back(m0);
        let n0 = m.s.splashes.len();
        let mut log = Vec::new();
        let mut owns = Vec::new();
        PKTS.with(|v| v.borrow_mut().clear());
        let ct0 = m.client_ticks[0];
        for t in 0..60 {
            step_dbg(&mut m, &plan.input(t), &mut log);
            owns.push(m.c[0].lp.l.body.pos);
            if m.s.splashes.len() > n0 {
                break;
            }
        }
        let Some(sp) = m.s.splashes.get(n0) else {
            println!("{} no splash", st);
            continue;
        };
        if !(h == sp.offered[0] && k == sp.ticks) && bad < 4 {
            // The server ticks from the spawn on, against reps[k].
            for (t, o) in owns.iter().enumerate() {
                let r = reps[t].own;
                if (o[0] - r[0]).abs() + (o[2] - r[2]).abs() > 1e-12 {
                    println!("   client tick {} full {:?} fast {:?}", t, o, r);
                    PKTS.with(|v| for (ct, s) in v.borrow().iter() { if *ct >= ct0 + t as u32 - 2 && *ct <= ct0 + t as u32 { println!("      pkt at {} {}", *ct - ct0, s); } });
                    break;
                }
            }
            let first = log.iter().position(|e| e.2 > 0).unwrap_or(0);
            for (q, e) in log[first..].iter().enumerate() {
                let kk = q + 1;
                let r = reps.get(kk).map(|r| r.feet).unwrap_or([0.0; 3]);
                let same = (e.1[0] - r[0]).abs() + (e.1[1] - r[1]).abs() + (e.1[2] - r[2]).abs() < 1e-9;
                println!("   upd {} server {:?} fast {:?} {}", kk, e.1, r, if same { "" } else { "DIFF" });
            }
        }
        let ok = h == sp.offered[0] && k == sp.ticks;
        if ok {
            same += 1;
        } else {
            bad += 1;
            if bad < 15 {
                println!("MISMATCH {} plan {} carry {} full ({}, {}, at {:?}) fast ({}, {}) at {:?}", st, plan.text(), carry, sp.offered[0], sp.ticks, sp.pos, h, k, path.last().map(|u| u.pos));
            }
        }
    }
    println!("validate: {} same, {} different; client copy without the server's answers same as the full duel in {}", same, bad, clone_same);
}

/// A plan family: keys `k01` on ticks 0 and 1, then `a` for `na` ticks,
/// then `b`, with a jump on tick `j` (none if j < 0).
fn family(turn: i32, dy: i32, k01: Keys, a: Keys, na: usize, b: Keys, j: isize, len: usize) -> Plan {
    let mut keys = Vec::with_capacity(len);
    for t in 0..len {
        let mut k = if t < 2 {
            k01
        } else if t < 2 + na {
            a
        } else {
            b
        };
        k.jump = j >= 0 && t == j as usize;
        keys.push(k);
    }
    Plan { turn, dy, keys, back: 0 }
}

/// Horizontal path of the client's own feet from the start to the end of
/// client tick `to`.
fn path_len(reps: &[Rep], start: V3, to: usize) -> f64 {
    let mut d = 0.0;
    let mut p = start;
    for r in reps.iter().take(to + 1) {
        d += (r.own[0] - p[0]).hypot(r.own[2] - p[2]);
        p = r.own;
    }
    d
}

/// A hash of what the server is told up to tick `n`.
fn prefix_hash(reps: &[Rep], n: usize) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for r in reps.iter().take(n) {
        for a in 0..3 {
            r.feet[a].to_bits().hash(&mut h);
        }
        r.on_ground.hash(&mut h);
    }
    h.finish()
}

/// Worst heal over the gaussians of `zs` for one throw: [carry][jitter].
fn worst_over(world: &World, r0: &Rep, reps: &[Rep], zs: &[[f64; 3]], path: &mut Vec<Up>) -> [[(f32, i32); 2]; 2] {
    let mut m = 0;
    worst_over_m(world, r0, reps, zs, path, &mut m)
}

fn worst_over_m(world: &World, r0: &Rep, reps: &[Rep], zs: &[[f64; 3]], path: &mut Vec<Up>, maxi: &mut usize) -> [[(f32, i32); 2]; 2] {
    let mut w = [[(8.0f32, 0i32); 2]; 2];
    for z in zs {
        for carry in 0..2 {
            let (from, m0) = throw_motion(r0, *z, carry == 1);
            potion_path(world, from, m0, path);
            for jit in 0..2 {
                let a = robust_m(path, reps, jit == 1, maxi);
                let c = &mut w[carry][jit];
                if a.0 < c.0 || (a.0 == c.0 && a.1 > c.1) {
                    *c = a;
                }
            }
        }
        if w.iter().all(|c| c.iter().all(|x| x.0 < 8.0)) {
            break;
        }
    }
    w
}

fn min_w(a: [[(f32, i32); 2]; 2], b: [[(f32, i32); 2]; 2]) -> [[(f32, i32); 2]; 2] {
    let mut o = a;
    for c in 0..2 {
        for j in 0..2 {
            let (x, y) = (a[c][j], b[c][j]);
            o[c][j] = if y.0 < x.0 || (y.0 == x.0 && y.1 > x.1) { y } else { x };
        }
    }
    o
}

/// The four ways a throw is judged: carry off and on, each with no
/// jitter and with any; and both carries together.
const CRIT: [(&str, [(usize, usize); 2]); 6] = [
    ("all", [(0, 1), (1, 1)]),
    ("nojit", [(0, 0), (1, 0)]),
    ("c0", [(0, 1), (0, 1)]),
    ("c1", [(1, 1), (1, 1)]),
    ("c0nj", [(0, 0), (0, 0)]),
    ("c1nj", [(1, 0), (1, 0)]),
];

fn crit_ok(w: &[[(f32, i32); 2]; 2], c: usize) -> bool {
    CRIT[c].1.iter().all(|&(a, b)| w[a][b].0 >= 8.0)
}

fn crit_ticks(w: &[[(f32, i32); 2]; 2], c: usize) -> i32 {
    CRIT[c].1.iter().map(|&(a, b)| w[a][b].1).max().unwrap()
}

/// Plans whose worst heal is eight, by criterion: for each plan of the
/// family and each window of mouse counts of the pitch in which it holds,
/// a row for the middle of the window.
fn cmd_search(state: &str, yaws: &[i32], out_path: &str, jumps_max: isize, max_ticks: i32) {
    let start = start_state(state);
    let lp0 = start.c[0].lp.clone();
    let own0 = lp0.l.body.pos;
    let ks = key_set();
    let zs = z_set();
    const LEN: usize = 36;
    const COARSE: i32 = 7;
    let rtts: Vec<i64> = (0..=12).map(|k| k * 25_000).collect();
    let mut file = std::io::BufWriter::new(std::fs::File::create(out_path).unwrap());
    writeln!(
        file,
        "state\tcrit\tturn\tdy\tpitch\twin\tplan\tticks\tworst\tw_c0nj\tw_c0j\tw_c1nj\tw_c1j\tk_c0nj\tk_c0j\tk_c1nj\tk_c1j\tB\tmoved\tdev\taway\tjump"
    )
    .unwrap();
    let nas = [0usize, 1, 2, 3, 5, 9];
    let jumps: Vec<isize> = (-1..=jumps_max).collect();
    let mut path = Vec::with_capacity(80);
    let (dy_lo, dy_hi) = (((-90.0 - lp0.l.pitch as f64) / 0.15).ceil() as i32, ((90.0 - lp0.l.pitch as f64) / 0.15).floor() as i32);
    let t0 = std::time::Instant::now();
    let mut n_plans = 0u64;
    let mut n_screen = 0u64;
    let mut n_rows = 0u64;
    // No keys at all from tick 0: where the start state slides anyway.
    let coast = client_reps(&start, &family(0, 0, Keys::default(), Keys::default(), 0, Keys::default(), -1, LEN), LEN);
    let mut seen_rows: std::collections::HashSet<(usize, i32, String)> = std::collections::HashSet::new();
    let mut tm = [0.0f64; 4];
    for &turn in yaws {
        // The pitch of every dy after the turn's tick, as the client has it.
        let pitch_of = |dy: i32| -> f32 {
            let mut c = lp0.clone();
            c.set_angles(turn as f32 * start.ccfg.mouse_f1, -(dy as f32 * start.ccfg.mouse_f1));
            c.l.pitch
        };
        let coarse: Vec<i32> = (dy_lo..=dy_hi).filter(|d| (d - dy_lo) % COARSE == 0).collect();
        let k01s: Vec<Keys> = match std::env::var("K01") {
            Ok(v) => v.split(',').map(Keys::parse).collect(),
            Err(_) => ks.clone(),
        };
        for &k01 in &k01s {
            // What the server has after tick 0, without and with a jump on
            // that tick: where the potion leaves from.
            let r0base: Vec<Rep> = [-1isize, 0]
                .iter()
                .map(|&jj| client_reps(&start, &family(turn, 0, k01, k01, 0, k01, jj, 3), 1)[0])
                .collect();
            let r0_of = |dy: i32, v: usize| -> Rep {
                let mut r = r0base[v];
                r.pitch = pitch_of(dy) % 360.0f32;
                r
            };
            let mut memo: std::collections::HashMap<(i32, usize, u64), [[(f32, i32); 2]; 2]> = std::collections::HashMap::new();
            let mut lens: std::collections::HashMap<i32, std::collections::BTreeSet<usize>> = std::collections::HashMap::new();
            // The ticks of the nominal throw at each dy (both carries), to
            // know how much of a plan's run can matter to it.
            let mut nom: [Vec<[Vec<Up>; 2]>; 2] = [Vec::new(), Vec::new()];
            for v in 0..2 {
                for &dy in &coarse {
                    let r0 = r0_of(dy, v);
                    let mut pp: [Vec<Up>; 2] = [Vec::new(), Vec::new()];
                    for carry in 0..2 {
                        let (from, m0) = throw_motion(&r0, [0.0; 3], carry == 1);
                        potion_path(&start.world, from, m0, &mut path);
                        pp[carry] = path.clone();
                    }
                    nom[v].push(pp);
                }
            }
            for &a in &ks {
                for &na in &nas {
                    if na == 0 && a != ks[0] {
                        continue;
                    }
                    for &b in &ks {
                        for &j in &jumps {
                            n_plans += 1;
                            let plan = family(turn, 0, k01, a, na, b, j, LEN);
                            let tt = std::time::Instant::now();
                            let reps = client_reps(&start, &plan, LEN);
                            tm[0] += tt.elapsed().as_secs_f64();
                            let tt = std::time::Instant::now();
                            let v = (j == 0) as usize;
                            debug_assert!(reps[0].feet == r0base[v].feet);
                            // Screening: the nominal throw, no jitter, either carry.
                            let mut passing: Vec<usize> = Vec::new();
                            for ci in 0..coarse.len() {
                                let ok = (0..2).any(|c| {
                                    let r = robust(&nom[v][ci][c], &reps, false);
                                    r.0 >= 8.0 && r.1 <= max_ticks
                                });
                                if ok {
                                    passing.push(ci);
                                }
                            }
                            tm[1] += tt.elapsed().as_secs_f64();
                            if passing.is_empty() {
                                continue;
                            }
                            n_screen += 1;
                            let tt = std::time::Instant::now();
                            // The robust result at a dy, from the plans before
                            // when one agrees on every tick it looked at.
                            let mut cache: std::collections::HashMap<i32, [[(f32, i32); 2]; 2]> = std::collections::HashMap::new();
                            let mut get = |dy: i32, path: &mut Vec<Up>| -> [[(f32, i32); 2]; 2] {
                                if let Some(w) = cache.get(&dy) {
                                    return *w;
                                }
                                if let Some(ls) = lens.get(&dy) {
                                    for &l in ls.iter() {
                                        if let Some(w) = memo.get(&(dy, l, prefix_hash(&reps, l + 1))) {
                                            cache.insert(dy, *w);
                                            return *w;
                                        }
                                    }
                                }
                                let mut mi = 0usize;
                                let w = worst_over_m(&start.world, &r0_of(dy, v), &reps, &zs, path, &mut mi);
                                memo.insert((dy, mi, prefix_hash(&reps, mi + 1)), w);
                                lens.entry(dy).or_default().insert(mi);
                                cache.insert(dy, w);
                                w
                            };
                            // Runs of passing coarse points; robust results at
                            // each, at more points for a short run, and the edges
                            // of each criterion's windows found by halving.
                            let mut windows: Vec<(usize, i32, i32)> = Vec::new();
                            let mut r = 0;
                            while r < passing.len() {
                                let mut e = r;
                                while e + 1 < passing.len() && passing[e + 1] == passing[e] + 1 {
                                    e += 1;
                                }
                                let mut pts: Vec<i32> = passing[r..=e].iter().map(|&ci| coarse[ci]).collect();
                                if e - r < 2 {
                                    let (lo, hi) = (coarse[passing[r]] - COARSE + 1, coarse[passing[e]] + COARSE - 1);
                                    let mut d = lo;
                                    while d <= hi {
                                        if d >= dy_lo && d <= dy_hi && !pts.contains(&d) {
                                            pts.push(d);
                                        }
                                        d += 2;
                                    }
                                    pts.sort();
                                }
                                let wv: Vec<[[(f32, i32); 2]; 2]> = pts.iter().map(|&d| get(d, &mut path)).collect();
                                for c in 0..CRIT.len() {
                                    let mut i = 0;
                                    while i < pts.len() {
                                        if !crit_ok(&wv[i], c) {
                                            i += 1;
                                            continue;
                                        }
                                        let mut k = i;
                                        while k + 1 < pts.len() && crit_ok(&wv[k + 1], c) {
                                            k += 1;
                                        }
                                        // Left edge: between the point before (or a
                                        // coarse step out) and pts[i].
                                        let mut f = if i > 0 { pts[i - 1] } else { (pts[i] - COARSE).max(dy_lo - 1) };
                                        let mut t = pts[i];
                                        while t - f > 1 {
                                            let m = (f + t) / 2;
                                            if crit_ok(&get(m, &mut path), c) {
                                                t = m;
                                            } else {
                                                f = m;
                                            }
                                        }
                                        let lo = t;
                                        let mut t = pts[k];
                                        let mut f = if k + 1 < pts.len() { pts[k + 1] } else { (pts[k] + COARSE).min(dy_hi + 1) };
                                        while f - t > 1 {
                                            let m = (f + t) / 2;
                                            if crit_ok(&get(m, &mut path), c) {
                                                t = m;
                                            } else {
                                                f = m;
                                            }
                                        }
                                        windows.push((c, lo, t));
                                        i = k + 1;
                                    }
                                }
                                r = e + 1;
                            }
                            tm[2] += tt.elapsed().as_secs_f64();
                            if windows.is_empty() {
                                continue;
                            }
                            let tt = std::time::Instant::now();
                            // Against the server's answers: the same plan in the full duel at each ping.
                            let mut alt: Vec<Vec<Rep>> = Vec::new();
                            for &rtt in &rtts {
                                let fr = full_reps(&start, &plan, LEN, 1, rtt);
                                if fr.iter().zip(reps.iter()).any(|(x, y)| x.feet != y.feet || x.on_ground != y.on_ground) && !alt.iter().any(|o: &Vec<Rep>| o.iter().zip(fr.iter()).all(|(x, y)| x.feet == y.feet)) {
                                    alt.push(fr);
                                }
                            }
                            tm[3] += tt.elapsed().as_secs_f64();
                            for &(c, lo, hi) in &windows {
                                {
                                    let mut dy = (lo + hi) / 2;
                                    if !crit_ok(&get(dy, &mut path), c) {
                                        dy = if crit_ok(&get(lo, &mut path), c) { lo } else { hi };
                                    }
                                    let mut w = get(dy, &mut path);
                                    for o in &alt {
                                        let mut r0 = o[0];
                                        r0.pitch = pitch_of(dy) % 360.0f32;
                                        w = min_w(w, worst_over(&start.world, &r0, o, &zs, &mut path));
                                    }
                                    let b_ok = crit_ok(&w, c);
                                    let k_end = crit_ticks(&w, c).max(1) as usize;
                                    let moved = path_len(&reps, own0, k_end + 1);
                                    let ke = (k_end + 1).min(LEN - 1);
                                    let dev = (reps[ke].own[0] - coast[ke].own[0]).hypot(reps[ke].own[2] - coast[ke].own[2]);
                                    let mut away = 2;
                                    if turn != 0 {
                                        for t in 2..=k_end + 1 {
                                            if plan.keys_at(t).sprint {
                                                away = t as i32 + 1;
                                            }
                                        }
                                    }
                                    let mut pd = plan.clone();
                                    pd.dy = dy;
                                    pd.keys.truncate((k_end + 3).min(LEN));
                                    if !seen_rows.insert((c, dy, pd.text())) {
                                        continue;
                                    }
                                    let worst = CRIT[c].1.iter().map(|&(a, b)| w[a][b].0).fold(8.0f32, f32::min);
                                    writeln!(
                                        file,
                                        "{}\t{}\t{}\t{}\t{:.2}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.3}\t{:.3}\t{}\t{}",
                                        state,
                                        CRIT[c].0,
                                        turn,
                                        dy,
                                        pitch_of(dy),
                                        hi - lo + 1,
                                        pd.text(),
                                        k_end,
                                        worst,
                                        w[0][0].0,
                                        w[0][1].0,
                                        w[1][0].0,
                                        w[1][1].0,
                                        w[0][0].1,
                                        w[0][1].1,
                                        w[1][0].1,
                                        w[1][1].1,
                                        if alt.is_empty() { "same" } else if b_ok { "ok" } else { "broke" },
                                        moved,
                                        dev,
                                        away,
                                        j
                                    )
                                    .unwrap();
                                    n_rows += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        eprintln!("{} turn {}: plans {} screened {} rows {} in {:.0}s (reps {:.1} screen {:.1} robust {:.1} full {:.1})", state, turn, n_plans, n_screen, n_rows, t0.elapsed().as_secs_f64(), tm[0], tm[1], tm[2], tm[3]);
    }
}

/// Monte Carlo of one plan in the full Match: random phase, jitter
/// 0-20 ms, carry half the time.
fn cmd_verify(state: &str, plan: &str, n: usize, seed: u64, ticks: usize, hits: bool) {
    let start = start_state(state);
    let plan = Plan::parse(plan);
    let mut rng = Rng::new(seed);
    let mut hist = [[0u64; 9]; 2];
    let mut ticks_hist = [0u64; 64];
    let mut not_thrown = 0u64;
    let mut jit_bad = [0u64; 5];
    let mut jit_n = [0u64; 5];
    for i in 0..n {
        // Ping 0-300 ms (both ways), the phase against the server's clock
        // anywhere, jitter by fifths: none, 5, 10, 20 ms, or any up to 20.
        let ping = (rng.next_u64() % 300_001) as i64;
        let phase = (rng.next_u64() % TICK_US as u64) as i64;
        let jb = (i % 5) as usize;
        let jit = [0i64, 5_000, 10_000, 20_000, (rng.next_u64() % 20_001) as i64][jb];
        let net = Net { up: 1 + ping / 2 + phase, down: 1 + ping / 2, jit };
        let carry = rng.chance(0.5);
        let hit = if hits {
            // One hit at a tick from the turn to the heal of the same
            // trial without it: vanilla sprint, plain, or low knockback,
            // from the side the player first looked to (+x).
            let r0 = trial(&start, &plan, net, carry, mix(seed ^ ((i as u64) << 1)), ticks);
            let ht = (rng.next_u64() % (r0.ticks.max(1) as u64 + 1)) as usize;
            let kb = [[0.9, 0.46], [0.4, 0.36], [0.2, 0.25]][(rng.next_u64() % 3) as usize];
            Some((ht, [-kb[0], kb[1], 0.0]))
        } else {
            None
        };
        let r = trial_hit(&start, &plan, net, carry, mix(seed ^ ((i as u64) << 1)), ticks, hit);
        if !r.thrown {
            not_thrown += 1;
        }
        let h = (r.heal.max(0.0) as usize).min(8);
        hist[carry as usize][h] += 1;
        ticks_hist[(r.ticks.max(0) as usize).min(63)] += 1;
        jit_n[jb] += 1;
        if h < 8 {
            jit_bad[jb] += 1;
        }
    }
    println!("verify {} plan {} n {}{}", state, plan.text(), n, if hits { " (a hit during the flight)" } else { "" });
    for c in 0..2 {
        println!("  carry {}: heal histogram 0..8 {:?}", c, hist[c]);
    }
    println!("  not thrown {}", not_thrown);
    println!("  below 8 by jitter (0, 5, 10, 20, U(0,20) ms): {:?} of {:?}", jit_bad, jit_n);
    let th: Vec<String> = ticks_hist.iter().enumerate().filter(|(_, &c)| c > 0).map(|(k, c)| format!("{}:{}", k, c)).collect();
    println!("  ticks {}", th.join(" "));
}

/// One plan, no jitter: the fast model and the full duel side by side.
fn cmd_debug(state: &str, plan: &str, up: i64, down: i64) {
    let start = start_state(state);
    let plan = Plan::parse(plan);
    let reps = client_reps(&start, &plan, 12);
    let fr = full_reps(&start, &plan, 12, up, down);
    for t in 0..8 {
        println!("tick {} clone feet {:?} og {} moved {:?} rot ({}, {}) | full feet {:?} og {}", t, reps[t].feet, reps[t].on_ground, reps[t].moved, reps[t].yaw, reps[t].pitch, fr[t].feet, fr[t].on_ground);
    }
    let mut path = Vec::new();
    let (from, m0) = throw_motion(&reps[0], [0.0; 3], false);
    potion_path(&start.world, from, m0, &mut path);
    println!("fast throw from {:?} m {:?} -> {:?}", from, m0, robust(&path, &reps, false));
    for (i, u) in path.iter().take(7).enumerate() {
        println!("  upd {} pos {:?} end {:?} block {}", i + 1, u.pos, u.end, u.block);
    }
    let mut m = start.clone();
    m.net.up_us[0] = up;
    m.net.down_us[0] = down;
    m.s.throw_motions.push_back(m0);
    let mut log = Vec::new();
    let n0 = m.s.splashes.len();
    for t in 0..40 {
        step_dbg(&mut m, &plan.input(t), &mut log);
        if m.s.splashes.len() > n0 {
            break;
        }
    }
    let first = log.iter().position(|e| e.2 > 0).unwrap_or(0);
    for (q, e) in log[first..].iter().take(7).enumerate() {
        println!("  full upd {} server feet {:?}", q + 1, e.1);
    }
    if let Some(sp) = m.s.splashes.get(n0) {
        println!("full: heal {} ticks {} at {:?} direct {:?}", sp.offered[0], sp.ticks, sp.pos, sp.on_player);
    }
    if let Some(e) = m.s.spawned.first() {
        println!("full spawned at {:?} motion {:?}", e.pos, e.motion);
    }
}

/// The agent's own planner (src/fullheal.rs) from a starting state, and
/// its plan in the full duel.
fn cmd_agentplan(state: &str, turn: i32, own: &str, carry: i8, n: usize) {
    let start = start_state(state);
    let o = Keys::parse(own);
    let inp = Input { forward: o.fw, strafe: o.st, sprint_key: o.sprint, dx: turn, slot_key: POT_SLOT + 1, ..Input::default() };
    let t0 = std::time::Instant::now();
    let own_k = pvpsim::fullheal::PKey { fw: o.fw, st: o.st, sprint: o.sprint, jump: false };
    let p = pvpsim::fullheal::plan(&start.c[0], &start.world, &inp, own_k, carry);
    let dt = t0.elapsed().as_secs_f64() * 1000.0;
    let Some(p) = p else {
        println!("{} turn {} own {}: no plan ({:.1} ms)", state, turn, own, dt);
        return;
    };
    let keys: Vec<Keys> = p.keys[..p.len].iter().map(|k| Keys { fw: k.fw, st: k.st, sprint: k.sprint, jump: k.jump }).collect();
    let plan = Plan { turn, dy: p.dy, keys, back: 0 };
    println!("{} turn {} own {}: plan {} class {} ticks {} window {} ({:.1} ms)", state, turn, own, plan.text(), p.class, p.ticks, p.window, dt);
    if n > 0 {
        cmd_verify(state, &plan.text(), n, 5, 40, false);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("kb") => cmd_kb(),
        Some("agentplan") => cmd_agentplan(&args[2], args[3].parse().unwrap(), &args[4], args.get(5).map_or(-1, |s| s.parse().unwrap()), args.get(6).map_or(2000, |s| s.parse().unwrap())),
        Some("debug") => cmd_debug(&args[2], &args[3], args.get(4).map_or(1, |s| s.parse().unwrap()), args.get(5).map_or(1, |s| s.parse().unwrap())),
        Some("validate") => cmd_validate(args.get(2).map_or(200, |s| s.parse().unwrap())),
        Some("search") => {
            let yaws: Vec<i32> = args.get(3).map_or(vec![0], |s| s.split(',').map(|x| x.parse().unwrap()).collect());
            let out = args.get(4).cloned().unwrap_or_else(|| format!("search_{}.tsv", args[2]));
            let jm: isize = args.get(5).map_or(9, |s| s.parse().unwrap());
            let mt: i32 = args.get(6).map_or(12, |s| s.parse().unwrap());
            cmd_search(&args[2], &yaws, &out, jm, mt)
        }
        Some("verify") => {
            let mut plan = args[3].clone();
            if args.get(6).map_or(false, |s| s == "back") {
                plan = Plan::parse(&plan).turned_back().expect("turn not a multiple of 45").text();
            }
            let hits = args.iter().any(|a| a == "hits");
            cmd_verify(&args[2], &plan, args.get(4).map_or(1000, |s| s.parse().unwrap()), args.get(5).map_or(1, |s| s.parse().unwrap()), 40, hits)
        }
        Some("state") => {
            let m = start_state(&args[2]);
            let l = &m.c[0].lp.l;
            println!("{}: pos {:?} motion {:?} on_ground {} sprinting {} yaw {} pitch {} server pos {:?}", args[2], l.body.pos, l.body.motion, l.body.on_ground, l.sprinting, l.yaw, l.pitch, m.s.p[0].l.body.pos);
        }
        _ => eprintln!("usage: fullheal kb|validate N|search STATE [YAWS] [fine] [OUT]|verify STATE PLAN N|state NAME"),
    }
}
