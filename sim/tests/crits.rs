//! Packet Criticals of the bot's client (`Params::crit_prob`): off by
//! default, and with it every hit of the bot is a critical one.
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};

const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];

fn run(crit_prob: f64, seed: u64) -> (f64, f64, bool) {
    let mut p = Params::default();
    p.pearl_prob = 0.0;
    p.start_dist_min = 3.0;
    p.start_dist_max = 3.0;
    p.bot_level = [4.0, 4.0];
    p.ep_ticks_min = 2000.0;
    p.ep_ticks_max = 2000.0;
    p.crit_prob = crit_prob;
    let mut e = Env::new(Mode::Bot, p, seed);
    let (mut fobs, mut fpriv) = (vec![0f32; OBS_DIM], vec![0f32; PRIV_DIM]);
    for _ in 0..600 {
        e.step(&IDLE);
        e.take_outcome(&mut fobs, &mut fpriv);
    }
    let st = *e.episode();
    (st.hits[1], st.crits[1], e.m.s.p[1].always_crit)
}

#[test]
fn off_by_default() {
    assert_eq!(Params::default().crit_prob, 0.0);
    for seed in 1..4 {
        let (hits, crits, on) = run(0.0, seed);
        assert!(!on);
        assert!(crits <= 0.3 * hits.max(1.0), "{crits} of {hits}");
    }
}

#[test]
fn every_hit_of_the_bot_is_critical() {
    for seed in 1..4 {
        let (hits, crits, on) = run(1.0, seed);
        assert!(on);
        assert!(hits >= 10.0, "the bot should keep hitting: {hits}");
        assert_eq!(crits, hits);
    }
}
