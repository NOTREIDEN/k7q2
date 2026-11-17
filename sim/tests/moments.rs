//! Half turns of the view, what the opponent's last pearl tells, and what
//! it costs to heal the opponent, to fight on hungry and to stay slow.
use pvpsim::env::{Env, Mode, Params, ACT_DIMS, ACT_HEADS, BAR_TURN, BAR_TURNS, FLICK_BINS, OBS_DIM, PRIV_DIM, YAW_BINS};

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
fn a_potion_thrown_with_a_turn_lands_on_the_runner_and_the_view_comes_back() {
    assert_eq!(ACT_DIMS[9], 10);
    assert_eq!((BAR_TURN, BAR_TURNS), (7, [1200, 600, -600]));
    let yaw = |e: &Env| e.m.c[0].lp.l.yaw;
    let pitch = |e: &Env| e.m.c[0].lp.l.pitch;
    let shown = |e: &Env| {
        let mut o = [0.0f32; OBS_DIM];
        e.write_obs(0, &mut o[..]);
        o[152]
    };
    for (k, turn) in [(0usize, 180.0f32), (1, 90.0), (2, -90.0)] {
        let mut e = Env::new(Mode::SelfPlay, params(), 3);
        let (at, up) = (yaw(&e), pitch(&e));
        let start = e.m.s.p[0].l.body.pos;
        let (health, other) = (e.m.s.p[0].health, e.m.s.p[1].health);
        assert_eq!(shown(&e), 0.0);
        // The tick of the choice turns the view, looks down and takes the
        // potion in hand; nothing is thrown with it.
        run(&mut e, 1, |_| with(9, BAR_TURN + k as i32));
        assert!((yaw(&e) - at - turn).abs() < 1e-3, "{} from {at}", yaw(&e));
        let down = pitch(&e);
        assert!(down > 20.0 && down <= 90.0, "{down}");
        assert_eq!(e.m.s.potion_throws[0], 0);
        assert!(shown(&e) > 0.0);
        // The click, whatever is asked of the mouse or the inventory.
        let busy = |_| {
            let mut a = with(5, 26);
            a[6] = 0;
            a[10] = 5;
            a[9] = 5;
            a[1] = 1;
            a
        };
        run(&mut e, 1, busy);
        assert!((yaw(&e) - at - turn).abs() < 1e-3 && pitch(&e) == down);
        // The next tick turns the view back, and from then on the mouse is
        // the policy's while the player keeps going the way it ran.
        run(&mut e, 1, |_| IDLE);
        assert!((yaw(&e) - at).abs() < 1e-3 && (pitch(&e) - up).abs() < 0.2, "{} {} from {at} {up}", yaw(&e), pitch(&e));
        let mut ticks = 3;
        while shown(&e) > 0.0 {
            let was = yaw(&e);
            run(&mut e, 1, busy);
            ticks += 1;
            let want = (YAW_BINS[26] + FLICK_BINS[5]) as f32 * 0.15;
            assert!((yaw(&e) - was - want).abs() < 1e-2, "tick {ticks}: {} from {was}", yaw(&e));
            assert!(!e.m.c[0].lp.inv_open);
            assert!(ticks < 18);
        }
        run(&mut e, 6, |_| IDLE);
        let pos = e.m.s.p[0].l.body.pos;
        let ran = (pos[0] - start[0]).hypot(pos[2] - start[2]);
        assert_eq!(e.m.s.potion_throws[0], 1);
        assert_eq!(e.m.c[0].lp.potions.count_ones(), 6);
        // It came down on the player that ran, and not on the one far away;
        // with the mouse spun this far the keys follow the run only to the
        // nearest of eight ways (see the next test for a still mouse).
        assert!(e.m.s.p[0].health >= health + 7.0, "turn {turn}: {health} to {} after {ticks} ticks and {ran:.1} blocks", e.m.s.p[0].health);
        assert_eq!(e.m.s.p[1].health, other);
        assert!(ran > 1.0, "{ran}");
    }
    // A potion asked for on the tick after the click follows the first
    // along the same view, and the view comes back a tick later.
    let mut e = Env::new(Mode::SelfPlay, params(), 3);
    let at = yaw(&e);
    run(&mut e, 2, |_| with(9, BAR_TURN));
    run(&mut e, 1, |_| with(9, 1));
    assert!((yaw(&e) - at - 180.0).abs() < 1e-3);
    run(&mut e, 1, |_| IDLE);
    assert!((yaw(&e) - at).abs() < 1e-3);
    run(&mut e, 20, |_| IDLE);
    assert_eq!(e.m.s.potion_throws[0], 2);
    // The plain throw of the same start does not turn.
    let mut e = Env::new(Mode::SelfPlay, params(), 3);
    run(&mut e, 1, |_| with(9, 1));
    run(&mut e, 12, |_| IDLE);
    assert!((yaw(&e) - at).abs() < 1e-3);
    assert_eq!(e.m.s.potion_throws[0], 1);
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

#[test]
fn getting_potions_down_with_the_opponent_close_costs_and_far_does_not() {
    let w = 0.3f32;
    let mut paid = Vec::new();
    for dist in [12.0, 2.5] {
        let mut p = params();
        p.refill_prob = 1.0;
        p.w_inv_near = w;
        p.start_dist_min = dist;
        p.start_dist_max = dist;
        let mut e = Env::new(Mode::SelfPlay, p, 4);
        let apart = {
            let (a, b) = (e.m.s.p[0].l.body.pos, e.m.s.p[1].l.body.pos);
            (a[0] - b[0]).hypot(a[2] - b[2])
        };
        assert!(e.m.c[0].lp.rows.count(pvpsim::net::Item::Potion) > 0);
        run(&mut e, 3, |_| IDLE);
        let mut open = 0;
        let mut got = 0.0;
        for _ in 0..6 {
            got += run(&mut e, 1, |_| with(9, 5));
            open += e.m.c[0].lp.inv_open as i32;
        }
        paid.push((apart, got, open));
    }
    let (far, near) = (paid[0], paid[1]);
    assert!(far.0 > 4.5 && near.0 < 4.5, "{paid:?}");
    assert!(far.2 >= 1 && near.2 >= 1, "{paid:?}");
    assert!((near.1 - far.1 + w * near.2 as f32).abs() < 1e-4, "{paid:?}");
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

#[test]
fn a_bot_that_presses_does_not_stop_to_heal_itself_and_jumps_at_a_hurt_opponent() {
    let throws = |press: f64, seed: u64| {
        let mut p = params();
        p.bot_level = [4.0, 4.0];
        p.start_health = [8.0, 8.0];
        p.bot_presser = press;
        let mut e = Env::new(Mode::Bot, p, seed);
        assert_eq!(e.bot.presser, press > 0.0);
        let mut jumped = false;
        for _ in 0..30 {
            run(&mut e, 1, |_| IDLE);
            jumped |= !e.m.s.p[1].l.body.on_ground;
        }
        (e.m.s.potion_throws[1], jumped)
    };
    // Both at eight points of health, twelve blocks apart, the agent standing still.
    let (mut plain, mut pressing, mut jumps) = (0, 0, 0);
    for seed in 1..=8 {
        plain += throws(0.0, seed).0;
        let (t, j) = throws(1.0, seed);
        pressing += t;
        jumps += j as u32;
    }
    assert!(plain >= 3, "bots that do not press heal themselves at that health: {plain} potions in 8 duels");
    assert_eq!(pressing, 0);
    assert_eq!(jumps, 8);
}

#[test]
fn a_player_that_goes_for_potions_runs_at_a_hurt_opponent_and_a_plain_one_does_not() {
    let mut apart = Vec::new();
    for steal in [0.0, 1.0] {
        let mut p = params();
        p.league_steal = steal;
        let mut e = Env::new(Mode::SelfPlay, p, 5);
        assert!(e.m.c[1].rp.health <= 12.0);
        let (_, at) = off_and_apart(&e, 1);
        // Ticks until it is within reach, and how far its view is off then.
        let mut reach = (0, 0.0);
        for k in 0..40 {
            run(&mut e, 1, |_| IDLE);
            let (off, now) = off_and_apart(&e, 1);
            if now < 3.0 {
                reach = (k + 1, off);
                break;
            }
        }
        apart.push((at, off_and_apart(&e, 1).1, reach));
    }
    let (plain, keen) = (apart[0], apart[1]);
    assert!((plain.0 - plain.1).abs() < 0.5 && plain.2 .0 == 0, "{apart:?}");
    // Nine blocks in a sprint with jumps, a second and a half.
    assert!((1..=35).contains(&keen.2 .0) && keen.2 .1 < 10.0, "{apart:?}");
}

#[test]
fn a_potion_thrown_with_a_turn_and_the_mouse_still_heals_all_eight() {
    // On a server that throws as the view does and on one that adds the
    // thrower's movement, known; and not yet known, the most of both.
    for (carry, knows) in [(0.0, Some(false)), (1.0, Some(true)), (0.0, None), (1.0, None)] {
        for turn in 0..3 {
            let (mut full, mut least) = (0, f32::MAX);
            for seed in 0..20 {
                let mut p = params();
                p.start_yaw_noise = 25.0;
                p.throw_carry_prob = carry;
                let mut e = Env::new(Mode::SelfPlay, p, seed);
                if let Some(k) = knows {
                    e.agent_mut(0).knows_carry(k);
                }
                let health = e.m.s.p[0].health;
                run(&mut e, 1, |_| with(9, BAR_TURN + turn));
                run(&mut e, 30, |_| IDLE);
                assert_eq!(e.m.s.potion_throws[0], 1);
                let gain = e.m.s.p[0].health - health;
                full += (gain >= 8.0) as i32;
                least = least.min(gain);
            }
            let sure = if knows.is_some() || carry == 1.0 { 19 } else { 0 };
            assert!(full >= sure && least >= 6.0, "carry {carry} {knows:?} turn {turn}: {full} of 20, least {least}");
        }
    }
}

#[test]
fn on_a_server_that_carries_throws_a_potion_thrown_running_flies_on_and_the_agent_sees_it() {
    let mut out = Vec::new();
    for carry in [0.0, 1.0] {
        let mut p = params();
        p.throw_carry_prob = carry;
        let mut e = Env::new(Mode::SelfPlay, p, 6);
        assert_eq!(e.m.s.cfg.throw_carry, carry > 0.0);
        let seen = |e: &Env| {
            let mut o = [0.0f32; OBS_DIM];
            e.write_obs(0, &mut o[..]);
            o[153]
        };
        assert_eq!(seen(&e), 0.5);
        // Sprinting ahead with the view down at the feet.
        let run_down = |k: usize| {
            let mut a = with(0, 1);
            a[3] = 1;
            if k < 4 {
                a[6] = DOWN;
            }
            a
        };
        run(&mut e, 14, run_down);
        let yaw = (e.m.s.p[0].l.yaw as f64).to_radians();
        let ahead = [-yaw.sin(), yaw.cos()];
        let at = e.m.s.p[0].l.body.pos;
        let thrown = e.m.s.potion_throws[0];
        run(&mut e, 1, |_| {
            let mut a = run_down(9);
            a[9] = 1;
            a
        });
        // Where the potion was last seen before it broke, ahead of where it went out.
        let mut last = None;
        for _ in 0..12 {
            run(&mut e, 1, |_| run_down(9));
            if let Some(t) = e.m.s.thrown.iter().find(|t| t.thrower == 0) {
                last = Some(t.pos);
            }
        }
        assert_eq!(e.m.s.potion_throws[0], thrown + 1);
        let end = last.expect("the potion was seen in the air");
        let went = (end[0] - at[0]) * ahead[0] + (end[2] - at[2]) * ahead[1];
        out.push((seen(&e), went));
    }
    let (plain, carried) = (out[0], out[1]);
    assert_eq!((plain.0, carried.0), (0.0, 1.0), "{out:?}");
    assert!(carried.1 > plain.1 + 0.5, "{out:?}");
}
