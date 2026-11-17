//! A Speed that runs out, potions of it to drink, hunger and steak, as the
//! trainer uses them: the kit, the keys of a drink and of a meal, what an
//! agent is told, and scripted bots that drink and eat. How a drink and a
//! meal go in the game is checked against the real game in `parity.rs`.
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};
use pvpsim::net::{Item, DRINK_TICKS, SLOT_FOOD, SLOT_SWORD};

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0];
/// What the last control asks for.
const THROW: i32 = 1;
const DRINK: i32 = 2;
const EAT: i32 = 3;
const STOP: i32 = 4;

fn params() -> Params {
    let mut p = Params::default();
    p.infinite_health = false;
    p.potions = [7.0, 7.0];
    p.start_hurt = 0.0;
    p.pearl_prob = 0.0;
    p.start_dist_min = 12.0;
    p.start_dist_max = 12.0;
    p.start_yaw_noise = 0.0;
    p.ep_ticks_min = 100_000.0;
    p.ep_ticks_max = 100_000.0;
    p.kb_random = 0.0;
    p.opp_edge_prob = 0.0;
    p.speed_end_prob = 1.0;
    p.speed_left_s = [0.0, 0.0];
    p.drinks = [2.0, 2.0];
    p
}

/// Let player 0 make `n` ticks with the actions `f` gives for each of them,
/// while player 1 stands still.
fn run(e: &mut Env, n: usize, mut f: impl FnMut(&Env, usize) -> [i32; ACT_HEADS]) {
    let (mut fo, mut fp) = ([0.0f32; OBS_DIM], [0.0f32; PRIV_DIM]);
    let mut k = 0;
    while k < n {
        if e.pending == 0 {
            let a = f(e, k);
            e.step(&a);
            k += 1;
        } else {
            e.step(&IDLE);
        }
        if e.pending == 0 || e.mode == Mode::SelfPlay {
            e.take_outcome(&mut fo, &mut fp);
        }
    }
}

/// The same, returning what player 0 was paid over those ticks.
fn paid(e: &mut Env, n: usize, mut f: impl FnMut(&Env, usize) -> [i32; ACT_HEADS]) -> f32 {
    let (mut fo, mut fp) = ([0.0f32; OBS_DIM], [0.0f32; PRIV_DIM]);
    let (mut k, mut sum) = (0, 0.0);
    while k < n {
        if e.pending == 0 {
            let a = f(e, k);
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
    sum
}

fn bar(v: i32) -> [i32; ACT_HEADS] {
    let mut a = IDLE;
    a[9] = v;
    a
}

fn obs(e: &Env) -> [f32; OBS_DIM] {
    let mut o = [0.0f32; OBS_DIM];
    e.write_obs(0, &mut o);
    o
}

#[test]
fn the_kit_has_potions_of_speed_after_the_healing_ones() {
    let e = Env::new(Mode::SelfPlay, params(), 1);
    for i in 0..2 {
        let (sp, lp) = (&e.m.s.p[i], &e.m.c[i].lp);
        assert_eq!(sp.potions.count_ones(), 5, "five healing potions and two of Speed fill the seven slots");
        assert_eq!(sp.drinks, 0b1_1000_0000);
        assert_eq!((lp.potions, lp.drinks), (sp.potions, sp.drinks));
        assert_eq!(sp.speed_ticks, 0);
        assert_eq!(sp.l.attr.speed_amp, -1, "no Speed at the start with no time left");
        assert_eq!(lp.l.attr.speed_amp, -1);
    }
    let o = obs(&e);
    assert_eq!((o[128], o[129], o[130]), (0.0, 0.0, 1.0));
}

#[test]
fn asked_once_a_drink_is_held_to_the_end_and_gives_speed() {
    let mut e = Env::new(Mode::SelfPlay, params(), 2);
    let mut done_at = None;
    run(&mut e, 60, |e, k| {
        if done_at.is_none() && e.m.s.p[0].speed_ticks > 0 {
            done_at = Some(k);
        }
        if k == 0 { bar(DRINK) } else { IDLE }
    });
    let (sp, lp) = (&e.m.s.p[0], &e.m.c[0].lp);
    // 32 ticks of use on the server, which begins when the click arrives.
    let at = done_at.expect("the drink was never finished");
    assert!((33..=40).contains(&at), "finished after {at} ticks");
    assert_eq!(sp.drinks.count_ones(), 1);
    assert_eq!(sp.bottles.count_ones(), 1, "the bottle stays in the slot");
    assert_eq!((lp.drinks, lp.bottles), (sp.drinks, sp.bottles));
    assert_eq!(sp.l.attr.speed_amp, 1);
    assert_eq!(lp.l.attr.speed_amp, 1, "the client is told its new speed");
    assert!(sp.speed_ticks > DRINK_TICKS - 30 && sp.speed_ticks <= DRINK_TICKS);
    assert_eq!(lp.held, SLOT_SWORD, "the sword comes back by itself");
    assert!(!lp.using_item && !sp.using_item);
    assert_eq!(e.episode().drinks, [1.0, 0.0]);
    assert_eq!(e.episode().sips, [1.0, 0.0]);
    let o = obs(&e);
    assert_eq!(o[128], 1.0);
    assert!(o[129] > 0.95, "nearly all of the time left: {}", o[129]);
    assert_eq!(o[130], 0.5);
    // The other player sees the particles.
    assert!(e.m.c[1].rp.fx);
}

#[test]
fn a_drink_slows_the_player_down_and_shows() {
    let mut e = Env::new(Mode::SelfPlay, params(), 3);
    let mut walk = IDLE;
    walk[0] = 1;
    walk[3] = 1;
    run(&mut e, 20, |_, _| walk);
    let fast = e.m.c[0].lp.l.body.motion[0].hypot(e.m.c[0].lp.l.body.motion[2]);
    let mut seen = false;
    run(&mut e, 25, |e, k| {
        seen |= e.m.c[1].rp.using && e.m.c[1].rp.item == Item::Drink;
        let mut a = walk;
        a[9] = if k == 0 { DRINK } else { 0 };
        a
    });
    let slow = e.m.c[0].lp.l.body.motion[0].hypot(e.m.c[0].lp.l.body.motion[2]);
    assert!(e.m.c[0].lp.consuming());
    assert!(slow < 0.3 * fast, "walking at {slow} while drinking, {fast} before");
    assert!(seen, "the other player sees the drink");
    let o = obs(&e);
    assert_eq!((o[135], o[136]), (1.0, 0.0));
    assert!(o[137] > 0.5 && o[137] < 1.0);
    let mut o1 = [0.0f32; OBS_DIM];
    e.write_obs(1, &mut o1);
    assert_eq!(o1[141], 1.0);
    assert!(o1[143] > 0.3);
}

#[test]
fn a_drink_that_is_stopped_keeps_the_potion() {
    for stop in [STOP, THROW] {
        let mut e = Env::new(Mode::SelfPlay, params(), 4);
        run(&mut e, 30, |_, k| match k {
            0 => bar(DRINK),
            15 => bar(stop),
            _ => IDLE,
        });
        let (sp, lp) = (&e.m.s.p[0], &e.m.c[0].lp);
        assert_eq!(sp.drinks.count_ones(), 2);
        assert_eq!(sp.speed_ticks, 0);
        assert!(!sp.using_item && !lp.using_item);
        assert_eq!(lp.held, SLOT_SWORD);
        assert_eq!(sp.potions.count_ones(), 5, "the tick that stops throws nothing");
        assert_eq!(e.episode().sips, [1.0, 0.0]);
        assert_eq!(e.episode().drinks, [0.0, 0.0]);
    }
}

#[test]
fn speed_runs_out_and_the_client_is_told() {
    let mut p = params();
    p.speed_left_s = [2.0, 2.0];
    let mut e = Env::new(Mode::SelfPlay, p, 5);
    assert_eq!(e.m.s.p[0].speed_ticks, 40);
    assert_eq!(e.m.c[0].lp.l.attr.speed_amp, 1);
    assert!(e.m.c[1].rp.fx);
    let o = obs(&e);
    assert_eq!(o[128], 1.0);
    assert!((o[129] - 40.0 / 1800.0).abs() < 1e-6);
    run(&mut e, 50, |_, _| IDLE);
    assert_eq!(e.m.s.p[0].speed_ticks, 0);
    assert_eq!(e.m.s.p[0].l.attr.speed_amp, -1);
    assert_eq!(e.m.c[0].lp.l.attr.speed_amp, -1);
    assert!(!e.m.c[1].rp.fx, "the particles are gone for the other player");
    let o = obs(&e);
    assert_eq!((o[128], o[129]), (0.0, 0.0));
    let st = e.episode();
    assert!(st.speed_on[0] >= 38.0 && st.speed_on[0] <= 41.0, "{:?}", st.speed_on);
}

#[test]
fn a_meal_is_one_steak_and_none_with_a_full_stomach() {
    let mut p = params();
    p.speed_end_prob = 0.0;
    p.hunger_prob = 1.0;
    p.start_food = [5.0, 5.0];
    let mut e = Env::new(Mode::SelfPlay, p, 6);
    {
        let (sp, lp) = (&e.m.s.p[0], &e.m.c[0].lp);
        assert_eq!((sp.steak, lp.steak), (64, 64));
        assert_eq!(sp.potions.count_ones(), 6, "the steak takes the last slot");
        assert_eq!((sp.food.level, lp.food), (5, 5));
    }
    let o = obs(&e);
    assert_eq!((o[131], o[132], o[134]), (1.0, 0.25, 0.0));
    let mut first = None;
    run(&mut e, 60, |e, k| {
        if first.is_none() && e.m.s.p[0].food.level > 5 {
            first = Some(k);
        }
        if k == 0 { bar(EAT) } else { IDLE }
    });
    let at = first.expect("nothing was eaten");
    assert!((33..=40).contains(&at), "the steak after {at} ticks");
    {
        let (sp, lp) = (&e.m.s.p[0], &e.m.c[0].lp);
        // The button is let go on the tick the server says the steak is
        // eaten, so one steak it is.
        assert_eq!((sp.food.level, lp.food), (13, 13));
        assert_eq!((sp.steak, lp.steak), (63, 63));
        assert_eq!(lp.held, SLOT_SWORD, "the sword comes back by itself");
        assert!(!lp.using_item && !sp.using_item);
    }
    // 13 + 8 is more than 20: the second steak fills it.
    run(&mut e, 60, |_, k| if k == 0 { bar(EAT) } else { IDLE });
    assert_eq!(e.m.s.p[0].food.level, 20);
    assert_eq!(e.episode().meals, [2.0, 0.0]);
    let o = obs(&e);
    assert_eq!((o[132], o[134]), (1.0, 1.0));
    // Asked again with a full stomach, nothing happens.
    run(&mut e, 5, |_, k| if k == 0 { bar(EAT) } else { IDLE });
    assert_eq!(e.m.c[0].lp.held, SLOT_SWORD);
    assert_eq!(e.m.c[0].lp.steak, 62);
    assert_eq!(e.m.s.p[0].held, SLOT_SWORD);
}

#[test]
fn a_meal_that_is_stopped_leaves_the_steak() {
    let mut p = params();
    p.speed_end_prob = 0.0;
    p.hunger_prob = 1.0;
    p.start_food = [3.0, 3.0];
    let mut e = Env::new(Mode::SelfPlay, p, 7);
    run(&mut e, 40, |_, k| match k {
        0 => bar(EAT),
        20 => bar(STOP),
        _ => IDLE,
    });
    let (sp, lp) = (&e.m.s.p[0], &e.m.c[0].lp);
    assert_eq!(sp.food.level, 3);
    assert_eq!((sp.steak, lp.steak), (64, 64));
    assert_eq!((lp.held, sp.held), (SLOT_SWORD, SLOT_SWORD));
    assert!(!sp.using_item && !lp.using_item);
    assert_eq!(e.episode().meals, [0.0, 0.0]);
    let _ = SLOT_FOOD;
}

#[test]
fn hunger_takes_the_sprint_away() {
    let mut p = params();
    p.speed_end_prob = 0.0;
    p.hunger_prob = 1.0;
    p.start_food = [6.0, 6.0];
    let mut e = Env::new(Mode::SelfPlay, p, 8);
    let mut run_keys = IDLE;
    run_keys[0] = 1;
    run_keys[3] = 1;
    run(&mut e, 20, |_, _| run_keys);
    assert!(!e.m.c[0].lp.l.sprinting, "no sprint with six food or less");
    // It sprints again within a few ticks of the steak.
    let (mut eaten, mut sprint) = (None, None);
    run(&mut e, 50, |e, k| {
        if eaten.is_none() && e.m.c[0].lp.food > 6 {
            eaten = Some(k);
        }
        if sprint.is_none() && e.m.c[0].lp.l.sprinting {
            sprint = Some(k);
        }
        let mut a = run_keys;
        a[9] = if k == 0 { EAT } else { 0 };
        a
    });
    let (eaten, sprint) = (eaten.expect("nothing eaten"), sprint.expect("no sprint after the steak"));
    assert!(sprint >= eaten && sprint <= eaten + 2, "eaten at {eaten}, sprinting at {sprint}");
}

#[test]
fn duels_without_the_new_kit_are_as_before() {
    let mut p = params();
    p.speed_end_prob = 0.0;
    let e = Env::new(Mode::SelfPlay, p, 9);
    for i in 0..2 {
        let (sp, lp) = (&e.m.s.p[i], &e.m.c[i].lp);
        assert_eq!(sp.potions.count_ones(), 7);
        assert_eq!((sp.drinks, sp.steak, lp.drinks, lp.steak), (0, 0, 0, 0));
        assert!(sp.speed_ticks > 1_000_000);
        assert_eq!(sp.l.attr.speed_amp, 1);
    }
    let o = obs(&e);
    assert_eq!((o[128], o[129], o[130], o[131]), (1.0, 1.0, 0.0, 0.0));
}

/// How many of twenty bots of this level had done `what` after `ticks`
/// ticks of standing `dist` blocks from an agent that does nothing.
/// Having Speed and having food enough to sprint are paid as a difference
/// of state: once when a player gets there, taken back when it loses it,
/// and nothing for a drink that changes neither.
#[test]
fn speed_and_food_are_paid_once_and_taken_back() {
    let duel = |w: f32, left: f64| {
        let mut p = params();
        p.w_speed = w;
        p.speed_left_s = [left, left];
        Env::new(Mode::SelfPlay, p, 21)
    };
    let drink = |_: &Env, k: usize| bar(if k == 0 { DRINK } else { 0 });
    // Without Speed: the drink brings it, and nothing else in these ticks
    // differs from the same duel without the payment.
    let (mut a, mut b) = (duel(2.0, 0.0), duel(0.0, 0.0));
    let (ra, rb) = (paid(&mut a, 60, drink), paid(&mut b, 60, drink));
    assert_eq!(a.m.c[0].lp.speed_amp, 1);
    assert!((ra - rb - 2.0).abs() < 1e-4, "paid {ra} with, {rb} without");
    // A second drink with Speed on changes nothing.
    let (ra, rb) = (paid(&mut a, 60, drink), paid(&mut b, 60, drink));
    assert_eq!(a.m.s.p[0].drinks, 0, "both potions are drunk");
    assert!((ra - rb).abs() < 1e-4, "paid {ra} with, {rb} without");
    // When it runs out it is taken back.
    let n = DRINK_TICKS as usize + 20;
    let (ra, rb) = (paid(&mut a, n, |_, _| IDLE), paid(&mut b, n, |_, _| IDLE));
    assert_eq!(a.m.c[0].lp.speed_amp, -1);
    assert!((ra - rb + 2.0).abs() < 1e-3, "paid {ra} with, {rb} without");

    // Food: too hungry to sprint, then a steak.
    let hungry = |w: f32| {
        let mut p = params();
        p.speed_end_prob = 0.0;
        p.hunger_prob = 1.0;
        p.start_food = [6.0, 6.0];
        p.w_fed = w;
        Env::new(Mode::SelfPlay, p, 22)
    };
    let eat = |_: &Env, k: usize| bar(if k == 0 { EAT } else { 0 });
    let (mut a, mut b) = (hungry(1.5), hungry(0.0));
    let (ra, rb) = (paid(&mut a, 60, eat), paid(&mut b, 60, eat));
    assert!(a.m.c[0].lp.food > 6);
    assert!((ra - rb - 1.5).abs() < 1e-4, "paid {ra} with, {rb} without");
    let (ra, rb) = (paid(&mut a, 60, eat), paid(&mut b, 60, eat));
    assert!((ra - rb).abs() < 1e-4, "a second steak: paid {ra} with, {rb} without");
}

fn bots(level: f64, dist: f64, ticks: usize, p: &Params, what: impl Fn(&Env) -> bool) -> i32 {
    let mut n = 0;
    for seed in 0..20 {
        let mut p = *p;
        p.bot_level = [level, level];
        p.start_dist_min = dist;
        p.start_dist_max = dist;
        let mut e = Env::new(Mode::Bot, p, 500 + seed);
        let mut ok = false;
        run(&mut e, ticks, |e, _| {
            ok |= what(e);
            IDLE
        });
        n += ok as i32;
    }
    n
}

#[test]
fn a_bot_without_speed_drinks_its_potion() {
    // From fourteen blocks away every level that drinks has the room it
    // wants. The first level neither heals itself nor drinks.
    for level in [2.0, 3.0, 4.0] {
        let n = bots(level, 14.0, 70, &params(), |e| e.m.s.p[1].speed_ticks > 0 && e.m.s.p[1].bottles != 0);
        assert!(n >= 17, "level {level}: {n} of 20 drank");
    }
    assert_eq!(bots(1.0, 14.0, 70, &params(), |e| e.m.c[1].lp.consuming()), 0);
    // None of them starts a drink right next to its opponent.
    for level in [2.0, 4.0] {
        let n = bots(level, 2.5, 25, &params(), |e| e.m.c[1].lp.consuming());
        assert!(n <= 2, "level {level} drank next to the agent {n} times");
    }
}

#[test]
fn a_hungry_bot_eats() {
    let mut p = params();
    p.speed_end_prob = 0.0;
    p.hunger_prob = 1.0;
    p.start_food = [5.0, 5.0];
    for level in [2.0, 4.0] {
        let n = bots(level, 14.0, 70, &p, |e| e.m.s.p[1].food.level > 5);
        assert!(n >= 17, "level {level}: {n} of 20 ate");
    }
}

#[test]
fn a_bot_with_speed_left_keeps_its_potion() {
    let mut p = params();
    p.speed_left_s = [60.0, 60.0];
    let n = bots(4.0, 14.0, 70, &p, |e| e.m.s.p[1].bottles != 0 || e.m.c[1].lp.consuming());
    assert_eq!(n, 0);
}
