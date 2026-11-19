//! Half turns of the view, what the opponent's last pearl tells, and what
//! it costs to heal the opponent, to fight on hungry and to stay slow.
use pvpsim::agent::{throw_pitch, THROW_PITCHES};
use pvpsim::env::{Env, Mode, Params, ACT_DIMS, ACT_HEADS, BAR_TURN, BAR_TURNS, FLICK_BINS, OBS_DIM, PRIV_DIM, YAW_BINS};
use pvpsim::net::Kind;
use pvpsim::server::throw_heading;

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];
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
fn a_potion_thrown_with_a_turn_takes_the_pitch_asked_for_and_the_rest_is_the_policys() {
    assert_eq!(ACT_DIMS[9], 13);
    assert_eq!(ACT_DIMS[11], THROW_PITCHES as i32);
    assert_eq!((BAR_TURN, BAR_TURNS), (7, [1200, 600, -600]));
    assert_eq!((throw_pitch(0), throw_pitch(10), throw_pitch(35), throw_pitch(99)), (90.0, 70.0, 20.0, 20.0));
    let yaw = |e: &Env| e.m.c[0].lp.l.yaw;
    let pitch = |e: &Env| e.m.c[0].lp.l.pitch;
    let shown = |e: &Env| {
        let mut o = [0.0f32; OBS_DIM];
        e.write_obs(0, &mut o[..]);
        o[152]
    };
    for (k, turn) in [(0usize, 180.0f32), (1, 90.0), (2, -90.0)] {
        for choice in [0, 10, 35] {
            let mut e = Env::new(Mode::SelfPlay, params(), 3);
            let at = yaw(&e);
            assert_eq!(shown(&e), 0.0);
            // The tick of the choice turns the view, sets the pitch asked
            // for and takes the potion in hand; nothing is thrown with it.
            run(&mut e, 1, |_| {
                let mut a = with(9, BAR_TURN + k as i32);
                a[11] = choice;
                a
            });
            assert!((yaw(&e) - at - turn).abs() < 1e-3, "{} from {at}", yaw(&e));
            let (ty, tp) = (yaw(&e), pitch(&e));
            assert!((tp - throw_pitch(choice)).abs() <= 0.08, "{tp} for {}", throw_pitch(choice));
            assert_eq!(e.m.s.potion_throws[0], 0);
            assert!(shown(&e) > 0.0);
            // The click; the mouse and the keys are the policy's, and
            // nothing else of the hotbar is done on it.
            let busy = |_| {
                let mut a = with(5, 26);
                a[6] = 0;
                a[10] = 5;
                a[9] = 5;
                a[1] = 1;
                a
            };
            run(&mut e, 1, busy);
            let want = (YAW_BINS[26] + FLICK_BINS[5]) as f32 * 0.15;
            assert!((yaw(&e) - ty - want).abs() < 1e-2, "{} from {ty}", yaw(&e));
            assert!(pitch(&e) < tp);
            assert!(!e.m.c[0].lp.inv_open);
            // The potion goes along the view of the turn, as the server had it.
            let mut aim = None;
            for _ in 0..12 {
                run(&mut e, 1, |_| IDLE);
                if aim.is_none() && e.m.s.potion_throws[0] == 1 {
                    aim = e.m.s.thrown.iter().find(|t| t.thrower == 0).map(|t| t.aim);
                }
            }
            let aim = aim.expect("the potion was seen when thrown");
            let (_, dir) = throw_heading(Kind::Potion, e.m.s.p[0].l.body.pos, ty, tp);
            for i in 0..3 {
                assert!((aim[i] - dir[i] * Kind::Potion.speed() as f64).abs() < 1e-6, "{aim:?} against {dir:?}");
            }
            // The observation of the turn counts down and is gone.
            run(&mut e, 4, |_| IDLE);
            assert_eq!(shown(&e), 0.0);
            assert_eq!(e.m.s.potion_throws[0], 1);
        }
    }
    // The plain throw of the same start does not turn.
    let mut e = Env::new(Mode::SelfPlay, params(), 3);
    let at = yaw(&e);
    run(&mut e, 1, |_| with(9, 1));
    run(&mut e, 12, |_| IDLE);
    assert!((yaw(&e) - at).abs() < 1e-3);
    assert_eq!(e.m.s.potion_throws[0], 1);
}

#[test]
fn two_potions_thrown_with_one_turn_go_on_two_ticks_in_a_row_along_the_same_view() {
    let yaw = |e: &Env| e.m.c[0].lp.l.yaw;
    let pitch = |e: &Env| e.m.c[0].lp.l.pitch;
    for (k, turn) in [(0usize, 180.0f32), (1, 90.0), (2, -90.0)] {
        let mut e = Env::new(Mode::SelfPlay, params(), 3);
        let at = yaw(&e);
        let potions = e.m.c[0].lp.potions.count_ones();
        run(&mut e, 1, |_| {
            let mut a = with(9, BAR_TURN + BAR_TURNS.len() as i32 + k as i32);
            a[11] = 4;
            a
        });
        assert!((yaw(&e) - at - turn).abs() < 1e-3, "{} from {at}", yaw(&e));
        let (ty, tp) = (yaw(&e), pitch(&e));
        assert!((tp - throw_pitch(4)).abs() <= 0.08);
        // The first click, then the second with the next potion: whatever
        // the policy asks of the mouse and the hotbar on that tick, the
        // view stays where the first one went.
        run(&mut e, 1, |_| IDLE);
        run(&mut e, 1, |_| {
            let mut a = with(5, 26);
            a[6] = 0;
            a[10] = 5;
            a[9] = 5;
            a
        });
        assert!((yaw(&e) - ty).abs() < 1e-3 && (pitch(&e) - tp).abs() < 1e-3, "{} {} from {ty} {tp}", yaw(&e), pitch(&e));
        assert_eq!(e.m.c[0].lp.potions.count_ones(), potions - 2);
        let mut ticks = Vec::new();
        for t in 0..12 {
            run(&mut e, 1, |_| IDLE);
            while ticks.len() < e.m.s.potion_throws[0] as usize {
                ticks.push(t);
            }
            let (_, dir) = throw_heading(Kind::Potion, e.m.s.p[0].l.body.pos, ty, tp);
            for th in e.m.s.thrown.iter().filter(|t| t.thrower == 0) {
                // Both go along the view of the turn (the player stood still).
                for i in 0..3 {
                    assert!((th.aim[i] - dir[i] * Kind::Potion.speed() as f64).abs() < 1e-6, "{:?} against {dir:?}", th.aim);
                }
            }
        }
        assert_eq!(ticks.len(), 2, "{ticks:?}");
        assert_eq!(ticks[1], ticks[0] + 1, "{ticks:?}");
        // And the sword is back in hand.
        assert_eq!(e.m.c[0].lp.held, 0);
    }
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
fn the_ticks_of_a_player_that_goes_for_potions_are_marked_as_not_its_policys() {
    for steal in [0.0, 1.0] {
        let mut p = params();
        p.league_steal = steal;
        let mut e = Env::new(Mode::SelfPlay, p, 5);
        assert!(e.m.c[1].rp.health <= 12.0);
        let (mut fo, mut fp) = ([0.0f32; OBS_DIM], [0.0f32; PRIV_DIM]);
        let mut marked = 0;
        for _ in 0..20 {
            let id = e.pending;
            e.step(&IDLE);
            e.take_outcome(&mut fo, &mut fp);
            if id == 1 {
                marked += e.overridden as i32;
            } else {
                assert!(!e.overridden);
            }
        }
        assert_eq!(marked, if steal > 0.0 { 10 } else { 0 });
    }
}

#[test]
fn for_every_turn_a_pitch_that_heals_all_eight_is_there_to_learn() {
    // On a server that throws as the view does and on one that adds the
    // thrower's movement: the player sprints on along the turned view, and
    // some pitch of the last control heals all eight.
    for carry in [0.0, 1.0] {
        for turn in 0..3 {
            let mut found = 0;
            for seed in 0..10 {
                let mut best = f32::MIN;
                for choice in 0..THROW_PITCHES as i32 {
                    let mut p = params();
                    p.start_yaw_noise = 25.0;
                    p.throw_carry_prob = carry;
                    let mut e = Env::new(Mode::SelfPlay, p, seed);
                    let health = e.m.s.p[0].health;
                    let on = |k: usize| {
                        let mut a = with(0, 1);
                        a[3] = 1;
                        if k == 0 {
                            a[9] = BAR_TURN + turn;
                            a[11] = choice;
                        }
                        a
                    };
                    run(&mut e, 30, on);
                    assert_eq!(e.m.s.potion_throws[0], 1);
                    best = best.max(e.m.s.p[0].health - health);
                }
                found += (best >= 8.0) as i32;
            }
            assert!(found >= 9, "carry {carry} turn {turn}: {found} of 10");
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

#[test]
fn the_teacher_of_turned_throws_says_what_it_would_do_and_plays_nothing() {
    let mut p = params();
    p.throw_carry_prob = 1.0;
    let mut taught = p;
    taught.teach = 1.0;
    let (mut a, mut b) = (Env::new(Mode::SelfPlay, p, 4), Env::new(Mode::SelfPlay, taught, 4));
    let act = |k: usize| {
        let mut x = with(0, 1);
        x[3] = 1;
        if k == 3 {
            x[9] = BAR_TURN;
            x[11] = 10;
        }
        x
    };
    let mut seen = vec![];
    for k in 0..30 {
        run(&mut a, 1, |_| act(k));
        run(&mut b, 1, |_| act(k));
        // The same play either way.
        for i in 0..2 {
            assert_eq!(a.m.c[i].lp.l.body.pos, b.m.c[i].lp.l.body.pos);
            assert_eq!((a.m.c[i].lp.l.yaw, a.m.c[i].lp.l.pitch), (b.m.c[i].lp.l.yaw, b.m.c[i].lp.l.pitch));
            assert_eq!(a.m.s.p[i].health, b.m.s.p[i].health);
        }
        assert_eq!(a.teach, [-1; ACT_HEADS]);
        seen.push(b.teach);
    }
    let none = [-1; ACT_HEADS];
    assert!(seen[..3].iter().all(|t| *t == none));
    // The turn: a pitch, and sprinting along the view with the mouse still.
    let t = seen[3];
    assert!((0..THROW_PITCHES as i32).contains(&t[11]), "{t:?}");
    assert_eq!((t[0], t[1], t[2], t[3], t[5], t[10]), (1, 0, 0, 1, 13, 0));
    // The click: the same, the pitch still too.
    assert_eq!((seen[4][0], seen[4][3], seen[4][5], seen[4][6], seen[4][10], seen[4][11]), (1, 1, 13, 10, 0, -1));
    // Then the view turns back by the half turn and it walks the way it ran.
    assert_eq!(FLICK_BINS[seen[5][10] as usize], 1200);
    assert!(seen[5][0] >= 0 && seen[5][3] == 0);
    let last = (5..30).rev().find(|&k| seen[k] != none).expect("it says how to walk");
    assert!(last <= 3 + 14, "{last}");
    assert!(seen[last + 1..].iter().all(|t| *t == none));
}

#[test]
fn the_insane_bot_pots_onto_itself() {
    let (mut heal, mut splashes) = (0.0, 0.0);
    for seed in 0..12 {
        let mut p = params();
        p.insane_prob = 1.0;
        p.start_health = [20.0, 20.0];
        p.start_hurt = 0.0;
        p.start_dist_min = 14.0;
        p.start_dist_max = 14.0;
        p.throw_carry_prob = if seed % 2 == 0 { 1.0 } else { 0.0 };
        let mut e = Env::new(Mode::Bot, p, seed);
        // Hurt, far from an opponent that stands still.
        e.m.s.p[1].health = 3.0;
        e.m.c[1].lp.health = 3.0;
        e.m.c[0].rp.health = 3.0;
        run(&mut e, 160, |_| IDLE);
        heal += e.episode().heal_self[1];
        splashes += e.episode().splashes[1];
    }
    assert!(splashes >= 12.0, "{splashes}");
    assert!(heal / splashes >= 7.0, "{} a potion over {splashes}", heal / splashes);
}
