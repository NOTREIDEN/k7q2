//! Half turns of the view, what the opponent's last pearl tells, and what
//! it costs to heal the opponent, to fight on hungry and to stay slow.
use pvpsim::env::{Env, Mode, Params, ACT_DIMS, ACT_HEADS, FLICK_BINS, OBS_DIM, PRIV_DIM, YAW_BINS};

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0];
/// The pitch choice that looks down the fastest of the first set.
const DOWN: i32 = 26;

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
    p.w_aim = 0.0;
    p.w_range = 0.0;
    p.w_ontarget = 0.0;
    p.w_heal = 0.0;
    p.w_miss = 0.0;
    p.w_waste = 0.0;
    p
}

fn with(head: usize, v: i32) -> [i32; ACT_HEADS] {
    let mut a = IDLE;
    a[head] = v;
    a
}

/// Let both players make `n` ticks with the actions `f` gives for player 0
/// and `g` for player 1. Returns what player 0 was paid.
fn run2(e: &mut Env, n: usize, mut f: impl FnMut(usize) -> [i32; ACT_HEADS], mut g: impl FnMut(usize) -> [i32; ACT_HEADS]) -> f32 {
    let (mut fo, mut fp) = ([0.0f32; OBS_DIM], [0.0f32; PRIV_DIM]);
    let mut paid = 0.0;
    let (mut k, mut j) = (0, 0);
    while k < n {
        if e.pending == 0 {
            let a = f(k);
            e.step(&a);
            k += 1;
        } else {
            let a = g(j);
            e.step(&a);
            j += 1;
        }
        if e.pending == 0 {
            paid += e.take_outcome(&mut fo, &mut fp).0;
        } else if e.mode == Mode::SelfPlay {
            e.take_outcome(&mut fo, &mut fp);
        }
    }
    paid
}

fn run(e: &mut Env, n: usize, f: impl FnMut(usize) -> [i32; ACT_HEADS]) -> f32 {
    run2(e, n, f, |_| IDLE)
}

#[test]
fn a_half_turn_and_a_quarter_turn_take_one_tick() {
    assert_eq!((YAW_BINS[27], YAW_BINS[28], YAW_BINS[29], YAW_BINS[30]), (-600, 600, -1200, 1200));
    let mut e = Env::new(Mode::SelfPlay, params(), 1);
    let yaw = |e: &Env| e.m.c[0].lp.l.yaw;
    let at = yaw(&e);
    // The view turns between two ticks.
    let turned = |e: &Env, by: f32| assert!((yaw(e) - at - by).abs() < 1e-3, "{} from {at}, not by {by}", yaw(e));
    run(&mut e, 2, |k| with(5, [30, 13][k]));
    turned(&e, 180.0);
    run(&mut e, 2, |k| with(5, [29, 13][k]));
    turned(&e, 0.0);
    run(&mut e, 2, |k| with(5, [27, 13][k]));
    turned(&e, -90.0);
    run(&mut e, 2, |k| with(5, [28, 13][k]));
    turned(&e, 0.0);
}

#[test]
fn a_flick_is_added_to_the_yaw_of_its_tick() {
    assert_eq!(ACT_DIMS[10] as usize, FLICK_BINS.len());
    assert_eq!(FLICK_BINS, [0, -300, 300, -600, 600, 1200]);
    let mut e = Env::new(Mode::SelfPlay, params(), 1);
    let yaw = |e: &Env| e.m.c[0].lp.l.yaw;
    let mut at = yaw(&e);
    // Each flick by itself, with the mouse otherwise at rest: 0.15 degrees a count.
    for (choice, by) in [(1, -45.0), (2, 45.0), (3, -90.0), (4, 90.0), (5, 180.0), (0, 0.0)] {
        run(&mut e, 2, |k| with(10, [choice, 0][k]));
        assert!((yaw(&e) - at - by).abs() < 1e-3, "flick {choice}: {} from {at}, not by {by}", yaw(&e));
        at = yaw(&e);
    }
    // With a turn of the yaw control in the same tick the two add up:
    // a quarter turn and 21 counts more the same way.
    assert_eq!(YAW_BINS[20], 21);
    run(&mut e, 2, |k| {
        let mut a = with(10, [4, 0][k]);
        a[5] = [20, 13][k];
        a
    });
    assert!((yaw(&e) - at - 93.15).abs() < 1e-3, "{} from {at}", yaw(&e));
}

#[test]
fn an_agent_is_shown_how_long_the_opponent_is_without_a_pearl() {
    let mut p = params();
    p.pearl_prob = 1.0;
    p.pearl_cd_s = [16.0, 16.0];
    let mut e = Env::new(Mode::SelfPlay, p, 5);
    let mut o = [0.0f32; OBS_DIM];
    let shown = |e: &Env, id: usize, o: &mut [f32; OBS_DIM]| {
        e.write_obs(id, &mut o[..]);
        o[151]
    };
    run(&mut e, 5, |_| IDLE);
    assert_eq!((shown(&e, 0, &mut o), shown(&e, 1, &mut o)), (0.0, 0.0));
    // The other player throws one: within the pings it is seen, and from
    // then on the wait is counted down.
    run2(&mut e, 12, |_| IDLE, |j| if j == 0 { with(8, 1) } else { IDLE });
    let first = shown(&e, 0, &mut o);
    assert!(first > 0.95 && first < 1.0, "{first}");
    assert_eq!(shown(&e, 1, &mut o), 0.0);
    run(&mut e, 160, |_| IDLE);
    let later = shown(&e, 0, &mut o);
    assert!((first - later - 0.5).abs() < 1e-6, "{first} {later}");
    run(&mut e, 200, |_| IDLE);
    assert_eq!(shown(&e, 0, &mut o), 0.0);
}

#[test]
fn health_given_to_the_opponent_is_paid_for() {
    let paid = |w: f32| {
        let mut p = params();
        p.start_dist_min = 2.5;
        p.start_dist_max = 2.5;
        p.w_gift = w;
        let mut e = Env::new(Mode::SelfPlay, p, 3);
        let paid = run(&mut e, 2, |_| with(6, DOWN)) + run(&mut e, 1, |_| with(9, 1)) + run(&mut e, 45, |_| IDLE);
        let ep = e.episode();
        assert_eq!(ep.splashes[0], 1.0);
        (paid, ep.heal_gift[0] as f32, e.m.s.cfg.hit_health())
    };
    let (free, gift, hh) = paid(0.0);
    assert!(gift >= 2.0, "{gift}");
    let (three, same, _) = paid(3.0);
    assert_eq!(gift, same);
    assert!((three - free + 3.0 * gift / hh).abs() < 1e-4, "{three} {free} {gift}");
}

#[test]
fn fighting_on_hungry_costs_until_the_steak_is_eaten() {
    let w = 0.02f32;
    let duel = |food: f64| {
        let mut p = params();
        p.hunger_prob = 1.0;
        p.start_food = [food, food];
        p.w_hungry = w;
        Env::new(Mode::SelfPlay, p, 2)
    };
    // Food for 5 of 10 on the bar: 8 short of what heals, of the 12 between that and no sprint.
    let mut e = duel(10.0);
    assert_eq!(e.m.s.p[0].food.level, 10);
    // (A tick is paid for when the next one is asked for.)
    run(&mut e, 3, |_| IDLE);
    let paid = run(&mut e, 10, |_| IDLE);
    assert!((paid + 10.0 * w * (0.5 + 0.5 * 8.0 / 12.0)).abs() < 1e-5, "{paid}");
    // A steak is 8: enough, and nothing more is paid.
    run(&mut e, 45, |k| if k == 0 { with(9, 3) } else { IDLE });
    assert_eq!(e.m.s.p[0].food.level, 18);
    assert_eq!(run(&mut e, 10, |_| IDLE), 0.0);
    // With food enough nothing is paid from the start, nor without the weight.
    let mut e = duel(18.0);
    assert_eq!(run(&mut e, 10, |_| IDLE), 0.0);
    let mut p = params();
    p.hunger_prob = 1.0;
    p.start_food = [10.0, 10.0];
    let mut e = Env::new(Mode::SelfPlay, p, 2);
    assert_eq!(run(&mut e, 10, |_| IDLE), 0.0);
}

#[test]
fn staying_slow_with_a_potion_of_speed_at_hand_costs_until_it_is_drunk() {
    let w = 0.02f32;
    let mut p = params();
    p.lesson_prob = 1.0;
    p.lessons = 1;
    p.w_slow = w;
    let mut e = Env::new(Mode::SelfPlay, p, 4);
    assert_eq!(e.lesson, 1);
    assert!(e.m.s.p[0].speed_ticks <= 0 && e.m.s.p[0].drinks != 0);
    run(&mut e, 3, |_| IDLE);
    let paid = run(&mut e, 10, |_| IDLE);
    assert!((paid + 10.0 * w).abs() < 1e-5, "{paid}");
    run(&mut e, 45, |k| if k == 0 { with(9, 2) } else { IDLE });
    assert!(e.m.s.p[0].speed_ticks > 0);
    assert_eq!(run(&mut e, 10, |_| IDLE), 0.0);
}

fn lesson(mode: Mode, lessons: u32, seed: u64) -> Env {
    let mut p = Params::default();
    p.lesson_prob = 1.0;
    p.lessons = lessons;
    p.kb_random = 0.7;
    p.opp_edge_prob = 0.5;
    Env::new(mode, p, seed)
}

/// How far, in degrees, the view of player `me` is from the other player,
/// and how far apart the two are.
fn off_and_apart(e: &Env, me: usize) -> (f64, f64) {
    let (a, b) = (e.m.s.p[me].l.body.pos, e.m.s.p[1 - me].l.body.pos);
    let (to, _) = pvpsim::bot::angles_to(a, b);
    let off = pvpsim::mth::wrap_deg(e.m.s.p[me].l.yaw as f64 - to).abs();
    (off, (a[0] - b[0]).hypot(a[2] - b[2]))
}

#[test]
fn the_lesson_with_its_back_to_the_opponent() {
    for mode in [Mode::Bot, Mode::SelfPlay] {
        for seed in 0..200 {
            let e = lesson(mode, 128, seed);
            assert_eq!(e.lesson, 8);
            // Hurt, with potions, looking away from the one behind it, who
            // looks at it.
            let (off, d) = off_and_apart(&e, 0);
            assert!(off >= 150.0 && (3.0..=7.01).contains(&d), "{off} {d}");
            let (off, _) = off_and_apart(&e, 1);
            assert!(off <= 15.01, "{off}");
            let me = &e.m.s.p[0];
            assert!(!e.m.s.cfg.infinite_health && (4.0..=12.0).contains(&me.health) && me.potions.count_ones() >= 3);
        }
    }
}

#[test]
fn the_lesson_with_the_opponent_where_a_pearl_would_put_it() {
    for mode in [Mode::Bot, Mode::SelfPlay] {
        let (mut left, mut hurt) = (0, 0);
        for seed in 0..200 {
            let mut p = Params::default();
            p.lesson_prob = 1.0;
            p.lessons = 256;
            p.start_hurt = 0.0;
            let e = Env::new(mode, p, seed);
            assert_eq!(e.lesson, 9);
            // Next to it, to a side or behind, and looking at it.
            let (a, b) = (e.m.s.p[0].l.body.pos, e.m.s.p[1].l.body.pos);
            let (to, _) = pvpsim::bot::angles_to(a, b);
            let turn = pvpsim::mth::wrap_deg(e.m.s.p[0].l.yaw as f64 - to);
            let d = (a[0] - b[0]).hypot(a[2] - b[2]);
            assert!(turn.abs() >= 69.9 && (1.5..=4.51).contains(&d), "{turn} {d}");
            left += (turn < 0.0) as i32;
            let (off, _) = off_and_apart(&e, 1);
            assert!(off <= 15.01, "{off}");
            // A duel like any other for the rest: nobody is hurt for it.
            hurt += (e.m.s.p[0].health < 20.0) as i32;
        }
        assert!((60..=140).contains(&left), "{left}");
        assert_eq!(hurt, 0);
    }
}

#[test]
fn the_new_lessons_come_up_with_the_others() {
    let mut p = Params::default();
    p.lesson_prob = 1.0;
    p.lessons = 511;
    p.extreme_share = 0.75;
    for (mode, never) in [(Mode::Bot, 0), (Mode::SelfPlay, 5)] {
        let mut n = [0; 10];
        let mut e = Env::new(mode, p, 4);
        for _ in 0..4000 {
            n[e.lesson as usize] += 1;
            e.reset();
        }
        assert_eq!(n[0], 0);
        for k in 1..=9 {
            assert_eq!(n[k] == 0, k == never, "{n:?}");
        }
        let extreme: i32 = n[5..].iter().sum();
        assert!((2850..=3150).contains(&extreme), "{n:?}");
    }
    // Asked for as they were, the lessons are drawn as they were.
    p.lessons = 127;
    let mut e = Env::new(Mode::Bot, p, 4);
    for _ in 0..500 {
        assert!(e.lesson <= 7);
        e.reset();
    }
}

#[test]
fn potions_of_speed_start_in_the_inventory_for_the_share_asked() {
    let mut p = Params::default();
    p.infinite_health = false;
    p.speed_end_prob = 1.0;
    p.drinks = [1.0, 2.0];
    let (mut at_hand, mut stowed) = (0, 0);
    for seed in 0..300 {
        // Without the share, as it was: all of them in the hotbar.
        let e = Env::new(Mode::SelfPlay, p, seed);
        for me in &e.m.s.p {
            assert!(me.drinks != 0 && me.rows.0.iter().all(|&c| c != 2));
        }
        let mut q = p;
        q.drinks_stowed = 0.6;
        let e = Env::new(Mode::SelfPlay, q, seed);
        for (me, own) in e.m.s.p.iter().zip(&e.m.c) {
            let up = me.rows.0.iter().filter(|&&c| c == 2).count();
            // All of a player's in one place or the other, and its client
            // knows the same.
            assert!((me.drinks != 0) != (up > 0) && (1..=2).contains(&(me.drinks.count_ones() as usize + up)));
            assert_eq!(own.lp.rows.0, me.rows.0);
            at_hand += (up == 0) as i32;
            stowed += (up > 0) as i32;
        }
    }
    assert!((300..=420).contains(&stowed) && at_hand + stowed == 600, "{at_hand} {stowed}");
}
