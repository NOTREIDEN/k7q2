//! `agent_dump` with the `fullheal` macro on (its gate and view restore
//! from FULLHEAL_S, FULLHEAL_D and FULLHEAL_R, default 4, 0 and 4), format 6. On top of format 5 it
//! writes, each tick, the movement state of the client's own player that
//! the macro's copy of the client tick needs (`walk`), after the keys the
//! plan under way (`plan`), and after the tick the movement state again
//! (`walk_after`), for the mod's copy of the client tick to be checked on
//! every tick it applies to:
//! `cargo run --release --example fh_dump -- OUT.bin [TICKS] [SEED]`.
//!
//! The rest as in agent_dump:
//!
//! The file is a row of little-endian doubles (every number an agent
//! handles fits one exactly). It starts with 5050, the format (5), the
//! sizes of an observation and of an action; then for each tick:
//!
//! - 1 and the duel if a new duel starts here, else 0. The duel: half the
//!   arena, its floor, whether it has walls, whether hits cost health, the
//!   health of a plain hit, the pearl cooldown, the drink window in
//!   seconds, the two pings, the click rate, the mouse factor, where the
//!   player stands, and the terrain if there is any: the corner and the
//!   sizes of its box, every block of it and which columns are open to the
//!   void (see `duel`).
//! - The packets the client was handed since its last tick that an agent's
//!   own counts go by, how many and then each (see `packet`).
//! - What the client holds when its packets are in and its tick is to come
//!   (`pvpsim::agent::view`).
//! - The observation, the action taken, and the keys and buttons it became.
//! - After the tick: where the player stands, whether it is drinking or
//!   eating, whether the other is seen to and what it holds, whether a
//!   pearl and a potion were thrown, whether a snowball or an egg was and
//!   whether a rod was used, whether it swung and whether it hit,
//!   and the place and the view the server had been told before the tick.
use pvpsim::agent::{view, Agent, Duel, ACT_DIMS, ACT_HEADS, OBS_DIM};
use pvpsim::fullheal::PLAN_TICKS;
use pvpsim::bot::{self, BotState};
use pvpsim::client::{Client, ClientCfg, Input};
use pvpsim::env::{new_duel, Mode, Params};
use pvpsim::mth::Rng;
use pvpsim::net::{Item, ThrownEv, S2C};
use pvpsim::sim::Match;
use std::io::Write;

/// Parameter numbers of `Params::set` and the values of the training runs.
const SETTINGS: [(i32, f64); 32] = [
    // A tool in every duel on a server that throws as 1.8 does, rods most
    // often, few snowballs and eggs so that some run out in hand.
    (169, 1.0),
    (170, 2.0),
    (173, 8.0),
    (175, 0.7),
    (130, 0.6),
    (131, 1.0),
    (125, 0.5),
    (20, 0.7),
    (48, 0.5),
    (57, 1.0),
    (7, 16.0),
    (67, 0.8),
    (85, 0.5),
    (90, 0.3),
    (95, 5.0),
    (96, 5.0),
    (97, 0.5),
    (104, 2.0),
    (105, 0.5),
    (106, 31.0),
    (113, 0.4),
    (107, 12.0),
    (108, 28.0),
    (110, 1.5),
    (111, 0.1),
    (42, 190.0),
    (43, 210.0),
    (112, 0.15),
    (37, 0.3),
    (39, 0.2),
    // Cobwebs in half of the duels, and caught in one at the start in half of those.
    (152, 0.5),
    (154, 0.5),
];

struct Out(Vec<f64>);

impl Out {
    fn n<T: Into<f64>>(&mut self, v: T) {
        self.0.push(v.into());
    }
    fn b(&mut self, v: bool) {
        self.0.push(v as u8 as f64);
    }
    fn v(&mut self, v: [f64; 3]) {
        self.0.extend_from_slice(&v);
    }
}

fn duel(o: &mut Out, d: &Duel, walls: bool, ag: &Agent, ccfg: &ClientCfg, feet: [f64; 3], world: &pvpsim::world::World) {
    o.n(d.arena_half);
    o.n(d.floor_y);
    o.b(walls);
    o.b(d.real_health);
    o.n(d.hit_health);
    o.n(d.pearl_cooldown);
    o.n(d.drink_window_s);
    o.n(ag.ping_ms[0]);
    o.n(ag.ping_ms[1]);
    o.n(ag.cps);
    o.n(ccfg.mouse_f1);
    o.v(feet);
    match world.terrain() {
        None => o.b(false),
        Some(t) => {
            o.b(true);
            for v in [t.x0, t.y0, t.z0, t.nx, t.ny, t.nz] {
                o.n(v);
            }
            for &b in &t.cells {
                o.n(b);
            }
            for &v in &t.void {
                o.b(v);
            }
        }
    }
}

/// A packet as the agent's counts need it; returns whether it is one of
/// those. Before the client handles it: 1 a velocity for its own player
/// (in 8000ths of a block per tick), 2 a player was hurt (1 its own, 0 the
/// other), 3 its own player was put somewhere, 4 the other player was (where
/// the copy was heading before, and the new place in 32nds), 5 a pearl (0)
/// or a potion (1), a snowball (2), an egg (3) or a hook (4) appeared (its
/// number, the kind, for a hook whether it is its own, the place in 32nds), 6
/// the velocity of one (number, 8000ths), 9 one is gone (number). After
/// the client handled it: 7 one moved (number, the move in 32nds, where the
/// copy now is), 8 one was put somewhere (number, 32nds, where the copy
/// now is).
fn packet(o: &mut Out, p: &S2C, c: &Client, after: bool) -> u32 {
    let mut n = 0;
    match *p {
        S2C::Velocity { own: true, v } if !after => {
            o.n(1);
            for x in v {
                o.n(x);
            }
            n = 1;
        }
        S2C::Hurt { own } if !after => {
            o.n(2);
            o.b(own);
            n = 1;
        }
        S2C::PosLook { .. } if !after => {
            o.n(3);
            n = 1;
        }
        S2C::Teleport { p, .. } if !after => {
            o.n(4);
            o.v(c.rp.target);
            for x in p {
                o.n(x);
            }
            n = 1;
        }
        S2C::Destroy { ids, n: count } if !after => {
            for id in &ids[..count as usize] {
                o.n(9);
                o.n(*id);
                n += 1;
            }
        }
        S2C::Thrown { id, ev } => {
            let (code, x) = match ev {
                ThrownEv::Spawn { p, .. } if !after => (5, p),
                ThrownEv::Velocity(v) if !after => (6, v),
                ThrownEv::Teleport { p, .. } if after => (8, p),
                ThrownEv::RelMove { d, .. } | ThrownEv::LookMove { d, .. } if after => (7, d.map(|x| x as i32)),
                ThrownEv::Look { .. } if after => (7, [0; 3]),
                _ => return 0,
            };
            o.n(code);
            o.n(id);
            if let ThrownEv::Spawn { kind, own, .. } = ev {
                o.n(kind as u8);
                // A hook's packet says whose it is.
                if kind == pvpsim::net::Kind::Hook {
                    o.b(own);
                }
            }
            for v in x {
                o.n(v);
            }
            if code == 7 || code == 8 {
                o.v(c.thrown.iter().find(|e| e.id == id).map(|e| e.pos).unwrap_or([0.0; 3]));
            }
            n = 1;
        }
        _ => {}
    }
    n
}

fn keys(o: &mut Out, i: &Input) {
    o.n(i.forward);
    o.n(i.strafe);
    o.b(i.jump);
    o.b(i.sprint_key);
    o.b(i.attack);
    o.n(i.extra_clicks);
    o.n(i.dx);
    o.n(i.dy);
    o.b(i.use_item);
    o.b(i.use_again);
    o.n(i.slot_key);
    o.n(i.inv);
}

/// The movement state of the client's own player beyond the view.
fn walk(o: &mut Out, c: &Client) {
    let lp = &c.lp;
    let l = &lp.l;
    o.v(l.body.bb.min);
    o.v(l.body.bb.max);
    o.b(l.body.in_web);
    o.n(l.attr.base);
    o.b(l.attr.sprint);
    o.n(if l.attr.speed_amp >= 0 { 0.20000000298023224 * (l.attr.speed_amp as i32 + 1) as f64 } else { 0.0 });
    o.n(l.jump_factor);
    o.n(lp.mi_forward);
    o.n(lp.sprinting_ticks_left);
    o.n(lp.sprint_toggle_timer);
    o.v(lp.last_reported);
    o.n(lp.last_yaw);
    o.n(lp.last_pitch);
    o.n(lp.position_update_ticks);
}

/// The whole movement state after a tick.
fn walk_after(o: &mut Out, c: &Client) {
    let lp = &c.lp;
    let l = &lp.l;
    o.v(l.body.pos);
    o.v(l.body.motion);
    o.b(l.body.on_ground);
    o.b(l.body.collided_h);
    o.n(l.body.fall_distance);
    o.n(l.yaw);
    o.n(l.pitch);
    o.b(l.sprinting);
    o.n(l.jump_ticks);
    o.b(lp.server_sprint_state);
    o.b(lp.using_item);
    walk(o, c);
}

/// The plan under way after the keys: its tick, and on the tick it was
/// made (its tick then 1) all of it.
fn plan(o: &mut Out, ag: &Agent) -> u32 {
    match ag.heal_plan_info() {
        None => {
            o.n(0);
            0
        }
        Some((p, t)) => {
            o.n(t as u32);
            if t == 1 {
                o.n(p.dy);
                o.n(p.yaw);
                o.n(p.len as u32);
                o.n(p.ticks);
                o.n(p.class);
                o.n(p.window);
                o.n(p.pitch);
                o.n(p.jump);
                for k in p.keys.iter().take(PLAN_TICKS) {
                    o.n(k.fw);
                    o.n(k.st);
                    o.b(k.sprint);
                    o.b(k.jump);
                }
            }
            t as u32
        }
    }
}

fn env_i32(name: &str, default: i32) -> i32 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// Actions that get everywhere: stretches of doing little, in which now
/// and then something is done that takes several ticks in a row (a drink,
/// a meal, potions from the inventory, a potion at the feet, a pearl, a
/// block, and one of these on top of another), between stretches of
/// anything at all.
struct Chooser {
    rng: Rng,
    tick: u64,
    doing: u64,
    step: u32,
    len: u32,
}

impl Chooser {
    fn new(seed: u64) -> Chooser {
        Chooser { rng: Rng::new(seed), tick: 0, doing: 0, step: 0, len: 0 }
    }

    fn next(&mut self) -> [i32; ACT_HEADS] {
        let wild = (self.tick / 200) % 2 == 1;
        self.tick += 1;
        let mut a = [1, 0, 0, 1, 1, 13, 10, 0, 0, 0, 0, 10, 0];
        for (k, x) in a.iter_mut().enumerate() {
            let r = self.rng.next_u64();
            if wild || r % 8 == 0 {
                *x = ((r >> 8) % ACT_DIMS[k] as u64) as i32;
            }
        }
        if wild {
            self.len = 0;
            return a;
        }
        if self.step >= self.len && self.rng.next_u64() % 25 == 0 {
            self.doing = self.rng.next_u64() % 17;
            let some = |rng: &mut Rng, lo: u32, n: u64| lo + (rng.next_u64() % n) as u32;
            self.len = match self.doing {
                0 | 1 => 45,
                2 => some(&mut self.rng, 2, 7),
                3 => some(&mut self.rng, 2, 4),
                4 => some(&mut self.rng, 3, 3),
                5 | 7 => 1,
                6 => some(&mut self.rng, 3, 13),
                8 => 52,
                9 => some(&mut self.rng, 6, 20),
                10 => 45,
                11 | 12 => 6,
                13 => 8,
                _ => some(&mut self.rng, 3, 30),
            };
            self.step = 0;
        }
        if self.step < self.len {
            let (t, last) = (self.step, self.step + 1 == self.len);
            self.step += 1;
            a[7] = 0;
            a[8] = 0;
            a[9] = 0;
            a[12] = 0;
            match self.doing {
                0 => a[9] = 2,
                1 => a[9] = 3,
                2 => a[9] = 5,
                3 => a[9] = 6,
                4 => {
                    // Down, the potions, and up again.
                    a[6] = if t == 0 { 28 } else if last { 27 } else { 10 };
                    a[9] = if t == 0 || last { 0 } else { 1 };
                }
                5 => a[8] = 1,
                6 => a[7] = 1,
                7 => a[9] = 4,
                // A drink, and a potion from the inventory right after it.
                8 => a[9] = if t < 45 { 2 } else { 5 },
                // A meal or a drink given up.
                9 => a[9] = if last { 4 } else { 2 + (self.len % 2) as i32 },
                // A potion asked for in the middle of a drink.
                10 => a[9] = if (10..13).contains(&t) { 1 } else { 2 },
                // A pearl and a potion with the right button down before.
                11 => {
                    a[7] = (t < 4) as i32;
                    a[8] = (t == 3) as i32;
                }
                12 => {
                    a[7] = (t < 4) as i32;
                    a[9] = (t == 3 || t == 4) as i32;
                }
                // Both kinds of potion from the inventory in turn.
                13 => a[9] = 5 + (t % 2) as i32,
                // The tool: used once and the sword back after a while,
                // used again and again, or used with a potion or a pearl
                // asked for in the middle.
                14 => a[12] = if t == 0 { 1 } else if last { 2 } else { 0 },
                15 => a[12] = if last { 2 } else { 1 },
                _ => {
                    a[12] = if t < 3 { 1 } else { 0 };
                    a[9] = (t == 4) as i32;
                    a[8] = (t == 6) as i32;
                }
            }
        }
        a
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("the file to write");
    let ticks: usize = args.get(2).map(|s| s.parse().expect("a number of ticks")).unwrap_or(12000);
    let seed: u64 = args.get(3).map(|s| s.parse().expect("a seed")).unwrap_or(23);
    let mut p = Params::default();
    for (k, x) in SETTINGS {
        assert!(p.set(k, x));
    }
    // FH_PARAMS: more parameters of `Params::set`, as "k=v,k=v".
    if let Ok(extra) = std::env::var("FH_PARAMS") {
        for kv in extra.split(',').filter(|s| !s.is_empty()) {
            let (k, v) = kv.split_once('=').expect("k=v");
            assert!(p.set(k.trim().parse().expect("a parameter number"), v.trim().parse().expect("a value")), "no parameter {k}");
        }
    }
    // Every fourth duel with a hotbar that is full from the start, Speed
    // about to run out and more potions in the inventory, for what is rare
    // otherwise: a bottle that has to make room first.
    let mut full = p.clone();
    for (k, x) in [(67, 1.0), (85, 1.0), (86, 0.0), (87, 5.0), (73, 7.0), (74, 7.0), (88, 1.0), (89, 2.0), (97, 1.0), (98, 2.0), (99, 6.0), (62, 1.0), (105, 0.0)] {
        assert!(full.set(k, x));
    }
    let mut o = Out(Vec::new());
    o.n(5050.0);
    o.n(6.0);
    o.n(OBS_DIM as u32);
    o.n(ACT_HEADS as u32);
    // The gate as fullheal::gate_open reads it.
    let heal = pvpsim::fullheal::FullHealCfg {
        s: env_i32("FULLHEAL_S", 4),
        d: env_i32("FULLHEAL_D", 0),
        restore: env_i32("FULLHEAL_R", 4),
    };
    o.n(heal.s);
    o.n(heal.d);
    o.n(heal.restore);
    let mut rng = Rng::new(seed);
    let mut choose = [Chooser::new(seed.wrapping_mul(7) + 5), Chooser::new(seed.wrapping_mul(7) + 6)];
    // FH_TURNS: the share of the first player's ticks on which a potion
    // thrown with a turn is asked for on top of the chooser's (a fifth of
    // them two potions), at a pitch drawn at random, for more plans.
    let turns: f64 = std::env::var("FH_TURNS").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    // FH_HURT: the same share for the ticks soon after a hurt of its own
    // (ticks_since_hurt at most 4), where the gate of the macro opens.
    let hurt_turns: f64 = std::env::var("FH_HURT").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let mut trng = Rng::new(seed ^ 0x5eed);
    // Plans made, ticks with a plan under way after the keys.
    let (mut plans, mut plan_ticks) = (0u32, 0u32);
    let (mut written, mut duels, mut thrown, mut clicks, mut done) = (0usize, 0u32, [0u32; 2], [0u32; 2], [0u32; 1]);
    // Ticks with the carry of throws not seen yet, seen not to be, seen to be.
    let mut seen = [0u32; 3];
    // Ticks with a tool, its uses, and the tool's hits that counted.
    let mut tools = [0u32; 3];
    while written < ticks {
        // Half of the duels against a copy that does as much at random,
        // half against the scripted bot.
        let mode = if duels % 2 == 0 { Mode::SelfPlay } else { Mode::Bot };
        duels += 1;
        let p = if duels % 4 == 3 { &full } else { &p };
        let (mut m, ping, bp, _) = new_duel(p, &mut rng, mode);
        let mut m: &mut Match = &mut m;
        let own = rng.range(p.cps_min, p.cps_max.max(p.cps_min));
        let (lo, hi) = if p.opp_cps_min < 0.0 { (p.cps_min, p.cps_max) } else { (p.opp_cps_min, p.opp_cps_max) };
        let cps = [own.max(1.0), rng.range(lo, hi.max(lo)).max(1.0)];
        let mut ag = [0, 1].map(|a| Agent::new([ping[a], ping[1 - a]], cps[a], m.c[a].lp.l.body.pos));
        for g in ag.iter_mut() {
            g.full_heal = Some(heal);
        }
        let cfg = &m.s.cfg;
        let d = Duel {
            arena_half: p.arena_half,
            floor_y: p.floor_y,
            center: [0.0, 0.0],
            real_health: !cfg.infinite_health,
            hit_health: cfg.hit_health(),
            pearl_cooldown: cfg.pearl_cooldown,
            drink_window_s: p.drink_window_s,
        };
        let mut bot_state = BotState::default();
        let mut left = rng.range(300.0, 900.0) as i32;
        let mut fresh = true;
        let mut told = Out(Vec::new());
        let mut count = 0u32;
        loop {
            let id = m.advance_seen(|who, p, c, after| {
                if who == 0 {
                    count += packet(&mut told, p, c, after);
                }
            });
            m.s.hits.clear();
            m.s.splashes.clear();
            m.s.landings.clear();
            tools[2] += m.s.thrown_hits.iter().filter(|h| h.thrower == 0 && h.victim == 1).count() as u32;
            m.s.thrown_hits.clear();
            if id == 0 {
                left -= 1;
                if left < 0 || written >= ticks || !m.s.p[0].alive() || !m.s.p[1].alive() {
                    break;
                }
            }
            let m = &mut m;
            if id == 1 && mode == Mode::Bot {
                let mut b = bot::act(&m.c[1], &m.world, &m.ccfg, &bp, &mut bot_state, &mut rng);
                if p.opp_cps_min >= 0.0 {
                    ag[1].limit_clicks(&mut b, &mut rng);
                }
                let out = m.client_tick(1, &b);
                ag[1].ticked(&m.c[1], &d, &b, &out);
                continue;
            }
            ag[id].before_tick(&m.c[id], &m.world);
            let mut obs = [0f32; OBS_DIM];
            ag[id].write_obs(&m.c[id], &m.world, &m.ccfg, &d, &mut obs);
            let mut action = choose[id].next();
            let share = if m.c[0].lp.ticks_since_hurt <= 4 { turns.max(hurt_turns) } else { turns };
            if id == 0 && share > 0.0 && trng.chance(share) {
                let r = trng.next_u64();
                action[9] = 7 + (r % 3) as i32 + if r % 5 == 0 { 3 } else { 0 };
                action[11] = ((r >> 8) % ACT_DIMS[11] as u64) as i32;
            }
            if id == 0 {
                seen[(obs[153] * 2.0) as usize] += 1;
                o.b(fresh);
                if fresh {
                    duel(&mut o, &d, p.walls, &ag[0], &m.ccfg, m.c[0].lp.l.body.pos, &m.world);
                    fresh = false;
                }
                o.n(count);
                o.0.append(&mut told.0);
                count = 0;
                o.0.append(&mut view(&m.c[0], &m.world, &m.ccfg));
                walk(&mut o, &m.c[0]);
                for x in obs {
                    o.n(x);
                }
                for x in action {
                    o.n(x);
                }
            }
            let was = m.c[id].lp.consuming();
            let (said, said_yaw, said_pitch) = (m.c[id].lp.last_reported, m.c[id].lp.last_yaw, m.c[id].lp.last_pitch);
            let inp = ag[id].keys(&m.c[id], &m.world, &d, &action, &mut rng);
            let out = m.client_tick(id, &inp);
            ag[id].ticked(&m.c[id], &d, &inp, &out);
            if id == 0 {
                let c = &m.c[0];
                keys(&mut o, &inp);
                let t = plan(&mut o, &ag[0]);
                plans += (t == 1) as u32;
                plan_ticks += (t > 0) as u32;
                o.v(c.lp.l.body.pos);
                o.b(c.lp.consuming());
                o.b(c.rp.using);
                o.n(c.rp.item as u8);
                o.b(out.threw);
                o.b(out.threw_potion);
                o.b(out.threw_ball);
                o.b(out.used_rod);
                o.b(out.swung);
                o.b(out.attacked);
                o.v(said);
                o.n(said_yaw);
                o.n(said_pitch);
                walk_after(&mut o, c);
                written += 1;
                tools[0] += (c.lp.tool.item != Item::Empty) as u32;
                tools[1] += (out.threw_ball || out.used_rod) as u32;
                thrown[0] += out.threw as u32;
                thrown[1] += out.threw_potion as u32;
                clicks[0] += out.moved.is_some() as u32;
                clicks[1] += (out.moved == Some(Item::Bottle)) as u32;
                done[0] += (was && !c.lp.consuming() && c.lp.held != 0) as u32;
            }
        }
    }
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).expect("the file could not be made"));
    for x in &o.0 {
        f.write_all(&x.to_le_bytes()).unwrap();
    }
    println!(
        "{written} ticks of {duels} duels, {} pearls, {} potions, {} drinks and meals to the end, {} clicks in the inventory of which {} on a bottle: {} numbers",
        thrown[0],
        thrown[1],
        done[0],
        clicks[0],
        clicks[1],
        o.0.len()
    );
    println!("carry of throws: not seen yet on {}, seen not to be on {}, seen to be on {}", seen[1], seen[0], seen[2]);
    println!("a tool on {} ticks, used {} times, {} hits with it", tools[0], tools[1], tools[2]);
    println!("fullheal: {plans} plans made, {plan_ticks} ticks with a plan under way");
    let cnt = |i: usize| pvpsim::fullheal::COUNTS[i].load(std::sync::atomic::Ordering::Relaxed);
    println!("fullheal counters (both players): {} turned throws, {} left to the policy by the gate, {} with no plan, {} plans dropped by a hit", cnt(0), cnt(46), cnt(4), cnt(5));
}
