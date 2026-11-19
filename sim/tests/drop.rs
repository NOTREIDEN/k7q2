//! Dying with healing potions left (`Params::w_drop`) costs more than
//! dying.
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};

const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0];

/// Player 0 stands still with a heart and a half against the bot until it
/// dies: the rewards of its ticks added up, and the deaths with potions
/// left counted.
fn die(w_drop: f32, potions: f64) -> (f32, f64) {
    let mut p = Params::default();
    p.infinite_health = false;
    p.potions = [potions, potions];
    p.pearl_prob = 0.0;
    p.start_dist_min = 2.5;
    p.start_dist_max = 2.5;
    p.bot_level = [3.0, 3.0];
    p.ep_ticks_min = 2000.0;
    p.ep_ticks_max = 2000.0;
    p.w_death = 5.0;
    p.w_drop = w_drop;
    let mut e = Env::new(Mode::Bot, p, 11);
    e.m.s.p[0].health = 3.0;
    e.m.c[0].lp.health = 3.0;
    let (mut fobs, mut fpriv) = (vec![0f32; OBS_DIM], vec![0f32; PRIV_DIM]);
    let mut total = 0.0;
    for _ in 0..2000 {
        e.step(&IDLE);
        let (r, d) = e.take_outcome(&mut fobs, &mut fpriv);
        total += r;
        if d != 0 {
            return (total, e.total.drops[0]);
        }
    }
    panic!("it did not die");
}

#[test]
fn dying_with_potions_left_costs_more_and_is_counted() {
    let (plain, n0) = die(0.0, 6.0);
    let (drop, n1) = die(10.0, 6.0);
    assert_eq!(n0, 0.0);
    assert_eq!(n1, 1.0);
    // Six potions left: the whole of it.
    assert!((plain - drop - 10.0).abs() < 1e-3, "{plain} {drop}");
    // With none there is nothing more to pay.
    let (plain, _) = die(0.0, 0.0);
    let (drop, n) = die(10.0, 0.0);
    assert_eq!(n, 0.0);
    assert_eq!(plain, drop);
}
