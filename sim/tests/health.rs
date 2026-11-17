//! Health, armour, Protection, hunger and regeneration, checked against
//! numbers that follow from the 1.8.9 source. The bit-exact comparison with
//! the running game is in the recordings (`tests/parity.rs`, scenes `hp*`).
use pvpsim::client::{ClientCfg, Input};
use pvpsim::mth::{round_f, JavaRandom};
use pvpsim::server::{feather_points, prot_points, KbProfile, ServerCfg};
use pvpsim::sim::{Match, NetCfg, Spawn};
use pvpsim::world::World;

/// Two players 2.5 blocks apart, facing each other, with real health.
fn duel(cfg: ServerCfg) -> Match {
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
        7,
    );
    m.s.p[0].update_counter = 2;
    m.s.p[1].update_counter = 2;
    m
}

fn real() -> ServerCfg {
    ServerCfg { infinite_health: false, sword_wear: 0.0, ..ServerCfg::default() }
}

fn step(m: &mut Match, i0: &Input, i1: &Input) {
    for _ in 0..2 {
        let id = m.advance();
        m.client_tick(id, if id == 0 { i0 } else { i1 });
    }
}

/// Player 0 hits once; returns the health player 1 lost.
fn one_hit(cfg: ServerCfg) -> f32 {
    let mut m = duel(cfg);
    let idle = Input::default();
    for _ in 0..4 {
        step(&mut m, &idle, &idle);
    }
    step(&mut m, &Input { attack: true, ..Default::default() }, &idle);
    for _ in 0..4 {
        step(&mut m, &idle, &idle);
    }
    assert_eq!(m.s.hits.len(), 1);
    assert_eq!(m.s.hits[0].dealt, 20.0 - m.s.p[1].health);
    20.0 - m.s.p[1].health
}

#[test]
fn the_generator_is_javas() {
    let mut r = JavaRandom::new(42);
    let got: Vec<i32> = [10, 5, 1, 13, 11, 8, 3, 1_000_000_007, 2].iter().map(|b| r.next_int(*b)).collect();
    assert_eq!(got, [0, 3, 0, 12, 3, 7, 2, 519_796_911, 1]);
    assert_eq!((round_f(2.5), round_f(-2.5), round_f(0.49999997)), (3, -2, 0));
}

#[test]
fn enchantment_points() {
    assert_eq!([1, 2, 3, 4].map(prot_points), [1, 2, 3, 5]);
    assert_eq!([1, 2, 3, 4].map(feather_points), [5, 8, 12, 18]);
}

#[test]
fn a_bare_player_takes_the_swords_full_damage() {
    assert_eq!(one_hit(real()), 8.0);
    // Sharpness adds 1.25 a level.
    assert_eq!(one_hit(ServerCfg { ench_damage: 5.0 * 1.25, ..real() }), 14.25);
}

#[test]
fn diamond_armour_takes_four_fifths() {
    let lost = one_hit(ServerCfg { armor: 20, ..real() });
    assert!((lost - 1.6).abs() < 1e-5, "{lost}");
}

#[test]
fn protection_takes_a_share_that_is_drawn_for_every_hit() {
    // Four pieces of Protection IV add up to 20: between 10 and 20
    // twenty-fifths are taken off what the armour let through.
    let mut seen = Vec::new();
    for seed in 0..40u64 {
        let mut m = duel(ServerCfg { armor: 20, prot: 4 * prot_points(4), ..real() });
        m.rng = pvpsim::mth::Rng::new(seed);
        let idle = Input::default();
        for _ in 0..4 {
            step(&mut m, &idle, &idle);
        }
        step(&mut m, &Input { attack: true, ..Default::default() }, &idle);
        for _ in 0..4 {
            step(&mut m, &idle, &idle);
        }
        let lost = 20.0 - m.s.p[1].health;
        let k = 25.0 - lost / 1.6 * 25.0;
        assert!((k - k.round()).abs() < 1e-3 && (10.0..=20.0).contains(&k.round()), "{lost} {k}");
        seen.push(k.round() as i32);
    }
    seen.sort();
    seen.dedup();
    assert!(seen.len() >= 6, "{seen:?}");
}

#[test]
fn infinite_health_is_untouched_by_all_of_it() {
    let mut cfg = ServerCfg { armor: 20, prot: 8, ..ServerCfg::default() };
    cfg.sword_wear = 0.0;
    let mut m = duel(cfg);
    let idle = Input::default();
    let mut told = 0;
    for t in 0..200 {
        let hit = Input { attack: t % 12 == 0, forward: 1, jump: t % 3 == 0, sprint_key: true, ..Default::default() };
        for _ in 0..2 {
            let id = m.advance();
            m.client_tick(id, if id == 0 { &hit } else { &idle });
        }
        told += m.c.iter().filter(|c| c.lp.health != 20.0 || c.lp.food != 20).count();
    }
    assert!(!m.s.hits.is_empty());
    assert_eq!((m.s.p[0].health, m.s.p[1].health, told), (20.0, 20.0, 0));
    assert_eq!(m.s.p[0].food.exhaustion, 0.0);
}

#[test]
fn both_clients_are_told_the_health() {
    let mut m = duel(real());
    let idle = Input::default();
    step(&mut m, &Input { attack: true, ..Default::default() }, &idle);
    for _ in 0..8 {
        step(&mut m, &idle, &idle);
    }
    assert_eq!(m.s.p[1].health, 12.0);
    assert_eq!(m.c[1].lp.health, 12.0, "the victim");
    assert_eq!(m.c[0].rp.health, 12.0, "the attacker sees it on the other player");
    assert_eq!(m.c[0].lp.health, 20.0);
}

#[test]
fn a_player_without_health_is_out_of_the_fight() {
    let mut m = duel(ServerCfg { ench_damage: 12.0, ..real() });
    let idle = Input::default();
    step(&mut m, &Input { attack: true, ..Default::default() }, &idle);
    for _ in 0..3 {
        step(&mut m, &idle, &idle);
    }
    assert_eq!(m.s.p[1].health, 0.0);
    assert!(!m.s.p[1].alive());
    let n = m.s.hits.len();
    for _ in 0..30 {
        step(&mut m, &Input { attack: true, ..Default::default() }, &idle);
    }
    assert_eq!(m.s.hits.len(), n, "a dead player cannot be hit");
}

#[test]
fn a_fed_player_gets_half_a_heart_back_every_four_seconds() {
    let mut m = duel(real());
    let idle = Input::default();
    step(&mut m, &Input { attack: true, ..Default::default() }, &idle);
    for _ in 0..4 {
        step(&mut m, &idle, &idle);
    }
    let h0 = m.s.p[1].health;
    assert_eq!(h0, 12.0);
    for _ in 0..80 {
        step(&mut m, &idle, &idle);
    }
    assert_eq!(m.s.p[1].health, 13.0);
    for _ in 0..80 {
        step(&mut m, &idle, &idle);
    }
    assert_eq!(m.s.p[1].health, 14.0);
    // Each point costs three of exhaustion. The hit cost 0.3, and being
    // knocked off the ground counts as a jump on the server: 0.2 more, and
    // a little for the slide. Four of it went into a point of saturation.
    let f = m.s.p[1].food;
    assert!((2.5..2.51).contains(&f.exhaustion) && f.saturation == 4.0, "{f:?}");
    // Without the game rule nothing comes back.
    let mut m = duel(ServerCfg { natural_regen: false, ..real() });
    step(&mut m, &Input { attack: true, ..Default::default() }, &idle);
    for _ in 0..200 {
        step(&mut m, &idle, &idle);
    }
    assert_eq!(m.s.p[1].health, 12.0);
}

#[test]
fn running_and_jumping_make_hungry_and_a_hungry_player_cannot_sprint() {
    let mut m = duel(real());
    // Far apart, so that nobody is in the way.
    m.s.p[1].l.body.set_position([100.0, 0.0, 100.0]);
    m.c[1].lp.l.body.set_position([100.0, 0.0, 100.0]);
    let idle = Input::default();
    let run = Input { forward: 1, sprint_key: true, jump: true, ..Default::default() };
    let mut ticks = 0;
    while m.s.p[0].food.level > 6 {
        step(&mut m, &run, &idle);
        ticks += 1;
        assert!(ticks < 20 * 600, "never got hungry");
    }
    // Saturation goes first, then the food: 5 + 14 points of four each, at
    // 0.8 for every sprint jump and a tenth for every block run.
    assert_eq!(m.s.p[0].food.saturation, 0.0);
    assert!((1500..4000).contains(&ticks), "{ticks}");
    for _ in 0..10 {
        step(&mut m, &run, &idle);
    }
    assert_eq!(m.c[0].lp.food, 6);
    assert!(!m.c[0].lp.l.sprinting, "no sprinting on three drumsticks");
    assert!(!m.s.p[0].l.sprinting);
    // No regeneration below nine drumsticks either.
    m.s.p[0].set_health(10.0);
    for _ in 0..100 {
        step(&mut m, &idle, &idle);
    }
    assert_eq!(m.s.p[0].health, 10.0);
}

#[test]
fn hunger_can_be_switched_off() {
    let mut m = duel(ServerCfg { hunger: false, ..real() });
    let idle = Input::default();
    let run = Input { forward: 1, sprint_key: true, jump: true, attack: true, ..Default::default() };
    for _ in 0..400 {
        step(&mut m, &run, &idle);
    }
    assert_eq!(m.s.p[0].food, pvpsim::server::Food::FULL);
}
