//! Duels played with `Agent` alone, without the training environment, and
//! everything that goes into and comes out of the agent of the first
//! player written to a file, tick by tick, for a copy of the agent in
//! another language to be checked against:
//! `cargo run --release --example agent_dump -- OUT.bin [TICKS]`.
//!
//! The file is a row of little-endian doubles (every number an agent
//! handles fits one exactly). It starts with 5050, the format (1), the
//! sizes of an observation and of an action; then for each tick:
//!
//! - 1 and the duel if a new duel starts here, else 0. The duel: half the
//!   arena, its floor, whether it has walls, whether hits cost health, the
//!   health of a plain hit, the pearl cooldown, the drink window in
//!   seconds, the two pings, the click rate, the mouse factor, and where
//!   the player stands (see `duel`).
//! - What the client holds when its packets are in and its tick is to come
//!   (see `view`), and the two combos.
//! - The observation, the action taken, and the keys and buttons it became.
//! - After the tick: where the player stands, whether it is drinking or
//!   eating, whether the other is seen to and what it holds, and whether a
//!   pearl and a potion were thrown.
use pvpsim::agent::{Agent, Duel, ACT_DIMS, ACT_HEADS, OBS_DIM};
use pvpsim::bot::{self, BotState};
use pvpsim::client::{Client, ClientCfg, Input};
use pvpsim::env::{new_duel, Mode, Params};
use pvpsim::mth::Rng;
use pvpsim::net::Item;
use pvpsim::sim::Match;
use pvpsim::world::World;
use std::io::Write;

/// Parameter numbers of `Params::set` and the values of the training runs.
const SETTINGS: [(i32, f64); 23] = [
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

fn duel(o: &mut Out, d: &Duel, walls: bool, ag: &Agent, ccfg: &ClientCfg, feet: [f64; 3]) {
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
}

/// Everything an agent reads of its client.
fn view(o: &mut Out, c: &Client, world: &World, ccfg: &ClientCfg) {
    let (lp, rp) = (&c.lp, &c.rp);
    let b = &lp.l.body;
    o.v(b.pos);
    o.v(b.motion);
    o.b(b.on_ground);
    o.b(b.collided_h);
    o.n(b.fall_distance);
    o.n(lp.l.yaw);
    o.n(lp.l.pitch);
    o.b(lp.l.sprinting);
    o.n(lp.l.jump_ticks);
    o.n(lp.l.attr.value());
    o.b(lp.server_sprint_state);
    o.n(lp.hurt_time);
    o.v(lp.last_kb);
    o.n(lp.ticks_since_kb);
    o.n(lp.ticks_since_swing);
    o.n(lp.ticks_since_attack);
    o.n(lp.ticks_since_hurt);
    o.n(lp.ticks_since_moved);
    o.b(lp.using_item);
    o.n(lp.right_click_delay);
    o.b(lp.flag_using);
    o.n(lp.held);
    o.n(lp.pearls);
    o.n(lp.potions);
    o.n(lp.drinks);
    o.n(lp.bottles);
    o.n(lp.steak);
    o.n(lp.potions_seen);
    o.n(lp.potions_pending);
    o.n(lp.food);
    o.n(lp.saturation);
    o.n(lp.speed_ticks);
    o.n(lp.health);
    o.b(lp.consuming());
    o.n(lp.use_slot);
    o.n(lp.use_count);
    o.b(lp.inv_open);
    for x in lp.rows.0 {
        o.n(x);
    }
    o.v(rp.pos);
    o.v(rp.prev_pos);
    o.v(rp.target);
    o.n(rp.increments);
    o.v(rp.bb.min);
    o.v(rp.bb.max);
    o.b(rp.on_ground);
    o.b(rp.sprinting);
    o.n(rp.hurt_time);
    o.b(rp.swing_in_progress);
    o.n(rp.swing_progress_int);
    o.n(rp.yaw);
    o.n(rp.pitch);
    o.n(rp.ticks_since_hurt);
    o.n(rp.ticks_since_jump);
    o.b(rp.using);
    o.n(rp.item as u8);
    o.n(rp.health);
    o.b(rp.fx);
    o.b(c.pick(world, ccfg).entity);
    o.n(c.thrown.len() as u32);
    for e in &c.thrown {
        o.n(e.id);
        o.n(e.kind as u8);
        o.b(e.mine);
        o.v(e.pos);
        o.v(e.motion);
        o.n(e.age);
        o.n(e.air);
        o.v(e.server_side());
    }
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
        let mut a = [1, 0, 0, 1, 1, 13, 10, 0, 0, 0];
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
            self.doing = self.rng.next_u64() % 14;
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
                _ => 8,
            };
            self.step = 0;
        }
        if self.step < self.len {
            let (t, last) = (self.step, self.step + 1 == self.len);
            self.step += 1;
            a[7] = 0;
            a[8] = 0;
            a[9] = 0;
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
                _ => a[9] = 5 + (t % 2) as i32,
            }
        }
        a
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("the file to write");
    let ticks: usize = args.get(2).map(|s| s.parse().expect("a number of ticks")).unwrap_or(12000);
    let mut p = Params::default();
    for (k, x) in SETTINGS {
        assert!(p.set(k, x));
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
    o.n(1.0);
    o.n(OBS_DIM as u32);
    o.n(ACT_HEADS as u32);
    let mut rng = Rng::new(23);
    let mut choose = [Chooser::new(5), Chooser::new(6)];
    let (mut written, mut duels, mut thrown, mut clicks, mut done) = (0usize, 0u32, [0u32; 2], [0u32; 2], [0u32; 1]);
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
        let cfg = &m.s.cfg;
        let d = Duel {
            arena_half: p.arena_half,
            floor_y: p.floor_y,
            real_health: !cfg.infinite_health,
            hit_health: cfg.hit_health(),
            pearl_cooldown: cfg.pearl_cooldown,
            drink_window_s: p.drink_window_s,
        };
        let mut bot_state = BotState::default();
        let mut combo = [0u32; 2];
        let mut left = rng.range(300.0, 900.0) as i32;
        let mut fresh = true;
        loop {
            let id = m.advance();
            for h in m.s.hits.drain(..) {
                combo[h.attacker] += 1;
                combo[h.victim] = 0;
            }
            m.s.splashes.clear();
            m.s.landings.clear();
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
            ag[id].write_obs(&m.c[id], &m.world, &m.ccfg, &d, [combo[id], combo[1 - id]], &mut obs);
            let action = choose[id].next();
            if id == 0 {
                o.b(fresh);
                if fresh {
                    duel(&mut o, &d, p.walls, &ag[0], &m.ccfg, m.c[0].lp.l.body.pos);
                    fresh = false;
                }
                view(&mut o, &m.c[0], &m.world, &m.ccfg);
                o.n(combo[0]);
                o.n(combo[1]);
                for x in obs {
                    o.n(x);
                }
                for x in action {
                    o.n(x);
                }
            }
            let was = m.c[id].lp.consuming();
            let inp = ag[id].keys(&m.c[id], &d, &action, &mut rng);
            let out = m.client_tick(id, &inp);
            ag[id].ticked(&m.c[id], &d, &inp, &out);
            if id == 0 {
                let c = &m.c[0];
                keys(&mut o, &inp);
                o.v(c.lp.l.body.pos);
                o.b(c.lp.consuming());
                o.b(c.rp.using);
                o.n(c.rp.item as u8);
                o.b(out.threw);
                o.b(out.threw_potion);
                written += 1;
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
}
