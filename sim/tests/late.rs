//! Timing: ticks the other player was open to a hit and took none
//! (`Params::w_late`), and attacks into its hurt time (`Params::w_hurt_click`).
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];

fn params(dist: f64, w_late: f32, w_hurt: f32) -> Params {
    let mut p = Params::default();
    p.infinite_health = false;
    p.potions = [0.0, 0.0];
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
    p.w_late = w_late;
    p.w_hurt_click = w_hurt;
    p
}

/// What player 0 was paid over `n` of its ticks, standing still. The pay
/// of its last tick is still owed.
fn run(p: Params, n: usize) -> (f32, Env) {
    let mut e = Env::new(Mode::SelfPlay, p, 11);
    let (mut fo, mut fp) = ([0.0f32; OBS_DIM], [0.0f32; PRIV_DIM]);
    let (mut k, mut sum) = (0, 0.0);
    while k < n {
        if e.pending == 0 {
            e.step(&IDLE);
            k += 1;
        } else {
            e.step(&IDLE);
        }
        let who = e.pending;
        let r = e.take_outcome(&mut fo, &mut fp).0;
        if who == 0 {
            sum += r;
        }
    }
    (sum, e)
}

#[test]
fn standing_next_to_an_open_opponent_is_late_every_tick() {
    let (plain, e0) = run(params(2.0, 0.0, 0.0), 60);
    let (paid, e) = run(params(2.0, 0.5, 0.0), 60);
    let late = e.episode().late[0];
    assert_eq!(e0.episode().late[0], late, "counted whether it costs or not");
    assert!(late >= 55.0, "late on {late} of 60 ticks");
    assert_eq!(e.episode().hits[0], 0.0);
    let cost = plain - paid;
    assert!(cost >= 0.5 * (late as f32 - 2.0) - 1e-3 && cost <= 0.5 * late as f32 + 1e-3, "cost {cost} for {late} ticks");
}

#[test]
fn out_of_reach_is_never_late() {
    let (_, e) = run(params(8.0, 0.5, 0.0), 60);
    assert_eq!(e.episode().late[0], 0.0);
}

#[test]
fn a_bot_that_hits_on_every_chance_is_seldom_late() {
    let mut p = params(3.0, 0.0, 0.0);
    p.bot_level = [4.0, 4.0];
    p.ep_ticks_min = 2000.0;
    p.ep_ticks_max = 2000.0;
    p.infinite_health = true;
    let mut e = Env::new(Mode::Bot, p, 3);
    let (mut fo, mut fp) = ([0.0f32; OBS_DIM], [0.0f32; PRIV_DIM]);
    for _ in 0..600 {
        e.step(&IDLE);
        e.take_outcome(&mut fo, &mut fp);
    }
    let st = e.episode();
    assert!(st.hits[1] >= 30.0, "bot hits {}", st.hits[1]);
    // Its clicks come faster than the hurt time lets them land.
    assert!(st.hurt_clicks[1] >= st.hits[1], "clicks into the hurt time {}", st.hurt_clicks[1]);
    assert!(st.late[1] < 1.5 * st.hits[1], "late {} for {} hits", st.late[1], st.hits[1]);
    // The agent never clicks.
    assert_eq!(st.hurt_clicks[0], 0.0);
}

#[test]
fn off_by_default() {
    let p = Params::default();
    assert_eq!((p.w_late, p.late_pot_free, p.w_hurt_click), (0.0, 8.0, 0.0));
    let mut p = Params::default();
    assert!(p.set(240, 0.1) && p.set(241, 5.0) && p.set(242, 0.02));
    assert_eq!((p.w_late, p.late_pot_free, p.w_hurt_click), (0.1, 5.0, 0.02));
}


#[test]
fn the_hardest_bot_ping_is_a_param() {
    let p = Params::default();
    assert_eq!((p.insane_ping_min_ms, p.insane_ping_max_ms), (0.0, 60.0));
    let mut p = Params::default();
    assert!(p.set(244, 100.0) && p.set(245, 250.0));
    assert_eq!((p.insane_ping_min_ms, p.insane_ping_max_ms), (100.0, 250.0));
}
