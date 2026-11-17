//! A crosshair that shakes costs; a potion that is not thrown perfectly
//! gives little in the duels that are a lesson in it; and the lessons with
//! nobody moved by a hit and with the first player in a corner.
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM, SHAKE_DEG};

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0];
/// Five mouse counts to the left and to the right, three quarters of a degree.
const LEFT: i32 = 9;
const RIGHT: i32 = 17;
/// The pitch choice that looks down the fastest of the first set.
const DOWN: i32 = 26;
/// A quarter turn up and down in one tick.
const FLICK_UP: i32 = 27;
const FLICK_DOWN: i32 = 28;

fn params() -> Params {
    let mut p = Params::default();
    p.infinite_health = false;
    p.prot = [2.0, 2.0];
    p.sharp = [2.0, 2.0];
    p.potions = [7.0, 7.0];
    p.start_hurt = 1.0;
    p.start_health = [6.0, 6.0];
    p.pearl_prob = 0.0;
    p.start_dist_min = 12.0;
    p.start_dist_max = 12.0;
    p.start_yaw_noise = 0.0;
    p.ep_ticks_min = 100_000.0;
    p.ep_ticks_max = 100_000.0;
    p.kb_random = 0.0;
    p.opp_edge_prob = 0.0;
    p
}

/// Nothing is paid but for the shaking.
fn bare(w: f32) -> Params {
    let mut p = params();
    p.w_aim = 0.0;
    p.w_range = 0.0;
    p.w_ontarget = 0.0;
    p.w_shake = w;
    p
}

/// Let player 0 make `n` ticks with the actions `f` gives for each of them,
/// while player 1 stands still. Returns what player 0 was paid.
fn run(e: &mut Env, n: usize, mut f: impl FnMut(usize) -> [i32; ACT_HEADS]) -> f32 {
    let (mut fo, mut fp) = ([0.0f32; OBS_DIM], [0.0f32; PRIV_DIM]);
    let mut paid = 0.0;
    let mut k = 0;
    while k < n {
        if e.pending == 0 {
            let a = f(k);
            e.step(&a);
            k += 1;
        } else {
            e.step(&IDLE);
        }
        if e.pending == 0 {
            paid += e.take_outcome(&mut fo, &mut fp).0;
        } else if e.mode == Mode::SelfPlay {
            e.take_outcome(&mut fo, &mut fp);
        }
    }
    paid
}

fn with(head: usize, v: i32) -> [i32; ACT_HEADS] {
    let mut a = IDLE;
    a[head] = v;
    a
}

#[test]
fn a_crosshair_that_goes_back_and_forth_pays_for_what_it_undoes() {
    let w = 0.12f32;
    // Left, right, left ...: every turn but the first takes the one before
    // it back, three quarters of a degree each. (The view turns between
    // two ticks, so the last turn is seen a tick later.)
    let swing = |k: usize| if k >= 10 { 13 } else if k % 2 == 0 { LEFT } else { RIGHT };
    let mut e = Env::new(Mode::SelfPlay, bare(w), 1);
    let paid = run(&mut e, 12, |k| with(5, swing(k)));
    let one = w * 0.75 / SHAKE_DEG;
    assert!((paid + 9.0 * one).abs() < 1e-4, "paid {paid}, one is {one}");
    assert!((e.episode().shake[0] - 9.0 * 0.75).abs() < 1e-3, "{}", e.episode().shake[0]);
    // Up and down counts on its own.
    let mut e = Env::new(Mode::SelfPlay, bare(w), 1);
    let paid = run(&mut e, 12, |k| {
        let mut a = with(5, swing(k));
        a[6] = if k >= 10 { 10 } else if k % 2 == 0 { 7 } else { 13 };
        a
    });
    // Three counts up, three down: 0.45 degrees.
    assert!((paid + 9.0 * one + 9.0 * w * 0.45 / SHAKE_DEG).abs() < 1e-4, "paid {paid}");
}

#[test]
fn a_steady_turn_costs_nothing() {
    let mut e = Env::new(Mode::SelfPlay, bare(0.12), 1);
    let paid = run(&mut e, 40, |_| with(5, RIGHT)) + run(&mut e, 4, |_| IDLE);
    assert_eq!((paid, e.episode().shake[0]), (0.0, 0.0));
    // Nor does a turn that slows down, stops and goes on, or one that is
    // taken back after the view has stood still for a while.
    let paid = run(&mut e, 40, |k| with(5, [RIGHT, 16, 13, 14, RIGHT][k % 5])) + run(&mut e, 4, |_| IDLE);
    assert_eq!(paid, 0.0);
    let paid = run(&mut e, 9, |k| with(5, [RIGHT, 13, 13, 13, LEFT, 13, 13, 13, 13][k]));
    assert_eq!(paid, 0.0);
    // Two ticks of standing still do not hide it.
    let paid = run(&mut e, 8, |k| with(5, [RIGHT, 13, 13, LEFT, 13, 13, 13, 13][k]));
    assert!((paid + 0.12 * 0.75 / SHAKE_DEG).abs() < 1e-5, "paid {paid}");
    // And without the weight nothing is paid.
    let mut e = Env::new(Mode::SelfPlay, bare(0.0), 1);
    let paid = run(&mut e, 12, |k| with(5, if k % 2 == 0 { LEFT } else { RIGHT }));
    assert_eq!(paid, 0.0);
    assert!(e.episode().shake[0] > 6.0);
}

#[test]
fn a_look_at_the_ground_and_back_pays_once_and_no_more_than_the_limit() {
    let w = 0.12f32;
    let mut e = Env::new(Mode::SelfPlay, bare(w), 1);
    let paid = run(&mut e, 4, |k| with(6, [FLICK_DOWN, FLICK_UP, 10, 10][k]));
    assert!((paid + w).abs() < 1e-5, "paid {paid}");
    let paid = run(&mut e, 20, |_| IDLE);
    assert_eq!(paid, 0.0);
}

/// Health player 0 has after one potion thrown the given way, and what the
/// potion offered it.
fn one_potion(strict: f64, up: bool) -> (f32, f64) {
    let mut p = params();
    p.pot_strict_prob = strict;
    let mut e = Env::new(Mode::SelfPlay, p, 3);
    if up {
        run(&mut e, 1, |_| with(6, FLICK_UP));
    } else {
        run(&mut e, 2, |_| with(6, DOWN));
    }
    run(&mut e, 1, |_| with(9, 1));
    run(&mut e, 45, |_| IDLE);
    assert_eq!(e.episode().splashes[0], 1.0);
    (e.m.s.p[0].health, e.episode().pot_offer[0])
}

#[test]
fn in_a_strict_duel_a_potion_that_is_not_perfect_gives_a_quarter() {
    // Thrown at its feet, standing: seven of the eight points in the game.
    assert_eq!(one_potion(0.0, false), (13.0, 7.0));
    assert_eq!(one_potion(1.0, false), (6.0 + 7.0 * 0.25, 7.0));
    // Thrown straight up it comes down on its thrower: all eight, strict or not.
    assert_eq!(one_potion(0.0, true), (14.0, 8.0));
    assert_eq!(one_potion(1.0, true), (14.0, 8.0));
}

/// What player 0 is paid over a potion thrown at its feet while it stands
/// with 6 health, with no reward for the healing itself.
fn paid_for_a_potion(strict: f64, w_waste: f32) -> f32 {
    let mut p = bare(0.0);
    p.pot_strict_prob = strict;
    p.w_heal = 0.0;
    p.w_miss = 0.0;
    p.w_waste = w_waste;
    let mut e = Env::new(Mode::SelfPlay, p, 3);
    let paid = run(&mut e, 2, |_| with(6, DOWN)) + run(&mut e, 1, |_| with(9, 1)) + run(&mut e, 45, |_| IDLE);
    assert_eq!((e.episode().splashes[0], e.episode().pot_offer[0]), (1.0, 7.0));
    paid
}

#[test]
fn what_a_strict_duel_takes_is_not_charged_as_waste_again() {
    // Seven of the eight points offered and all of them needed: one point
    // of waste, in a strict duel as in any other.
    let hh = Env::new(Mode::SelfPlay, bare(0.0), 3).m.s.cfg.hit_health();
    let one = paid_for_a_potion(0.0, 1.0) - paid_for_a_potion(0.0, 0.0);
    assert!((one + 1.0 / hh).abs() < 1e-4, "{one} {hh}");
    let strict = paid_for_a_potion(1.0, 1.0) - paid_for_a_potion(1.0, 0.0);
    assert!((strict - one).abs() < 1e-5, "{strict} {one}");
}

fn lesson(mode: Mode, lessons: u32, seed: u64) -> Env {
    let mut p = Params::default();
    p.lesson_prob = 1.0;
    p.lessons = lessons;
    p.kb_random = 0.7;
    p.opp_edge_prob = 0.5;
    Env::new(mode, p, seed)
}

#[test]
fn the_lesson_in_which_nobody_is_moved() {
    for mode in [Mode::Bot, Mode::SelfPlay] {
        for seed in 0..200 {
            let e = lesson(mode, 32, seed);
            assert_eq!(e.lesson, 6);
            let k = e.m.s.plugin.expect("knockback from the hit alone");
            assert!(k.h_ground <= 0.15 && k.h_air <= 0.17 && k.h_sprint <= 0.15 && k.v_ground <= 0.2 && k.v_air <= 0.2, "{k:?}");
            let (a, b) = (e.m.s.p[0].l.body.pos, e.m.s.p[1].l.body.pos);
            let d = (a[0] - b[0]).hypot(a[2] - b[2]);
            assert!((2.0..=5.01).contains(&d), "{d}");
            // Both start hurt and with potions: a duel of who uses them better.
            assert!(!e.m.s.cfg.infinite_health);
            for me in &e.m.s.p {
                assert!((4.0..=12.0).contains(&me.health) && me.potions.count_ones() >= 3, "{} {}", me.health, me.potions);
            }
        }
    }
}

#[test]
fn the_lesson_in_a_corner() {
    for mode in [Mode::Bot, Mode::SelfPlay] {
        let mut stuck = 0;
        for seed in 0..200 {
            let e = lesson(mode, 64, seed);
            assert_eq!(e.lesson, 7);
            let h = e.params.arena_half;
            let (a, b) = (e.m.s.p[0].l.body.pos, e.m.s.p[1].l.body.pos);
            // Near both walls of one corner, and the other player farther
            // from both and inside the arena.
            for k in [0, 2] {
                let wall = h * a[k].signum();
                assert!((0.4..=2.0).contains(&(wall - a[k]).abs()), "{a:?}");
                assert!((wall - b[k]).abs() > (wall - a[k]).abs() && b[k].abs() < h - 0.3, "{a:?} {b:?}");
            }
            let d = (a[0] - b[0]).hypot(a[2] - b[2]);
            assert!((2.0..=4.0).contains(&d), "{d}");
            let me = &e.m.s.p[0];
            assert!(!e.m.s.cfg.infinite_health && (4.0..=12.0).contains(&me.health) && me.potions.count_ones() >= 3);
            assert!(e.m.c[1].edge.kb_taken <= 0.7);
            stuck += (me.pearls == 0) as i32;
        }
        // Seven in ten without a pearl, and one in five of the rest by the
        // draw every duel makes.
        assert!((130..=180).contains(&stuck), "{stuck}");
    }
}

#[test]
fn the_extreme_lessons_get_their_share() {
    let count = |mode: Mode, lessons: u32, share: f64| {
        let mut p = Params::default();
        p.lesson_prob = 1.0;
        p.lessons = lessons;
        p.extreme_share = share;
        let mut n = [0; 8];
        let mut e = Env::new(mode, p, 9);
        for _ in 0..4000 {
            n[e.lesson as usize] += 1;
            e.reset();
        }
        n
    };
    let n = count(Mode::Bot, 127, 0.6);
    assert_eq!(n[0], 0);
    for k in 1..=4 {
        assert!((320..=480).contains(&n[k]), "{n:?}");
    }
    for k in 5..=7 {
        assert!((700..=900).contains(&n[k]), "{n:?}");
    }
    // The bot that runs rings is a scripted one: no such lesson without it.
    let n = count(Mode::SelfPlay, 127, 0.6);
    assert_eq!(n[5], 0);
    assert!((1050..=1350).contains(&n[6]) && (1050..=1350).contains(&n[7]), "{n:?}");
    // The lessons there were are drawn as they were.
    let n = count(Mode::Bot, 31, 0.4);
    assert_eq!((n[6], n[7]), (0, 0));
    assert!((1450..=1750).contains(&n[5]), "{n:?}");
    let n = count(Mode::SelfPlay, 31, 0.4);
    assert_eq!((n[5], n[6], n[7]), (0, 0, 0));
}
