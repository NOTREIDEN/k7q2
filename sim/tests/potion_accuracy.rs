//! Potion accuracy: the health a healing potion gave its thrower out of
//! the eight it can give, the points lost because it broke away from the
//! thrower's feet and those lost because that much health was not
//! missing, counted in `Stats`; and what `w_pot_early` and `w_pot_far`
//! make the thrower pay for them.
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];
/// The pitch choice that looks down the fastest of the first set.
const DOWN: i32 = 26;

fn params(health: f64) -> Params {
    let mut p = Params::default();
    p.infinite_health = false;
    p.prot = [2.0, 2.0];
    p.sharp = [2.0, 2.0];
    p.potions = [7.0, 7.0];
    p.start_hurt = 1.0;
    p.start_health = [health, health];
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

/// Let player 0 make `n` ticks with the actions `f` gives for each of them,
/// while player 1 stands still. Returns what each player was paid (in
/// self-play the outcome taken after a tick closes the last action of the
/// player that acts next).
fn run(e: &mut Env, n: usize, mut f: impl FnMut(usize) -> [i32; ACT_HEADS]) -> [f32; 2] {
    let (mut fo, mut fp) = ([0.0f32; OBS_DIM], [0.0f32; PRIV_DIM]);
    let mut paid = [0.0f32; 2];
    let mut k = 0;
    while k < n {
        if e.pending == 0 {
            e.step(&f(k));
            k += 1;
        } else {
            e.step(&IDLE);
        }
        // What closes the last action of the player that acts next.
        let (r, _) = e.take_outcome(&mut fo, &mut fp);
        paid[e.pending] += r;
    }
    paid
}

fn with(head: usize, v: i32) -> [i32; ACT_HEADS] {
    let mut a = IDLE;
    a[head] = v;
    a
}

/// Look at the ground, throw `count` potions on as many ticks in a row,
/// and wait for them to break. Returns the environment and what each
/// player was paid from the throw on.
fn pot_down(p: Params, count: usize) -> (Env, [f32; 2]) {
    let mut e = Env::new(Mode::SelfPlay, p, 3);
    run(&mut e, 2, |_| with(6, DOWN));
    let a = run(&mut e, count, |_| with(9, 1));
    let b = run(&mut e, 40, |_| IDLE);
    (e, [a[0] + b[0], a[1] + b[1]])
}

#[test]
fn accuracy_is_split_into_what_it_gave_missed_and_overhealed() {
    // Six health: fourteen missing, and standing still a potion at the
    // feet offers seven of the eight (it breaks a little away from them).
    let (e, _) = pot_down(params(6.0), 1);
    let ep = e.episode();
    assert_eq!((ep.splashes[0], ep.pot_offer[0]), (1.0, 7.0));
    assert_eq!((ep.pot_given[0], ep.pot_far[0], ep.pot_over[0], ep.pot_early[0]), (7.0, 1.0, 0.0, 0.0));
    // Fifteen health: five missing, so of the seven it offered two were
    // more than was missing, and it was thrown too early.
    let (e, _) = pot_down(params(15.0), 1);
    let ep = e.episode();
    assert_eq!((ep.pot_given[0], ep.pot_far[0], ep.pot_over[0], ep.pot_early[0]), (5.0, 1.0, 2.0, 1.0));
    // Full health: all of it lost.
    let mut p = params(6.0);
    p.start_hurt = 0.0;
    let (e, _) = pot_down(p, 1);
    let ep = e.episode();
    assert_eq!((ep.pot_given[0], ep.pot_far[0], ep.pot_over[0], ep.pot_early[0]), (0.0, 1.0, 7.0, 1.0));
    // Thrown ahead, where it reaches nobody: all eight broke away.
    let mut e = Env::new(Mode::SelfPlay, params(6.0), 3);
    run(&mut e, 1, |_| with(9, 1));
    run(&mut e, 40, |_| IDLE);
    let ep = e.episode();
    assert_eq!((ep.splashes[0], ep.pot_given[0], ep.pot_far[0], ep.pot_over[0]), (1.0, 0.0, 8.0, 0.0));
    // In a strict duel the stats count what the game would have given.
    let mut p = params(6.0);
    p.pot_strict_prob = 1.0;
    p.pot_strict = 0.25;
    let (e, _) = pot_down(p, 1);
    let ep = e.episode();
    assert_eq!((ep.heal_self[0], ep.pot_given[0], ep.pot_far[0], ep.pot_over[0]), (1.75, 7.0, 1.0, 0.0));
}

#[test]
fn the_stats_are_written_after_the_tool_counts_in_order() {
    let (e, _) = pot_down(params(15.0), 1);
    let mut out = vec![0.0f64; pvpsim::env::STATS_DIM];
    e.episode().write(&mut out);
    // Right after tool_uses and tool_hits (four numbers ending at 111).
    assert_eq!(&out[111..119], &[5.0, 0.0, 1.0, 0.0, 2.0, 0.0, 1.0, 0.0]);
}

#[test]
fn a_potion_thrown_too_early_costs_at_the_throw() {
    let cost = |health: f64, count: usize| {
        let mut p = params(health);
        let (e0, plain) = pot_down(p.clone(), count);
        p.w_pot_early = 2.0;
        let (e, paid) = pot_down(p, count);
        assert_eq!(e.episode().splashes[0], count as f64);
        assert_eq!(e0.episode().pot_given, e.episode().pot_given);
        let hh = e.m.s.cfg.hit_health();
        ((plain[0] - paid[0]) * hh / 2.0, plain[1] - paid[1])
    };
    // Fourteen missing: nothing. Thirteen health, seven missing: one
    // point. Fifteen: three. Full: all eight.
    for (health, points) in [(6.0, 0.0), (12.0, 0.0), (13.0, 1.0), (15.0, 3.0), (20.0, 8.0)] {
        let (c, other) = cost(health, 1);
        assert!((c - points).abs() < 1e-3, "health {health}: {c} points, expected {points}");
        assert_eq!(other, 0.0, "the opponent gets nothing of it");
    }
    // Two in a row with four health (sixteen missing) both fit; with six,
    // the second has six of its eight, as the first is still on its way.
    let (c, _) = cost(4.0, 2);
    assert!(c.abs() < 1e-3, "{c}");
    let (c, _) = cost(6.0, 2);
    assert!((c - 2.0).abs() < 1e-3, "{c}");
}

#[test]
fn what_a_potion_did_not_offer_its_thrower_costs_all_of_it() {
    // At the feet, standing: one of the eight did not reach the thrower.
    let mut p = params(6.0);
    let (_, plain) = pot_down(p.clone(), 1);
    p.w_pot_far = 2.0;
    let (e, paid) = pot_down(p, 1);
    let hh = e.m.s.cfg.hit_health();
    assert!((plain[0] - paid[0] - 2.0 * 1.0 / hh).abs() < 1e-3, "{plain:?} {paid:?}");
    // Thrown ahead, where it reaches nobody: all eight, where `w_miss`
    // counts two.
    let far = |w: f32| {
        let mut p = params(6.0);
        p.w_pot_far = w;
        let mut e = Env::new(Mode::SelfPlay, p, 3);
        let a = run(&mut e, 1, |_| with(9, 1));
        let b = run(&mut e, 40, |_| IDLE);
        assert_eq!((e.episode().splashes[0], e.episode().pot_offer[0]), (1.0, 0.0));
        (a[0] + b[0], e.m.s.cfg.hit_health())
    };
    let ((plain, hh), (paid, _)) = (far(0.0), far(2.0));
    assert!((plain - paid - 2.0 * 8.0 / hh).abs() < 1e-3, "{plain} {paid}");
}

/// A potion the opponent stands near takes from what it offers the
/// thrower, who pays for that; the opponent is paid nothing more for it
/// than the health it got, as before.
#[test]
fn a_potion_the_opponent_shares_costs_the_thrower_only() {
    let run_it = |far: f32, early: f32| {
        let mut p = params(6.0);
        p.start_dist_min = 2.0;
        p.start_dist_max = 2.0;
        p.w_pot_far = far;
        p.w_pot_early = early;
        let (e, paid) = pot_down(p, 1);
        assert_eq!(e.episode().heal_gift[0], 4.0);
        assert_eq!(e.episode().pot_far[0], 1.0);
        (paid, e.m.s.cfg.hit_health())
    };
    let ((plain, hh), (paid, _)) = (run_it(0.0, 0.0), run_it(3.0, 3.0));
    assert!((plain[0] - paid[0] - 3.0 * 1.0 / hh).abs() < 1e-3, "{plain:?} {paid:?}");
    assert_eq!(plain[1], paid[1]);
}

#[test]
fn off_by_default() {
    let p = Params::default();
    assert_eq!((p.w_pot_early, p.w_pot_far), (0.0, 0.0));
    let mut p = Params::default();
    assert!(p.set(184, 1.5) && p.set(185, 2.5));
    assert_eq!((p.w_pot_early, p.w_pot_far), (1.5, 2.5));
}
