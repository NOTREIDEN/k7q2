//! Random play over many episodes: speed, and that nothing breaks.
use chestsim::env::{VecEnv, ACT_DIMS, ACT_HEADS, OBS_DIM, PRIV_DIM, STATS_DIM, STAT_NAMES};
use chestsim::mth::Rng;
use std::time::Instant;

fn main() {
    let n = 64;
    let mut v = VecEnv::new(n, 7);
    let mut obs = vec![0f32; n * OBS_DIM];
    let mut pv = vec![0f32; n * PRIV_DIM];
    let mut fo = vec![0f32; n * OBS_DIM];
    let mut fp = vec![0f32; n * PRIV_DIM];
    let mut rew = vec![0f32; n];
    let mut done = vec![0u8; n];
    v.reset(&mut obs, &mut pv);
    let mut rng = Rng::new(1);
    let mut acts = vec![0i32; n * ACT_HEADS];
    let steps = 3000;
    let t0 = Instant::now();
    let mut rsum = 0.0f64;
    for _ in 0..steps {
        for e in 0..n {
            for h in 0..ACT_HEADS {
                acts[e * ACT_HEADS + h] = (rng.f64() * ACT_DIMS[h] as f64) as i32;
            }
            // Mostly forward, mostly sprinting: covers ground.
            acts[e * ACT_HEADS] = if rng.chance(0.7) { 2 } else { acts[e * ACT_HEADS] };
        }
        v.step(&acts, &mut obs, &mut pv, &mut rew, &mut done, &mut fo, &mut fp);
        rsum += rew.iter().map(|&r| r as f64).sum::<f64>();
        assert!(obs.iter().all(|x| x.is_finite()), "non-finite observation");
    }
    let dt = t0.elapsed().as_secs_f64();
    println!("{} steps x {} envs in {:.2}s: {:.0} steps/s, obs {} priv {}", steps, n, dt, (steps * n) as f64 / dt, OBS_DIM, PRIV_DIM);
    let mut s = vec![0f64; STATS_DIM];
    v.take_stats(&mut s);
    for (k, x) in STAT_NAMES.iter().zip(s.iter()) {
        print!("{}={:.0} ", k, x);
    }
    println!("\nreward/step {:.4}", rsum / (steps * n) as f64);
}

#[allow(dead_code)]
fn unused() {}
