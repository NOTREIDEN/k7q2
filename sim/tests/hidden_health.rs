//! Servers that hide the opponent's health (`Server::health_shown`,
//! `Params::opp_hp_hide_prob`), the guess of it (`Agent::guess`,
//! `Params::opp_hp_guess`) and the guard that acts only there
//! (`SafetyCfg::hidden`, `Params::pot_safety_hidden`).
use pvpsim::agent::{ACT_HEADS, OBS_DIM};
use pvpsim::client::{ClientCfg, Input};
use pvpsim::env::{Env, Mode, Params};
use pvpsim::server::{KbProfile, ServerCfg};
use pvpsim::sim::{Match, NetCfg, Spawn};
use pvpsim::world::World;

/// Two players 2.5 blocks apart, facing each other, with real health.
fn duel(seed: u64) -> Match {
    let net = NetCfg { up_us: [10_000; 2], down_us: [10_000; 2], jitter_us: [0; 2] };
    let mut m = Match::new(
        World::arena(0.0, 200.0, true),
        [Spawn { pos: [0.0, 0.0, 0.0], yaw: 0.0, pitch: 0.0 }, Spawn { pos: [0.0, 0.0, 2.5], yaw: 180.0, pitch: 0.0 }],
        -1,
        KbProfile::VANILLA,
        ServerCfg { infinite_health: false, ..ServerCfg::default() },
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

#[test]
fn a_hidden_health_is_sent_as_the_set_value_and_only_to_that_player() {
    let mut m = duel(1);
    m.s.health_shown[0] = Some(1.0);
    m.c[0].rp.health = 1.0;
    let idle = Input::default();
    step(&mut m, &Input { attack: true, ..Default::default() }, &idle);
    for _ in 0..6 {
        step(&mut m, &idle, &idle);
    }
    let real = m.s.p[1].health;
    assert!(real < 20.0, "the hit cost nothing");
    assert_eq!(m.c[0].rp.health, 1.0);
    assert_eq!(m.c[1].lp.health, real);
    // The other way round nothing is hidden: player 1 hits player 0.
    let mut m = duel(2);
    m.s.health_shown[0] = Some(1.0);
    m.c[0].rp.health = 1.0;
    step(&mut m, &idle, &Input { attack: true, ..Default::default() });
    for _ in 0..6 {
        step(&mut m, &idle, &idle);
    }
    assert!(m.s.p[0].health < 20.0);
    assert_eq!(m.c[1].rp.health, m.s.p[0].health);
    assert_eq!(m.c[0].rp.health, 1.0);
}

#[test]
fn hiding_is_off_by_default_and_drawn_per_duel() {
    let e = Env::new(Mode::Bot, Params::default(), 3);
    assert_eq!(e.m.s.health_shown, [None, None]);
    let mut p = Params::default();
    assert!(p.set(340, 1.0));
    let e = Env::new(Mode::Bot, p.clone(), 3);
    assert_eq!(e.m.s.health_shown, [Some(1.0), None], "a bot still sees the agent's health");
    assert_eq!(e.m.c[0].rp.health, 1.0);
    let e = Env::new(Mode::SelfPlay, p.clone(), 3);
    assert_eq!(e.m.s.health_shown, [Some(1.0), Some(1.0)]);
    let mut first = p.clone();
    assert!(first.set(358, 1.0));
    let e = Env::new(Mode::SelfPlay, first, 3);
    assert_eq!(e.m.s.health_shown, [Some(1.0), None], "against an earlier version only the learner's is hidden");
    assert!(p.set(341, 20.0));
    let e = Env::new(Mode::SelfPlay, p, 3);
    assert_eq!(e.m.s.health_shown, [Some(20.0), Some(20.0)]);
}

/// What the first player is shown of the opponent's health after a few idle ticks,
/// and whether its agent finds the health hidden; the health its client has.
fn shown(params: &[(i32, f64)]) -> (f32, bool, f32) {
    let mut p = Params::default();
    for &(k, v) in params {
        assert!(p.set(k, v));
    }
    assert!(p.set(67, 1.0), "health_prob: hits cost health");
    let mut e = Env::new(Mode::Bot, p, 11);
    for _ in 0..5 {
        e.step(&[0; ACT_HEADS]);
    }
    let mut o = vec![0.0f32; OBS_DIM];
    e.write_obs(0, &mut o);
    let sent = e.m.c[0].rp.health;
    (o[98] * 20.0, e.agent_mut(0).hidden(), sent)
}

#[test]
fn the_guess_replaces_a_hidden_health_and_nothing_else() {
    // Hidden, no guess: the policy sees what the server sends.
    assert_eq!(shown(&[(340, 1.0)]), (1.0, false, 1.0));
    // Hidden with the guess: a full opponent at the start.
    assert_eq!(shown(&[(340, 1.0), (351, 1.0)]), (20.0, true, 1.0));
    assert_eq!(shown(&[(340, 1.0), (351, 2.0)]), (20.0, true, 1.0));
    // Not hidden, guess on auto: never shown, the health as sent.
    let (h, hidden, sent) = shown(&[(351, 2.0)]);
    assert!(!hidden);
    assert_eq!(h, sent);
    assert_eq!(shown(&[]).0, h);
    // The guard that waits for a hidden health finds it alone.
    assert_eq!(shown(&[(340, 1.0), (342, 1.0), (355, 1.0)]), (1.0, true, 1.0));
    assert_eq!(shown(&[(342, 1.0), (355, 1.0)]).1, false);
}

#[test]
fn a_value_shown_in_place_of_the_opponents_health() {
    // Once the health is found hidden (auto), or on every server.
    assert_eq!(shown(&[(340, 1.0), (356, 1.0), (357, 20.0)]), (20.0, true, 1.0));
    let (h, hidden, sent) = shown(&[(356, 1.0), (357, 20.0)]);
    assert_eq!((h, hidden), (sent, false));
    assert_eq!(shown(&[(356, 2.0), (357, 1.0)]).0, 1.0);
    // Its own health.
    let mut p = Params::default();
    for (k, v) in [(340, 1.0), (356, 2.0), (357, -1.0), (67, 1.0)] {
        assert!(p.set(k, v));
    }
    let mut e = Env::new(Mode::Bot, p, 11);
    e.step(&[0; ACT_HEADS]);
    let mut o = vec![0.0f32; OBS_DIM];
    e.write_obs(0, &mut o);
    assert_eq!(o[98] * 20.0, e.m.c[0].lp.health);
    assert_eq!(o[99], 0.0);
}
