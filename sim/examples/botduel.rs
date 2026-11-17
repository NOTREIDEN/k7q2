//! Let two scripted bots fight and print how each did.
//!
//!     cargo run --release --example botduel -- A B [--duels N] [--seconds S] [--ping LO HI] [--health] [--trace] [--ticks]
//!
//! A and B are a level (`4`) optionally followed by settings, for example
//! `4:jr=0:strafe=0:noise=1.5:click=0.6:pearl=0:escape=0`. `--trace` prints
//! every hit and `--ticks` every client tick as well. `--health` gives the
//! bots real health, armour and potions; a duel then ends when one dies.
use pvpsim::bot::{self, BotParams, BotState};
use pvpsim::env::{new_match, Mode, Params};
use pvpsim::mth::Rng;

fn spec(s: &str) -> BotParams {
    let mut it = s.split(':');
    let mut p = BotParams::level(it.next().unwrap().parse().expect("level"));
    for kv in it {
        let (k, v) = kv.split_once('=').expect("key=value");
        let f: f64 = v.parse().expect("number");
        match k {
            "jr" => p.jump_reset = f != 0.0,
            "strafe" => p.strafe = f != 0.0,
            "wtap" => p.wtap = f != 0.0,
            "noise" => p.aim_noise = f,
            "turn" => p.max_turn = f,
            "click" => p.click_prob = f,
            "pearl" => p.pearl_attack = f,
            "escape" => p.pearl_escape = f,
            "potion" => p.potion = f,
            _ => panic!("unknown setting {k}"),
        }
    }
    p
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut names = Vec::new();
    let mut duels = 200u64;
    let mut seconds = 60.0;
    let mut params = Params::default();
    let mut trace = false;
    let mut per_tick = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--duels" => {
                duels = args[i + 1].parse().unwrap();
                i += 1;
            }
            "--seconds" => {
                seconds = args[i + 1].parse().unwrap();
                i += 1;
            }
            "--ping" => {
                params.ping_min_ms = args[i + 1].parse().unwrap();
                params.ping_max_ms = args[i + 2].parse().unwrap();
                i += 2;
            }
            "--health" => {
                params.infinite_health = false;
                params.start_hurt = 0.0;
                params.potions = [7.0, 7.0];
            }
            "--trace" => trace = true,
            "--ticks" => {
                trace = true;
                per_tick = true;
            }
            a => names.push(a.to_string()),
        }
        i += 1;
    }
    if names.len() != 2 {
        eprintln!("usage: botduel A B [--duels N] [--seconds S] [--ping LO HI] [--trace] [--ticks]");
        std::process::exit(2);
    }
    let base = [spec(&names[0]), spec(&names[1])];
    let rounds = (seconds * 20.0) as u32;
    let mut hits = [0u64; 2];
    let mut sprint = [0u64; 2];
    let mut best = [0u64; 2];
    let mut clicks = [0u64; 2];
    // Pearls that landed, and how far from the opponent they put their thrower.
    let mut pearls = [0u64; 2];
    let mut pearl_dist = [0.0f64; 2];
    // With real health: deaths, potions that broke, the health they gave
    // their thrower and the other player, and how long the duels lasted.
    let mut deaths = [0u64; 2];
    let mut potions = [0u64; 2];
    let mut heal = [[0.0f64; 2]; 2];
    let mut offered = [0.0f64; 2];
    let mut left = [0u64; 2];
    let mut lasted = 0u64;
    for d in 0..duels {
        let mut rng = Rng::new(0x5EED_0000 + d);
        let (mut m, ping, _) = new_match(&params, &mut rng, Mode::SelfPlay);
        let mut bp = base;
        for b in bp.iter_mut() {
            b.pearl_cd = m.s.cfg.pearl_cooldown;
        }
        let mut st = [BotState::default(); 2];
        let mut combo = [0u64; 2];
        let mut top = [0u64; 2];
        let mut next = m.advance();
        let mut done = 0;
        while done < rounds {
            let inp = bot::act(&m.c[next], &m.world, &m.ccfg, &bp[next], &mut st[next], &mut rng);
            let out = m.client_tick(next, &inp);
            if out.attacked {
                clicks[next] += 1;
            }
            if per_tick {
                let c = &m.c[next];
                let dx = c.rp.pos[0] - c.lp.l.body.pos[0];
                let dz = c.rp.pos[2] - c.lp.l.body.pos[2];
                let t = [m.s.p[0].l.body.pos, m.s.p[1].l.body.pos];
                let o = &m.c[1 - next].lp.l.body.pos;
                let now = ((o[0] - c.lp.l.body.pos[0]).powi(2) + (o[2] - c.lp.l.body.pos[2]).powi(2)).sqrt();
                println!(
                    "  c{} t{:4} fwd {:2} jump {} atk {} | sees {:.2} now {:.2} server {:.2} | ground {} sprint {} hurt {:2} speed {:.2} | srv hrt {:2}/{:2} srvsprint {}",
                    next,
                    m.client_ticks[next],
                    inp.forward,
                    inp.jump as u8,
                    if out.attacked { "A" } else if out.swung { "s" } else { "." },
                    (dx * dx + dz * dz).sqrt(),
                    now,
                    ((t[0][0] - t[1][0]).powi(2) + (t[0][2] - t[1][2]).powi(2)).sqrt(),
                    c.lp.l.body.on_ground as u8,
                    c.lp.l.sprinting as u8,
                    c.lp.hurt_time,
                    (c.lp.l.body.motion[0].powi(2) + c.lp.l.body.motion[2].powi(2)).sqrt(),
                    m.s.p[0].hurt_resistant_time,
                    m.s.p[1].hurt_resistant_time,
                    m.s.p[next].l.sprinting as u8
                );
            }
            if next == 1 {
                done += 1;
            }
            next = m.advance();
            for l in m.s.landings.drain(..) {
                let o = m.s.p[1 - l.thrower].l.body.pos;
                pearls[l.thrower] += 1;
                pearl_dist[l.thrower] += (l.pos[0] - o[0]).hypot(l.pos[2] - o[2]);
            }
            for s in m.s.splashes.drain(..) {
                potions[s.thrower] += 1;
                heal[s.thrower][0] += s.healed[s.thrower] as f64;
                heal[s.thrower][1] += s.healed[1 - s.thrower] as f64;
                offered[s.thrower] += s.offered[s.thrower] as f64;
                if trace {
                    println!("tick {:5}  potion of {}  offered {:?}  healed {:?}  after {} ticks", m.server_ticks, s.thrower, s.offered, s.healed, s.ticks);
                }
            }
            if !m.s.p[0].alive() || !m.s.p[1].alive() {
                for k in 0..2 {
                    deaths[k] += !m.s.p[k].alive() as u64;
                }
                break;
            }
            for h in m.s.hits.drain(..) {
                if trace {
                    let seen = |k: usize| {
                        let c = &m.c[k];
                        let dx = c.rp.pos[0] - c.lp.l.body.pos[0];
                        let dz = c.rp.pos[2] - c.lp.l.body.pos[2];
                        (dx * dx + dz * dz).sqrt()
                    };
                    println!(
                        "tick {:5}  {} hits {}  true {:.2}  sprint {}  kb {:.2}  seen by 0 {:.2}  seen by 1 {:.2}  ping {:.0}/{:.0}",
                        m.server_ticks,
                        h.attacker,
                        h.victim,
                        h.dist,
                        h.sprint as u8,
                        (h.kb[0] * h.kb[0] + h.kb[2] * h.kb[2]).sqrt(),
                        seen(0),
                        seen(1),
                        ping[0],
                        ping[1]
                    );
                }
                hits[h.attacker] += 1;
                if h.sprint {
                    sprint[h.attacker] += 1;
                }
                combo[h.attacker] += 1;
                combo[h.victim] = 0;
                top[h.attacker] = top[h.attacker].max(combo[h.attacker]);
            }
        }
        for k in 0..2 {
            best[k] += top[k];
            left[k] += m.s.p[k].potions.count_ones() as u64;
        }
        lasted += done as u64;
    }
    let mins = duels as f64 * seconds / 60.0;
    println!("bot                             hits/min  sprint-hit  attacks/s  best-combo  pearls/min  blocks-from-opponent");
    for k in 0..2 {
        println!(
            "{:30}  {:8.1}  {:10.2}  {:9.1}  {:10.1}  {:10.1}  {:20.1}",
            names[k],
            hits[k] as f64 / mins,
            sprint[k] as f64 / hits[k].max(1) as f64,
            clicks[k] as f64 / (mins * 60.0),
            best[k] as f64 / duels as f64,
            pearls[k] as f64 / mins,
            pearl_dist[k] / pearls[k].max(1) as f64
        );
    }
    if !params.infinite_health {
        println!("duels lasted {:.1} s on average", lasted as f64 / duels as f64 / 20.0);
        println!("bot                             deaths  potions/duel  health-each  given-away  offered-each  left-at-end");
        for k in 0..2 {
            let n = potions[k].max(1) as f64;
            println!(
                "{:30}  {:6}  {:12.1}  {:11.1}  {:10.1}  {:12.1}  {:11.1}",
                names[k],
                deaths[k],
                potions[k] as f64 / duels as f64,
                heal[k][0] / n,
                heal[k][1] / n,
                offered[k] / n,
                left[k] as f64 / duels as f64
            );
        }
    }
}
