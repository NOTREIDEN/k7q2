//! Potions thrown with a turn, nobody near: where `run_feet` said the
//! thrower would be, where it was, and where the potion broke.
//! `cargo run --release --example turn_check -- [SEEDS] [WAY]`
use pvpsim::agent::{run_feet, ACT_HEADS, BAR_TURN, BAR_TURNS, OBS_DIM};
use pvpsim::aabb::V3;
use pvpsim::env::{Env, Mode, Params, PRIV_DIM};

const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0];

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

fn d3(a: V3, b: V3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seeds: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(40);
    let way: i32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(BAR_TURN);
    let (mut fo, mut fp) = ([0.0f32; OBS_DIM], [0.0f32; PRIV_DIM]);
    let mut eights = 0;
    for seed in 0..seeds {
        let mut e = Env::new(Mode::SelfPlay, params(), seed);
        let hp0 = e.m.s.p[0].health;
        let yaw = e.m.c[0].lp.l.yaw as f64 + BAR_TURNS[(way - BAR_TURN) as usize] as f64 * 0.15;
        let pred = run_feet(&e.m.c[0], &e.m.world, yaw as f32);
        let mut srv: Vec<V3> = Vec::new();
        let mut cli: Vec<V3> = Vec::new();
        let mut pot: Vec<(V3, i32)> = Vec::new();
        let mut k = 0;
        let mut last: Option<(V3, i32)> = None;
        let mut broke: Option<(V3, i32, usize)> = None;
        while k < 40 {
            let a = if e.pending == 0 {
                let mut a = IDLE;
                if k == 0 {
                    a[9] = way;
                }
                k += 1;
                a
            } else {
                IDLE
            };
            let was0 = e.pending == 0;
            e.step(&a);
            e.take_outcome(&mut fo, &mut fp);
            if was0 {
                cli.push(e.m.c[0].lp.l.body.pos);
            }
            srv.push(e.m.s.p[0].l.body.pos);
            let now = e.m.s.thrown.iter().find(|t| t.thrower == 0).map(|t| (t.pos, t.ticks_in_air));
            if let Some(p) = now {
                pot.push(p);
            }
            if broke.is_none() && last.is_some() && now.is_none() {
                let (at, t) = last.unwrap();
                broke = Some((at, t, srv.len() - 1));
            }
            last = now;
        }
        let gain = e.m.s.p[0].health - hp0;
        eights += (gain >= 7.99) as i32;
        let Some((at, t, i)) = broke else {
            println!("seed {seed}: no break, gain {gain}");
            continue;
        };
        // The server's feet at the impact, and which predicted tick is nearest.
        let feet = srv[i];
        let (best, bd) = pred.iter().enumerate().map(|(q, p)| (q, d3(*p, feet))).fold((0, 1e9), |a, b| if b.1 < a.1 { b } else { a });
        println!(
            "seed {seed:2}: gain {gain:.0}, potion flew {t} ticks, broke {:.2} from the feet (dy {:.2}); feet nearest predicted tick {best} ({bd:.3} off); client ticks {}",
            d3(at, feet),
            at[1] - feet[1],
            cli.len()
        );
    }
    println!("{eights} of {seeds} healed 8");
}
