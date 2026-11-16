//! Throughput check with random actions, and a look at what the scripted
//! bots do against each other. Run with `cargo run --release --example bench`.
use pvpsim::bot::{self, BotParams, BotState};
use pvpsim::client::ClientCfg;
use pvpsim::env::{Mode, VecEnv, ACT_DIMS, ACT_HEADS, OBS_DIM, PRIV_DIM, STATS_DIM};
use pvpsim::mth::Rng;
use pvpsim::server::{KbProfile, ServerCfg};
use pvpsim::sim::{Match, NetCfg, Spawn};
use pvpsim::world::World;
use std::time::Instant;

fn bots(l0: u8, l1: u8, fix: bool, seed: u64) -> ([u32; 2], [u32; 2], [u32; 2]) {
    let net = NetCfg { up_us: [25_000; 2], down_us: [25_000; 2], jitter_us: [1000; 2] };
    let mut ccfg = ClientCfg::with_sensitivity(0.5);
    ccfg.mouse_delay_fix = fix;
    let mut m = Match::new(
        World::arena(4.0, 24.0, true),
        [Spawn { pos: [0.0, 4.0, -4.0], yaw: 0.0, pitch: 0.0 }, Spawn { pos: [0.0, 4.0, 4.0], yaw: 180.0, pitch: 0.0 }],
        1,
        KbProfile::VANILLA,
        ServerCfg::default(),
        ccfg,
        net,
        [0, 21_000, 37_000],
        seed,
    );
    let p = [BotParams::level(l0), BotParams::level(l1)];
    let mut st = [BotState::default(), BotState::default()];
    let mut rng = Rng::new(seed + 99);
    let (mut hits, mut sprint, mut combo, mut best) = ([0u32; 2], [0u32; 2], [0u32; 2], [0u32; 2]);
    for _ in 0..(20 * 60 * 2) {
        let id = m.advance();
        for h in m.s.hits.drain(..) {
            hits[h.attacker] += 1;
            if h.sprint {
                sprint[h.attacker] += 1;
            }
            combo[h.attacker] += 1;
            combo[h.victim] = 0;
            best[h.attacker] = best[h.attacker].max(combo[h.attacker]);
        }
        let inp = bot::act(&m.c[id], &m.world, &m.ccfg, &p[id], &mut st[id], &mut rng);
        m.client_tick(id, &inp);
    }
    (hits, sprint, best)
}

fn main() {
    for mode in [Mode::SelfPlay, Mode::Bot] {
        let n = 256;
        let mut v = VecEnv::new(n, mode, 1);
        let mut obs = vec![0f32; n * OBS_DIM];
        let mut pv = vec![0f32; n * PRIV_DIM];
        let mut fobs = vec![0f32; n * OBS_DIM];
        let mut fpv = vec![0f32; n * PRIV_DIM];
        let mut rew = vec![0f32; n];
        let mut done = vec![0u8; n];
        let mut act = vec![0i32; n * ACT_HEADS];
        v.reset(&mut obs, &mut pv);
        let mut rng = Rng::new(5);
        let calls = 4000;
        let t = Instant::now();
        let mut rsum = 0.0f64;
        for _ in 0..calls {
            for i in 0..n {
                for h in 0..ACT_HEADS {
                    act[i * ACT_HEADS + h] = (rng.next_u64() % ACT_DIMS[h] as u64) as i32;
                }
            }
            v.step(&act, &mut obs, &mut pv, &mut rew, &mut done, &mut fobs, &mut fpv);
            rsum += rew.iter().map(|x| *x as f64).sum::<f64>();
        }
        let dt = t.elapsed().as_secs_f64();
        let mut s = [0f64; STATS_DIM];
        v.take_stats(&mut s);
        assert!(obs.iter().all(|x| x.is_finite()) && pv.iter().all(|x| x.is_finite()));
        println!(
            "{mode:?}: {:.0} agent-steps/s on one core, episodes {}, hits by agent0 {} by agent1 {}, mean reward/step {:.4}",
            (calls * n) as f64 / dt,
            s[0],
            s[3],
            s[4],
            rsum / (calls * n) as f64
        );
    }
    println!("scripted bots, 60 s duels (hits, sprint hits, best combo):");
    for (a, b) in [(2, 2), (3, 2), (4, 2), (4, 3), (4, 4), (3, 3)] {
        for fix in [true, false] {
            let (h, s, c) = bots(a, b, fix, 11);
            println!("  level {a} vs {b}, mouse fix {fix}: hits {h:?} sprint {s:?} combo {c:?}");
        }
    }
}
