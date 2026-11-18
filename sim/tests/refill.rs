//! The inventory as the trainer uses it: potions in its rows, the keys that
//! get one down to the hotbar, what an agent is told, what it is paid,
//! lessons, and scripted bots that go to their inventory. How the screen, a
//! shift click and the server's answer go in the game is checked against
//! the real game in `parity.rs`.
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};
use pvpsim::net::{Item, Rows, SLOT_SWORD};

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0];
/// What the last control asks for.
const THROW: i32 = 1;
const DRINK: i32 = 2;
const STOP: i32 = 4;
const HEAL_UP: i32 = 5;
const SPEED_UP: i32 = 6;

fn params() -> Params {
    let mut p = Params::default();
    p.infinite_health = false;
    p.potions = [0.0, 0.0];
    p.start_hurt = 0.0;
    p.pearl_prob = 0.0;
    p.start_dist_min = 12.0;
    p.start_dist_max = 12.0;
    p.start_yaw_noise = 0.0;
    p.ep_ticks_min = 100_000.0;
    p.ep_ticks_max = 100_000.0;
    p.kb_random = 0.0;
    p.opp_edge_prob = 0.0;
    p.refill_prob = 1.0;
    p.inv_potions = [10.0, 10.0];
    p.cps_min = 20.0;
    p.cps_max = 20.0;
    p
}

/// Let player 0 make `n` ticks with the actions `f` gives for each of them,
/// while player 1 stands still. Returns what player 0 was paid.
fn run(e: &mut Env, n: usize, mut f: impl FnMut(&Env, usize) -> [i32; ACT_HEADS]) -> f32 {
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
fn the_rows_are_stocked_and_the_agent_is_told() {
    let e = Env::new(Mode::SelfPlay, params(), 1);
    for i in 0..2 {
        let (sp, lp) = (&e.m.s.p[i], &e.m.c[i].lp);
        assert_eq!((sp.potions, lp.potions), (0, 0), "no healing potion at hand");
        assert_eq!(sp.rows.count(Item::Potion), 10);
        assert_eq!(lp.rows, sp.rows);
    }
    let o = obs(&e);
    assert_eq!(o[144], 0.0, "the screen is shut");
    assert_eq!(o[145], 10.0 / 8.0);
    assert_eq!(o[146], 0.0);
    assert_eq!(o[147], 1.0, "all eight slots beside the sword are free");
    let mut pv = [0.0f32; PRIV_DIM];
    e.write_priv(0, &mut pv);
    assert_eq!((pv[68], pv[69]), (0.0, 10.0 / 8.0));
}

#[test]
fn asked_once_a_potion_comes_down_in_three_ticks() {
    let mut e = Env::new(Mode::SelfPlay, params(), 2);
    // Asked on one tick only: the key, the click and the key again follow.
    run(&mut e, 1, |_, _| bar(HEAL_UP));
    assert!(e.m.c[0].lp.inv_open, "the screen opens on the tick it is asked for");
    assert_eq!(e.m.c[0].lp.potions, 0);
    assert_eq!(obs(&e)[149], 1.0, "and the click is still to come");
    run(&mut e, 1, |_, _| IDLE);
    let lp = &e.m.c[0].lp;
    assert_eq!(lp.potions, 0b10, "the potion is in the first free slot at once, for the client");
    assert_eq!(lp.rows.count(Item::Potion), 9);
    assert!(lp.inv_open, "the screen stays open after the click");
    run(&mut e, 1, |_, _| IDLE);
    assert!(!e.m.c[0].lp.inv_open, "and shuts on the third tick");
    run(&mut e, 12, |_, _| IDLE);
    let sp = &e.m.s.p[0];
    assert_eq!((sp.potions, sp.rows.count(Item::Potion)), (0b10, 9), "the server made the same move");
    assert!(sp.can_click, "and took the click as the client said");
    let st = e.episode();
    assert_eq!((st.refills[0], st.inv_opens[0], st.inv_ticks[0]), (1.0, 1.0, 3.0));
    // It can be thrown like any other.
    run(&mut e, 2, |_, k| if k == 0 { bar(THROW) } else { IDLE });
    assert_eq!(e.m.c[0].lp.potions, 0);
    assert_eq!(e.episode().potions[0], 1.0);
}

#[test]
fn held_down_it_fills_the_hotbar_one_potion_a_tick() {
    let mut e = Env::new(Mode::SelfPlay, params(), 3);
    run(&mut e, 4, |_, _| bar(HEAL_UP));
    let lp = &e.m.c[0].lp;
    assert_eq!(lp.potions.count_ones(), 3, "the key, then a potion on each of three ticks");
    assert!(lp.inv_open);
    run(&mut e, 5, |_, _| bar(HEAL_UP));
    let lp = &e.m.c[0].lp;
    assert_eq!(lp.potions, 0b1_1111_1110, "all eight slots beside the sword");
    assert!(!lp.inv_open, "the click that fills the last slot shuts the screen");
    assert_eq!(lp.rows.count(Item::Potion), 2);
    // Nowhere to put another: asking does nothing.
    run(&mut e, 3, |_, _| bar(HEAL_UP));
    assert!(!e.m.c[0].lp.inv_open);
    assert_eq!(e.episode().inv_opens[0], 1.0);
    run(&mut e, 12, |_, _| IDLE);
    assert_eq!(e.m.s.p[0].potions, 0b1_1111_1110);
    assert_eq!(e.m.s.p[0].rows.count(Item::Potion), 2);
}

#[test]
fn anything_else_shuts_the_screen_and_nothing_else_is_done_meanwhile() {
    let mut e = Env::new(Mode::SelfPlay, params(), 4);
    let from = e.m.c[0].lp.l.body.pos;
    // Forward, sprint and attack asked on every tick as well.
    let mut a = bar(HEAL_UP);
    a[0] = 1;
    a[3] = 1;
    a[4] = 1;
    run(&mut e, 3, |_, _| a);
    assert_eq!(e.m.c[0].lp.potions.count_ones(), 2);
    assert_eq!(e.episode().swings[0], 0.0, "no click with the sword while the screen is open");
    let mut b = IDLE;
    b[0] = 1;
    b[4] = 1;
    run(&mut e, 1, |_, _| b);
    assert!(!e.m.c[0].lp.inv_open, "asking for nothing more shuts it");
    let at = e.m.c[0].lp.l.body.pos;
    assert_eq!((from[0], from[2]), (at[0], at[2]), "and it has not moved through all of it, the tick it shut too");
    assert_eq!(e.episode().swings[0], 0.0);
    run(&mut e, 1, |_, _| b);
    assert_eq!(e.episode().swings[0], 1.0, "on the next tick the keys count again");
    assert_ne!(e.m.c[0].lp.l.body.pos, at);
}

#[test]
fn opening_the_screen_lets_go_of_a_block() {
    let mut e = Env::new(Mode::SelfPlay, params(), 5);
    let mut block = IDLE;
    block[7] = 1;
    run(&mut e, 8, |_, _| block);
    assert!(e.m.c[0].lp.using_item && e.m.s.p[0].using_item);
    let mut a = bar(HEAL_UP);
    a[7] = 1;
    run(&mut e, 1, |_, _| a);
    assert!(e.m.c[0].lp.inv_open);
    assert!(!e.m.c[0].lp.using_item, "the sword comes down with the screen");
    run(&mut e, 8, |_, _| IDLE);
    assert!(!e.m.s.p[0].using_item, "and the server is told");
}

#[test]
fn nothing_opens_without_a_potion_to_get_or_in_the_middle_of_something() {
    let mut p = params();
    p.refill_prob = 0.0;
    let mut e = Env::new(Mode::SelfPlay, p, 6);
    run(&mut e, 3, |_, _| bar(HEAL_UP));
    assert!(!e.m.c[0].lp.inv_open, "empty rows");
    // No potion of Speed in the rows of a duel in which the Speed lasts.
    let mut e = Env::new(Mode::SelfPlay, params(), 6);
    run(&mut e, 3, |_, _| bar(SPEED_UP));
    assert!(!e.m.c[0].lp.inv_open);
    // In the middle of a drink the screen stays shut and the drink goes on.
    let mut p = params();
    p.speed_end_prob = 1.0;
    p.speed_left_s = [0.0, 0.0];
    p.drinks = [1.0, 1.0];
    let mut e = Env::new(Mode::SelfPlay, p, 6);
    run(&mut e, 10, |_, k| if k == 0 { bar(DRINK) } else { bar(HEAL_UP) });
    assert!(e.m.c[0].lp.consuming() && !e.m.c[0].lp.inv_open);
}

#[test]
fn a_potion_of_speed_comes_down_and_a_bottle_makes_room() {
    let mut p = params();
    p.speed_end_prob = 1.0;
    p.speed_left_s = [0.0, 0.0];
    p.drinks = [0.0, 0.0];
    p.inv_drinks = [2.0, 2.0];
    p.inv_potions = [3.0, 3.0];
    let mut e = Env::new(Mode::SelfPlay, p, 7);
    assert_eq!(e.m.c[0].lp.rows.count(Item::Drink), 2);
    assert_eq!(obs(&e)[146], 1.0);
    run(&mut e, 3, |_, k| if k == 0 { bar(SPEED_UP) } else { IDLE });
    let lp = &e.m.c[0].lp;
    assert_eq!((lp.drinks, lp.rows.count(Item::Drink)), (0b10, 1));
    // A hotbar with no free slot and one empty bottle in it: the bottle is
    // put away first, then the potion comes down, asked for once.
    for i in 0..1 {
        let mut b = e.m.s.p[i].bar();
        for k in 1..9u8 {
            b.put(k, if k == 4 { Item::Bottle } else { Item::Potion }, 1);
        }
        e.m.s.p[i].set_bar(b);
        e.m.c[i].lp.set_bar(b);
    }
    let rows_before = e.m.c[0].lp.rows;
    run(&mut e, 1, |_, _| bar(HEAL_UP));
    assert!(e.m.c[0].lp.inv_open);
    run(&mut e, 1, |_, _| IDLE);
    let lp = &e.m.c[0].lp;
    assert_eq!(lp.bottles, 0, "the bottle went to the rows");
    assert_eq!(lp.rows.count(Item::Bottle), 1);
    assert!(lp.inv_open);
    assert_eq!(obs(&e)[149], 1.0, "the potion is still to come");
    run(&mut e, 1, |_, _| IDLE);
    let lp = &e.m.c[0].lp;
    assert_eq!(lp.potions.count_ones(), 8, "and the potion took its place");
    assert_eq!(lp.rows.count(Item::Potion), rows_before.count(Item::Potion) - 1);
    assert!(!lp.inv_open, "which filled the hotbar, so the screen shut with it");
    run(&mut e, 12, |_, _| IDLE);
    assert_eq!(e.m.s.p[0].bar(), e.m.c[0].lp.bar());
    assert_eq!(e.m.s.p[0].rows, e.m.c[0].lp.rows);
    assert!(e.m.s.p[0].can_click);
}

#[test]
fn the_clicks_come_out_of_the_click_rate() {
    let mut p = params();
    p.cps_min = 10.0;
    p.cps_max = 10.0;
    let mut e = Env::new(Mode::SelfPlay, p, 8);
    run(&mut e, 9, |_, _| bar(HEAL_UP));
    let n = e.m.c[0].lp.potions.count_ones();
    assert!((4..=5).contains(&n), "at ten clicks a second, a potion every other tick: {n}");
    assert!(e.m.c[0].lp.inv_open);
}

#[test]
fn an_empty_hotbar_with_potions_in_the_rows_costs_until_one_is_got() {
    let mut p = params();
    p.w_stock = 2.0;
    // Nothing else is paid: the potions are thrown at full health.
    p.w_waste = 0.0;
    p.w_aim = 0.0;
    p.w_range = 0.0;
    p.w_close = 0.0;
    p.w_reach_adv = 0.0;
    let mut e = Env::new(Mode::SelfPlay, p, 9);
    assert_eq!(run(&mut e, 5, |_, _| IDLE), 0.0, "nothing changes while it stands there");
    let got = run(&mut e, 3, |_, k| if k == 0 { bar(HEAL_UP) } else { IDLE });
    assert!((got - 2.0).abs() < 1e-5, "paid once when the first potion is at hand: {got}");
    let more = run(&mut e, 5, |_, k| if k < 4 { bar(HEAL_UP) } else { IDLE });
    assert!(more.abs() < 1e-5, "more potions pay nothing: {more}");
    assert_eq!(e.m.c[0].lp.potions.count_ones(), 4);
    // Throwing them all takes it back when the last one goes.
    let mut paid = Vec::new();
    for _ in 0..4 {
        // With time for the server's own account of the hotbar to come back.
        paid.push(run(&mut e, 14, |_, k| if k == 0 { bar(THROW) } else { IDLE }));
    }
    assert_eq!(e.m.c[0].lp.potions, 0);
    assert!(paid[..3].iter().all(|r| r.abs() < 1e-5) && (paid[3] + 2.0).abs() < 1e-5, "{paid:?}");
}

#[test]
fn a_drink_pays_as_it_goes_and_giving_it_up_takes_it_back() {
    let mut p = params();
    p.refill_prob = 0.0;
    p.speed_end_prob = 1.0;
    p.speed_left_s = [0.0, 0.0];
    p.drinks = [2.0, 2.0];
    p.w_speed = 4.0;
    p.w_aim = 0.0;
    p.w_range = 0.0;
    p.w_close = 0.0;
    p.w_reach_adv = 0.0;
    let mut e = Env::new(Mode::SelfPlay, p, 10);
    // Half a drink: about half of it is paid.
    let half = run(&mut e, 20, |_, k| if k == 0 { bar(DRINK) } else { IDLE });
    assert!(half > 1.0 && half < 3.0, "{half}");
    // Given up: all of it is taken back.
    let back = run(&mut e, 12, |_, k| if k == 0 { bar(STOP) } else { IDLE });
    assert!((half + back).abs() < 1e-5, "{half} {back}");
    assert_eq!(e.m.s.p[0].speed_ticks, 0);
    // To the end: the whole of it, once. A second one, with all that
    // Speed left, is not taken and pays nothing.
    let all = run(&mut e, 50, |_, k| if k == 0 { bar(DRINK) } else { IDLE });
    assert!(e.m.s.p[0].speed_ticks > 0);
    assert!((all - 4.0).abs() < 1e-5, "{all}");
    let again = run(&mut e, 50, |_, k| if k == 0 { bar(DRINK) } else { IDLE });
    assert_eq!(e.episode().drinks[0], 1.0);
    assert_eq!(e.m.s.p[0].drinks.count_ones(), 1, "the second potion is kept");
    assert!(again.abs() < 1e-5, "{again}");
}

#[test]
fn a_lesson_starts_with_the_thing_to_learn_plainly_the_thing_to_do() {
    let mut p = Params::default();
    p.lesson_prob = 1.0;
    p.start_dist_min = 2.0;
    p.start_dist_max = 3.0;
    p.arena_half = 16.0;
    let far = |e: &Env| {
        let (a, b) = (e.m.c[0].lp.l.body.pos, e.m.c[1].lp.l.body.pos);
        ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    };
    for seed in 0..40 {
        // No Speed and a potion of it at hand.
        p.lessons = 1;
        let e = Env::new(Mode::Bot, p, seed);
        let (sp, lp) = (&e.m.s.p[0], &e.m.c[0].lp);
        assert_eq!(e.lesson, 1);
        assert!(!e.m.s.cfg.infinite_health);
        assert!(sp.speed_ticks == 0 && lp.speed_amp < 0 && sp.drinks != 0 && lp.drinks == sp.drinks, "seed {seed}");
        assert!(far(&e) > 9.0, "seed {seed}: {}", far(&e));
        // Too hungry to sprint, with steak.
        p.lessons = 2;
        let e = Env::new(Mode::Bot, p, seed);
        let (sp, lp) = (&e.m.s.p[0], &e.m.c[0].lp);
        assert!(sp.food.level <= 6 && lp.food == sp.food.level && sp.steak > 0 && lp.steak > 0, "seed {seed}");
        // Hurt, no healing potion at hand, some in the rows.
        p.lessons = 4;
        let e = Env::new(Mode::Bot, p, seed);
        let (sp, lp) = (&e.m.s.p[0], &e.m.c[0].lp);
        assert!(sp.potions == 0 && lp.potions == 0 && sp.rows.count(Item::Potion) >= 3 && lp.rows == sp.rows, "seed {seed}");
        assert!(sp.health <= 12.0 && lp.health == sp.health, "seed {seed}");
        // No Speed, and the potion of it is in the rows.
        p.lessons = 8;
        let e = Env::new(Mode::Bot, p, seed);
        let (sp, lp) = (&e.m.s.p[0], &e.m.c[0].lp);
        assert!(sp.speed_ticks == 0 && sp.drinks == 0 && sp.rows.count(Item::Drink) >= 1 && lp.rows == sp.rows, "seed {seed}");
    }
    // All four come up, and without lessons none does.
    p.lessons = 15;
    let mut seen = [0; 5];
    for seed in 0..80 {
        seen[Env::new(Mode::Bot, p, seed).lesson as usize] += 1;
    }
    assert!(seen[0] == 0 && seen[1..].iter().all(|&n| n > 8), "{seen:?}");
    p.lesson_prob = 0.0;
    assert_eq!(Env::new(Mode::Bot, p, 1).lesson, 0);
}

/// The extreme lesson: the first player hurt, with potions, and on top of
/// it a bot that hits hardly move, with Speed III to X, running rings
/// round it. Against scripted bots only.
#[test]
fn the_extreme_lesson_is_a_bot_that_nothing_gets_off() {
    let mut p = Params::default();
    p.lesson_prob = 1.0;
    p.lessons = 16;
    p.bot_level = [3.0, 4.0];
    let mut amps = [false; 10];
    for seed in 0..60 {
        let e = Env::new(Mode::Bot, p, seed);
        assert_eq!(e.lesson, 5);
        let (sp, lp) = (&e.m.s.p[0], &e.m.c[0].lp);
        assert!(!e.m.s.cfg.infinite_health);
        assert!((4.0..=12.0).contains(&sp.health) && lp.health == sp.health, "seed {seed}: {}", sp.health);
        assert!(sp.potions.count_ones() >= 3 && lp.potions == sp.potions, "seed {seed}");
        let (a, b) = (lp.l.body.pos, e.m.c[1].lp.l.body.pos);
        let d = (a[0] - b[0]).hypot(a[2] - b[2]);
        assert!(d < 5.5, "seed {seed}: {d}");
        let (op, oc) = (&e.m.s.p[1], &e.m.c[1]);
        let amp = op.l.attr.speed_amp;
        assert!((2..=9).contains(&amp) && oc.lp.speed_amp == amp && oc.lp.l.attr.speed_amp == amp, "seed {seed}: {amp}");
        assert!(oc.edge.kb_taken <= 0.2, "seed {seed}");
        assert!(e.bot.circler);
        // The first player has the Speed it would have had anyway.
        assert!(sp.l.attr.speed_amp <= 1);
        amps[amp as usize] = true;
    }
    assert!(amps[2..].iter().all(|&a| a), "{amps:?}");
    // Two players that both learn do not get it, with or without the others.
    assert_eq!(Env::new(Mode::SelfPlay, p, 1).lesson, 0);
    p.lessons = 31;
    let mut seen = [0; 6];
    for seed in 0..100 {
        assert!(Env::new(Mode::SelfPlay, p, seed).lesson <= 4);
        seen[Env::new(Mode::Bot, p, seed).lesson as usize] += 1;
    }
    assert!(seen[0] == 0 && seen[1..].iter().all(|&n| n > 8), "{seen:?}");
    // It can be given a share of its own among the lessons, the others
    // sharing the rest; two players that both learn still do not get it.
    p.extreme_share = 0.5;
    let mut seen = [0; 6];
    for seed in 0..200 {
        assert!(Env::new(Mode::SelfPlay, p, seed).lesson <= 4);
        seen[Env::new(Mode::Bot, p, seed).lesson as usize] += 1;
    }
    assert!(seen[0] == 0 && (80..=120).contains(&seen[5]) && seen[1..5].iter().all(|&n| n > 12), "{seen:?}");
    p.extreme_share = 1.0;
    assert!((0..40).all(|seed| Env::new(Mode::Bot, p, seed).lesson == 5));
    p.extreme_share = -1.0;

    // The bot goes round the first player, which only stands there and
    // is knocked about: in ten seconds it has been on most sides of it,
    // and never far.
    p.lessons = 16;
    p.potions = [0.0, 0.0];
    let mut round = 0;
    for seed in 0..6 {
        let mut e = Env::new(Mode::Bot, p, 500 + seed);
        // Enough health to last the ten seconds.
        e.m.s.cfg.infinite_health = true;
        let mut sides = [false; 8];
        let mut far = 0.0f64;
        for t in 0..200 {
            run(&mut e, 1, |_, _| IDLE);
            let (a, b) = (e.m.s.p[0].l.body.pos, e.m.s.p[1].l.body.pos);
            let ang = (b[2] - a[2]).atan2(b[0] - a[0]);
            sides[(((ang + std::f64::consts::PI) / std::f64::consts::TAU * 8.0) as usize).min(7)] = true;
            if t > 40 {
                far = far.max((a[0] - b[0]).hypot(a[2] - b[2]));
            }
        }
        let n = sides.iter().filter(|&&s| s).count();
        assert!(n >= 4, "seed {seed}: {sides:?}");
        assert!(far < 9.0, "seed {seed}: {far}");
        round += n;
    }
    assert!(round >= 36, "{round} of 48 sides seen");
}

#[test]
fn a_scripted_bot_goes_to_its_inventory_when_it_is_out_of_potions() {
    let mut p = params();
    p.bot_level = [4.0, 4.0];
    p.start_dist_min = 14.0;
    p.start_dist_max = 14.0;
    p.arena_half = 16.0;
    let mut got = 0;
    for seed in 0..10 {
        let mut e = Env::new(Mode::Bot, p, seed);
        assert_eq!(e.m.s.p[1].potions, 0);
        // The agent stands still, far away.
        run(&mut e, 200, |_, _| IDLE);
        let (sp, lp) = (&e.m.s.p[1], &e.m.c[1].lp);
        if sp.potions != 0 {
            got += 1;
            assert_eq!(sp.potions.count_ones() + sp.rows.count(Item::Potion), 10, "none lost on the way");
            assert_eq!(lp.potions.count_ones() + lp.rows.count(Item::Potion), 10);
            assert!(e.episode().refills[1] >= 1.0 && e.episode().inv_opens[1] >= 1.0);
        }
        assert!(sp.can_click);
        assert_eq!(e.m.c[1].lp.held, SLOT_SWORD);
    }
    assert!(got >= 8, "bots that refilled: {got} of 10");
    // A bot that is told not to never does.
    p.bot_refill = 0.0;
    let mut e = Env::new(Mode::Bot, p, 1);
    run(&mut e, 200, |_, _| IDLE);
    assert_eq!(e.m.s.p[1].rows, Rows({
        let mut r = [0u8; 27];
        r[..10].fill(1);
        r
    }));
}
