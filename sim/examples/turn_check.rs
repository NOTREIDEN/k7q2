//! Potions thrown with a turn, nobody near: for each pitch the last control
//! can ask for, the health the potion gives with the player then doing one
//! of a few simple things, to see that a pitch that heals all eight is
//! there to be learnt.
//! `cargo run --release --example turn_check -- [SEEDS] [WAY] [CARRY] [AFTER]`,
//! CARRY the share of duels on a server that adds the thrower's movement,
//! AFTER what the player does from the click on: 0 stands, 1 runs on along
//! the turned view with W and sprint, 2 turns back at once and walks back
//! with S.
use pvpsim::agent::{throw_pitch, ACT_HEADS, BAR_TURN, OBS_DIM, THROW_PITCHES};
use pvpsim::env::{Env, Mode, Params, PRIV_DIM};

const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];

fn params() -> Params {
    let mut p = Params::default();
    p.infinite_health = false;
    p.potions = [7.0, 7.0];
    p.start_hurt = 1.0;
    p.start_health = [6.0, 6.0];
    p.pearl_prob = 0.0;
    p.start_dist_min = 12.0;
    p.start_dist_max = 12.0;
    p.ep_ticks_min = 100_000.0;
    p.ep_ticks_max = 100_000.0;
    p
}

/// The health one potion thrown at pitch choice `k` gives.
fn heal(seed: u64, way: i32, carry: f64, after: i32, k: i32) -> f32 {
    let mut p = params();
    p.throw_carry_prob = carry;
    let mut e = Env::new(Mode::SelfPlay, p, seed);
    let (mut fo, mut fp) = ([0.0f32; OBS_DIM], [0.0f32; PRIV_DIM]);
    let hp0 = e.m.s.p[0].health;
    let mut t = 0;
    while t < 40 {
        let mut a = IDLE;
        if e.pending == 0 {
            if t == 0 {
                a[9] = way;
                a[11] = k;
                // Running from the turn on.
                a[0] = 1;
                a[3] = 1;
            } else {
                match after {
                    1 => {
                        a[0] = 1;
                        a[3] = 1;
                    }
                    2 => {
                        if t == 2 {
                            a[10] = 5;
                        }
                        a[0] = 2;
                    }
                    _ => {}
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
    let seeds: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(20);
    let way: i32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(BAR_TURN);
    let carry: f64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let after: i32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut reach = 0;
    let mut tally = [0u32; THROW_PITCHES];
    for seed in 0..seeds {
        let gains: Vec<f32> = (0..THROW_PITCHES as i32).map(|k| heal(seed, way, carry, after, k)).collect();
        let best = gains.iter().cloned().fold(f32::MIN, f32::max);
        reach += (best >= 7.99) as i32;
        let full: Vec<String> = (0..THROW_PITCHES).filter(|&k| gains[k] >= 7.99).map(|k| {
            tally[k] += 1;
            format!("{}", throw_pitch(k as i32))
        }).collect();
        println!("seed {seed:2}: best {best:.0}; all eight at {}", if full.is_empty() { "none".to_string() } else { full.join(" ") });
    }
    let common: Vec<String> = (0..THROW_PITCHES).filter(|&k| tally[k] > 0).map(|k| format!("{}:{}", throw_pitch(k as i32), tally[k])).collect();
    println!("{reach} of {seeds} have a pitch that heals 8; pitch:seeds {}", common.join(" "));
}
