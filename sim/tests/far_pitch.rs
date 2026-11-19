//! Looking up far from the other player with nothing under way
//! (`Params::w_far_pitch`, `Params::far_pitch_deg`, `Params::far_pitch_dist`).
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];
/// The pitch bin that moves the view up the most.
const UP: i32 = 0;
const EAT: i32 = 3;

fn params(dist: f64, w: f32) -> Params {
    let mut p = Params::default();
    p.infinite_health = false;
    p.start_hurt = 0.0;
    p.pearl_prob = 0.0;
    p.start_dist_min = dist;
    p.start_dist_max = dist;
    p.start_yaw_noise = 0.0;
    p.ep_ticks_min = 100_000.0;
    p.ep_ticks_max = 100_000.0;
    p.kb_random = 0.0;
    p.opp_edge_prob = 0.0;
    p.speed_end_prob = 0.0;
    p.hunger_prob = 0.0;
    p.w_far_pitch = w;
    p
}

/// What player 0 was paid over its first `n` ticks, and its pitch then:
/// it moves the view up on ticks `up`, and asks to eat on its first if
/// `eat`.
fn run(p: Params, up: std::ops::Range<i32>, eat: bool, n: i32) -> (f32, f32) {
    let mut e = Env::new(Mode::SelfPlay, p, 6);
    let (mut fo, mut fp) = ([0.0f32; OBS_DIM], [0.0f32; PRIV_DIM]);
    let (mut k, mut sum) = (0, 0.0);
    let mut pitch = 0.0;
    while k < n {
        if e.pending == 0 {
            let mut a = IDLE;
            if up.contains(&k) {
                a[6] = UP;
            }
            if eat && k == 0 {
                a[9] = EAT;
            }
            e.step(&a);
            k += 1;
        } else {
            e.step(&IDLE);
        }
        let who = e.pending;
        let r = e.take_outcome(&mut fo, &mut fp).0;
        if who == 0 {
            sum += r;
            pitch = e.m.c[0].lp.l.pitch;
        }
    }
    (sum, pitch)
}

#[test]
fn looking_up_far_away_costs_by_how_far_up() {
    // Up to the top within a few ticks, then 30 ticks there: about 30 ticks
    // of the whole cost.
    let (plain, pitch) = run(params(20.0, 0.0), 0..10, false, 40);
    let (paid, _) = run(params(20.0, 1.0), 0..10, false, 40);
    assert!(pitch < -85.0, "{pitch}");
    let cost = plain - paid;
    assert!(cost > 25.0 && cost < 40.0, "{cost}");
}

#[test]
fn level_or_close_costs_nothing() {
    let (plain, _) = run(params(20.0, 0.0), 0..0, false, 40);
    let (paid, _) = run(params(20.0, 1.0), 0..0, false, 40);
    assert!((plain - paid).abs() < 1e-4, "level far away: {}", plain - paid);
    let (plain, _) = run(params(6.0, 0.0), 0..10, false, 40);
    let (paid, _) = run(params(6.0, 1.0), 0..10, false, 40);
    assert!((plain - paid).abs() < 1e-4, "up close by: {}", plain - paid);
}

#[test]
fn nothing_while_a_meal_has_the_keys() {
    let mut p0 = params(20.0, 0.0);
    p0.hunger_prob = 1.0;
    p0.start_food = [5.0, 5.0];
    let mut p1 = p0;
    p1.w_far_pitch = 1.0;
    // The meal runs for 32 ticks: look up on its first ten, count twenty.
    let (plain, _) = run(p0, 1..11, true, 20);
    let (paid, _) = run(p1, 1..11, true, 20);
    assert!((plain - paid).abs() < 1e-4, "{}", plain - paid);
}

#[test]
fn off_by_default() {
    let p = Params::default();
    assert_eq!((p.w_far_pitch, p.far_pitch_deg, p.far_pitch_dist), (0.0, 40.0, 10.0));
    let mut p = Params::default();
    assert!(p.set(270, 0.01) && p.set(271, 45.0) && p.set(272, 12.0));
    assert_eq!((p.w_far_pitch, p.far_pitch_deg, p.far_pitch_dist), (0.01, 45.0, 12.0));
}
