//! Survival drills (`Params::drill_prob`): an opponent that cannot die and
//! takes no knockback, and players the first one cannot see that come at
//! it in waves, run after it and hit it while they are in reach.
use pvpsim::env::{Env, Mode, Params, ACT_HEADS};

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0];

fn params() -> Params {
    let mut p = Params::default();
    p.infinite_health = false;
    p.drill_prob = 1.0;
    p.drill_strength_max = 2.0;
    p.potions = [0.0, 0.0];
    p.pearl_prob = 0.0;
    p.start_dist_min = 12.0;
    p.start_dist_max = 12.0;
    p.ep_ticks_min = 4000.0;
    p.ep_ticks_max = 4000.0;
    p
}

/// Player 0 makes `n` ticks with `f`'s actions; the bot plays its own.
fn run(e: &mut Env, n: usize, mut f: impl FnMut(&Env) -> [i32; ACT_HEADS]) {
    for _ in 0..n {
        let a = f(e);
        e.step(&a);
    }
}

#[test]
fn a_drill_opponent_cannot_die_nor_be_knocked_and_the_ganks_come_in_waves() {
    let mut e = Env::new(Mode::Bot, params(), 3);
    assert!(e.m.s.p[1].immortal && e.m.s.p[1].no_kb);
    assert!(!e.m.s.p[0].immortal && !e.m.s.p[0].no_kb);
    // Standing still, the first wave finds it within a few seconds.
    let mut lost = 0.0;
    let mut first = None;
    for t in 0..200 {
        let h = e.m.s.p[0].health;
        run(&mut e, 1, |_| IDLE);
        let d = h - e.m.s.p[0].health;
        if d > 0.0 {
            lost += d;
            first.get_or_insert(t);
        }
        if !e.m.s.p[0].alive() {
            break;
        }
    }
    assert!(first.map_or(false, |t| t < 60 + 30), "{first:?}");
    assert!(lost >= 4.0, "{lost}");
    assert_eq!(e.m.s.p[1].health, 20.0);
}

#[test]
fn not_in_self_play_unless_asked_and_never_with_infinite_health() {
    let e = Env::new(Mode::SelfPlay, params(), 3);
    assert!(!e.m.s.p[1].immortal);
    let mut p = params();
    p.drill_self = true;
    let e = Env::new(Mode::SelfPlay, p, 3);
    assert!(e.m.s.p[1].immortal);
    let mut p = params();
    p.infinite_health = true;
    let e = Env::new(Mode::Bot, p, 3);
    assert!(!e.m.s.p[1].immortal);
}

#[test]
fn a_drill_lost_and_played_again_is_still_a_drill() {
    let mut p = params();
    p.retry_prob = 1.0;
    let mut e = Env::new(Mode::Bot, p, 5);
    // Standing still with no potions it dies, again and again; the retries
    // of moments before its deaths are drills as well.
    for _ in 0..40_000 {
        run(&mut e, 1, |_| IDLE);
        if e.total.retries >= 2.0 {
            break;
        }
    }
    let t = &e.total;
    assert!(t.retries >= 2.0 && t.episodes >= 3.0, "{} {}", t.retries, t.episodes);
    assert_eq!(t.drills, t.episodes);
    assert_eq!(t.drill_deaths, t.deaths[0]);
    assert!(t.deaths[0] >= 2.0 && t.deaths[1] == 0.0);
}
