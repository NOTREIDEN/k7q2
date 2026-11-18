//! A potion thrown with a turn by a player that was sprinting before it:
//! for each pitch and each of a few simple ways of moving from the turn on
//! (a key held for some ticks, with or without sprint), whether the potion
//! heals all eight. Shows what there is to learn when the turn comes in
//! the middle of a run, as it does in a fight.
//! `cargo run --release --example turn_fight -- [SEEDS] [CARRY] [PRE] [WAY]`
use pvpsim::agent::{throw_pitch, ACT_HEADS, BAR_TURN, OBS_DIM, THROW_PITCHES};
use pvpsim::env::{Env, Mode, Params, PRIV_DIM};

const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0];
const DURS: [i32; 6] = [1, 3, 5, 8, 12, 40];

fn params(carry: f64) -> Params {
    let mut p = Params::default();
    p.infinite_health = false;
    p.potions = [7.0, 7.0];
    p.start_hurt = 1.0;
    p.start_health = [6.0, 6.0];
    p.pearl_prob = 0.0;
    p.start_dist_min = 30.0;
    p.start_dist_max = 30.0;
    p.ep_ticks_min = 100_000.0;
    p.ep_ticks_max = 100_000.0;
    p.throw_carry_prob = carry;
    p
}

/// (key 0 none / 1 W / 2 S, sprint, ticks held from the turn on)
fn plans() -> Vec<(i32, i32, i32)> {
    let mut v = vec![];
    for key in 0..3 {
        for sprint in 0..2 {
            if sprint == 1 && key != 1 {
                continue;
            }
            for &d in DURS.iter() {
                if key == 0 && d > 1 {
                    continue;
                }
                v.push((key, sprint, d));
            }
        }
    }
    v
}

fn heal(seed: u64, carry: f64, pre: i32, way: i32, k: i32, plan: (i32, i32, i32)) -> f32 {
    let mut e = Env::new(Mode::SelfPlay, params(carry), seed);
    let (mut fo, mut fp) = ([0.0f32; OBS_DIM], [0.0f32; PRIV_DIM]);
    let mut hp0 = e.m.s.p[0].health;
    let mut t = -pre;
    while t < 40 {
        let mut a = IDLE;
        if e.pending == 0 {
            if t < 0 {
                a[0] = 1;
                a[3] = 1;
            } else {
                if t == 0 {
                    hp0 = e.m.s.p[0].health;
                    a[9] = way;
                    a[11] = k;
                }
                if t < plan.2 {
                    a[0] = plan.0;
                    a[3] = plan.1;
                }
            }
            t += 1;
        }
        e.step(&a);
        e.take_outcome(&mut fo, &mut fp);
    }
    e.m.s.p[0].health - hp0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seeds: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
    let carry: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let pre: i32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(15);
    let way: i32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(BAR_TURN);
    let ps = plans();
    let mut tally = vec![[0u32; THROW_PITCHES]; ps.len()];
    let mut reach = 0;
    for seed in 0..seeds {
        let mut any = false;
        let mut best = f32::MIN;
        for (pi, &plan) in ps.iter().enumerate() {
            for k in 0..THROW_PITCHES as i32 {
                let g = heal(seed, carry, pre, way, k, plan);
                best = best.max(g);
                if g >= 7.99 {
                    tally[pi][k as usize] += 1;
                    any = true;
                }
            }
        }
        reach += any as u32;
        println!("seed {seed:2}: best {best:.2}");
    }
    println!("{reach} of {seeds} seeds have a full heal");
    let keys = ["none", "W", "S"];
    for (pi, &(key, sprint, d)) in ps.iter().enumerate() {
        let ks: Vec<String> = (0..THROW_PITCHES).filter(|&k| tally[pi][k] * 2 >= seeds as u32).map(|k| format!("{}:{}", throw_pitch(k as i32), tally[pi][k])).collect();
        let total: u32 = tally[pi].iter().sum();
        if total > 0 {
            println!("{}{} for {d:2} ticks: {total} full heals; pitches full on half the seeds or more: {}", keys[key as usize], if sprint == 1 { "+sprint" } else { "" }, if ks.is_empty() { "none".into() } else { ks.join(" ") });
        }
    }
}
