//! A drink or a meal begun with the other player close
//! (`Params::w_sip_near`, `Params::sip_near_dist`).
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];
const EAT: i32 = 3;

fn params(dist: f64, w: f32) -> Params {
    let mut p = Params::default();
    p.infinite_health = false;
    p.potions = [7.0, 7.0];
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
    p.hunger_prob = 1.0;
    p.start_food = [5.0, 5.0];
    p.w_sip_near = w;
    p
}

/// What player 0 was paid over `n` of its ticks, asking to eat on the first.
fn eat(p: Params) -> (f32, Env) {
    let mut e = Env::new(Mode::SelfPlay, p, 6);
    let (mut fo, mut fp) = ([0.0f32; OBS_DIM], [0.0f32; PRIV_DIM]);
    let (mut k, mut sum) = (0, 0.0);
    while k < 50 {
        if e.pending == 0 {
            let mut a = IDLE;
            if k == 0 {
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
        }
    }
    (sum, e)
}

#[test]
fn a_meal_begun_close_costs_by_how_close() {
    for (dist, share) in [(2.0, 0.75), (4.0, 0.5), (6.0, 0.25), (12.0, 0.0)] {
        let (plain, e0) = eat(params(dist, 0.0));
        let (paid, e) = eat(params(dist, 2.0));
        assert_eq!(e0.episode().meals[0], 1.0);
        assert_eq!(e.episode().sips[0], 1.0);
        assert_eq!(e.episode().meals[0], 1.0, "the meal goes on as before");
        let cost = plain - paid;
        assert!((cost - 2.0 * share).abs() < 0.02, "at {dist}: {cost}, expected {}", 2.0 * share);
    }
}

#[test]
fn off_by_default() {
    let p = Params::default();
    assert_eq!((p.w_sip_near, p.sip_near_dist), (0.0, 8.0));
    let mut p = Params::default();
    assert!(p.set(209, 1.5) && p.set(210, 6.0));
    assert_eq!((p.w_sip_near, p.sip_near_dist), (1.5, 6.0));
}
