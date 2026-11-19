//! Servers that ignore some sword blocks (`ServerCfg::block_fail`,
//! `Params::block_fail_prob`, `Params::block_fail`).
use pvpsim::client::{ClientCfg, Input};
use pvpsim::server::{KbProfile, ServerCfg};
use pvpsim::sim::{Match, NetCfg, Spawn};
use pvpsim::world::World;

/// Two players 2.5 blocks apart, facing each other, with real health.
fn duel(cfg: ServerCfg, seed: u64) -> Match {
    let net = NetCfg { up_us: [10_000; 2], down_us: [10_000; 2], jitter_us: [0; 2] };
    let mut m = Match::new(
        World::arena(0.0, 200.0, true),
        [Spawn { pos: [0.0, 0.0, 0.0], yaw: 0.0, pitch: 0.0 }, Spawn { pos: [0.0, 0.0, 2.5], yaw: 180.0, pitch: 0.0 }],
        -1,
        KbProfile::VANILLA,
        cfg,
        ClientCfg::with_sensitivity(0.5),
        net,
        [0, 17_000, 31_000],
        seed,
    );
    m.s.p[0].update_counter = 2;
    m.s.p[1].update_counter = 2;
    m
}

fn step(m: &mut Match, i0: &Input, i1: &Input) {
    for _ in 0..2 {
        let id = m.advance();
        m.client_tick(id, if id == 0 { i0 } else { i1 });
    }
}

/// Player 1 holds its sword up and player 0 hits it once: whether the
/// server had it blocking, and the health it lost.
fn blocked_hit(block_fail: f32, seed: u64) -> (bool, f32) {
    let mut m = duel(ServerCfg { infinite_health: false, sword_wear: 0.0, block_fail, ..ServerCfg::default() }, seed);
    let (idle, guard) = (Input::default(), Input { use_item: true, ..Default::default() });
    for _ in 0..6 {
        step(&mut m, &idle, &guard);
    }
    step(&mut m, &Input { attack: true, ..Default::default() }, &guard);
    for _ in 0..4 {
        step(&mut m, &idle, &guard);
    }
    assert_eq!(m.s.hits.len(), 1);
    (m.s.hits[0].blocked, 20.0 - m.s.p[1].health)
}

#[test]
fn a_raised_sword_halves_the_hit_by_default() {
    for seed in 0..8 {
        assert_eq!(blocked_hit(0.0, seed), (true, 4.5));
    }
}

#[test]
fn a_server_that_ignores_every_block_takes_the_full_hit() {
    for seed in 0..8 {
        assert_eq!(blocked_hit(1.0, seed), (true, 8.0));
    }
}

#[test]
fn a_server_that_ignores_some_blocks_cuts_the_rest() {
    let lost: Vec<f32> = (0..64).map(|s| blocked_hit(0.45, s).1).collect();
    let full = lost.iter().filter(|&&x| x == 8.0).count();
    assert_eq!(full + lost.iter().filter(|&&x| x == 4.5).count(), lost.len());
    assert!((12..=46).contains(&full), "{full} of 64");
}

#[test]
fn block_fail_is_drawn_per_duel_and_off_by_default() {
    use pvpsim::env::{Env, Mode, Params};
    let e = Env::new(Mode::SelfPlay, Params::default(), 3);
    assert_eq!(e.m.s.cfg.block_fail, 0.0);
    let mut p = Params::default();
    assert!(p.set(275, 1.0) && p.set(276, 0.4));
    let e = Env::new(Mode::SelfPlay, p, 3);
    assert!((e.m.s.cfg.block_fail - 0.4).abs() < 1e-6);
}
