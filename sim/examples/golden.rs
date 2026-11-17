//! A fingerprint of everything the trainer gets from the simulator, with
//! the settings the training machines use and random actions. It has to
//! stay the same through any change that is not meant to change what an
//! agent sees or is paid: `cargo run --release --example golden`.
use pvpsim::env::{Mode, VecEnv, ACT_DIMS, ACT_HEADS, OBS_DIM, PRIV_DIM, STATS_DIM};
use pvpsim::mth::Rng;

/// Parameter numbers of `Params::set` and the values of the training runs.
const SETTINGS: [(i32, f64); 23] = [
    (20, 0.7),   // kb_random
    (48, 0.5),   // opp_edge_prob
    (57, 1.0),   // w_damage
    (7, 16.0),   // start_dist_max
    (67, 0.8),   // health_prob
    (85, 0.5),   // speed_end_prob
    (90, 0.3),   // hunger_prob
    (95, 5.0),   // w_speed
    (96, 5.0),   // w_fed
    (97, 0.5),   // refill_prob
    (104, 2.0),  // w_stock
    (105, 0.5),  // lesson_prob
    (106, 31.0), // lessons
    (113, 0.4),  // extreme_share
    (107, 12.0), // lesson_dist_min
    (108, 28.0), // lesson_dist_max
    (110, 1.5),  // w_miss
    (111, 0.1),  // w_pot_time
    (42, 190.0), // agent_ping_min_ms
    (43, 210.0), // agent_ping_max_ms
    (112, 0.15), // bot_runner
    (37, 0.3),   // w_sprint_hit
    (39, 0.2),   // w_combo
];

fn mix(h: &mut u64, bits: u32) {
    for b in bits.to_le_bytes() {
        *h = (*h ^ b as u64).wrapping_mul(0x100000001b3);
    }
}

fn main() {
    for mode in [Mode::SelfPlay, Mode::Bot] {
        let n = 64;
        let mut v = VecEnv::new(n, mode, 7);
        for (k, x) in SETTINGS {
            assert!(v.set_param(k, x));
        }
        let mut obs = vec![0f32; n * OBS_DIM];
        let mut pv = vec![0f32; n * PRIV_DIM];
        let mut fobs = vec![0f32; n * OBS_DIM];
        let mut fpv = vec![0f32; n * PRIV_DIM];
        let mut rew = vec![0f32; n];
        let mut done = vec![0u8; n];
        let mut act = vec![0i32; n * ACT_HEADS];
        v.reset(&mut obs, &mut pv);
        let mut rng = Rng::new(11);
        let (mut h_obs, mut h_priv, mut h_rew) = (0xcbf29ce484222325u64, 0xcbf29ce484222325u64, 0xcbf29ce484222325u64);
        // Stretches of doing little, so that duels get somewhere, between
        // stretches of anything at all.
        for call in 0..6000 {
            let wild = (call / 200) % 2 == 1;
            for i in 0..n {
                // The controls the reference was made with; one added since stays at nothing.
                for k in 0..10 {
                    let r = rng.next_u64();
                    act[i * ACT_HEADS + k] = if wild || r % 8 == 0 { ((r >> 8) % ACT_DIMS[k] as u64) as i32 } else { [1, 0, 0, 1, 1, 13, 10, 0, 0, 0][k] };
                }
            }
            v.step(&act, &mut obs, &mut pv, &mut rew, &mut done, &mut fobs, &mut fpv);
            for x in &obs {
                mix(&mut h_obs, x.to_bits());
            }
            for x in pv.iter().chain(&fpv) {
                mix(&mut h_priv, x.to_bits());
            }
            for (i, x) in rew.iter().enumerate() {
                mix(&mut h_rew, x.to_bits());
                mix(&mut h_rew, done[i] as u32);
                if done[i] != 0 {
                    for x in &fobs[i * OBS_DIM..(i + 1) * OBS_DIM] {
                        mix(&mut h_obs, x.to_bits());
                    }
                }
            }
        }
        let mut s = [0f64; STATS_DIM];
        v.take_stats(&mut s);
        println!("{mode:?}: observations {h_obs:016x}, critic's {h_priv:016x}, rewards {h_rew:016x}; episodes {}, hits {} and {}, potions {} and {}, pearls {} and {}", s[0], s[3], s[4], s[40], s[41], s[24], s[25]);
    }
}
