//! Real health and healing potions as the trainer uses them: the kit, the
//! throw action of the environment, what an agent is told, how a duel ends,
//! and scripted bots that heal themselves. How a potion flies and heals is
//! checked against the real game in `parity.rs` and in `potions.rs`.
use pvpsim::bot::{potion_pitch, potion_reach};
use pvpsim::env::{Env, Mode, Params, ACT_DIMS, ACT_HEADS, OBS_DIM, PITCH_BINS, PRIV_DIM, YAW_BINS};
use pvpsim::net::SLOT_SWORD;

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0];
/// The pitch choice that looks down the fastest of the first set.
const DOWN: i32 = 26;
/// A quarter turn up and down in one tick.
const FLICK_UP: i32 = 27;
const FLICK_DOWN: i32 = 28;

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
    p
}

/// Let player 0 make `n` ticks with the actions `f` gives for each of them,
/// while player 1 stands still. Returns what player 0 was paid and the
/// last end-of-duel flag it was given.
fn run(e: &mut Env, n: usize, mut f: impl FnMut(&Env, usize) -> [i32; ACT_HEADS]) -> (f32, u8) {
    let (mut fo, mut fp) = ([0.0f32; OBS_DIM], [0.0f32; PRIV_DIM]);
    let (mut paid, mut flag) = (0.0, 0);
    let mut k = 0;
    while k < n {
        let me = e.pending == 0;
        if me {
            let a = f(e, k);
            e.step(&a);
            k += 1;
        } else {
            e.step(&IDLE);
        }
        if e.pending == 0 {
            let (r, d) = e.take_outcome(&mut fo, &mut fp);
            paid += r;
            if d != 0 {
                flag = d;
            }
        } else if e.mode == Mode::SelfPlay {
            e.take_outcome(&mut fo, &mut fp);
        }
    }
    (paid, flag)
}

fn with(head: usize, v: i32) -> [i32; ACT_HEADS] {
    let mut a = IDLE;
    a[head] = v;
    a
}

fn obs(e: &Env) -> [f32; OBS_DIM] {
    let mut o = [0.0f32; OBS_DIM];
    e.write_obs(0, &mut o);
    o
}

#[test]
fn the_controls_are_what_the_trainer_is_told() {
    assert_eq!(ACT_DIMS, [3, 3, 2, 2, 2, 31, 29, 2, 2, 7, 6]);
    assert_eq!(YAW_BINS[13], 0);
    assert_eq!(&YAW_BINS[..27], &[-377, -233, -144, -89, -55, -34, -21, -13, -8, -5, -3, -2, -1, 0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377]);
    assert_eq!(PITCH_BINS[10], 0);
    assert_eq!(PITCH_BINS[DOWN as usize], 377);
    // The choices the first networks were trained with are where they were.
    assert_eq!(&PITCH_BINS[..21], &[-89, -55, -34, -21, -13, -8, -5, -3, -2, -1, 0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89]);
}

#[test]
fn a_potion_takes_the_crosshair_off_the_opponent_for_one_tick() {
    assert_eq!((PITCH_BINS[FLICK_UP as usize], PITCH_BINS[FLICK_DOWN as usize]), (-600, 600));
    let mut e = Env::new(Mode::SelfPlay, params(), 1);
    run(&mut e, 5, |_, _| IDLE);
    // One tick to look straight down. On the tick of the throw the view
    // is already back on the other player, because the server throws
    // along the view it was sent on the tick before.
    run(&mut e, 1, |_, _| with(6, FLICK_DOWN));
    assert_eq!(e.m.c[0].lp.l.pitch, 90.0);
    run(&mut e, 1, |_, _| {
        let mut a = with(9, 1);
        a[6] = FLICK_UP;
        a
    });
    assert!(e.m.c[0].lp.l.pitch.abs() < 0.01, "{}", e.m.c[0].lp.l.pitch);
    assert_eq!(e.m.c[0].lp.potions.count_ones(), 6);
    run(&mut e, 1, |_, _| IDLE);
    assert_eq!(e.m.c[0].lp.held, SLOT_SWORD);
    run(&mut e, 30, |_, _| IDLE);
    assert_eq!(e.m.s.p[0].health, 13.0, "thrown straight down all the same");
    let ep = e.episode();
    assert_eq!((ep.potions, ep.pot_away, ep.pot_double, ep.pot_health, ep.pot_perfect, ep.pot_hit), ([1.0, 0.0], [1.0, 0.0], [0.0; 2], [6.0, 0.0], [0.0; 2], [0.0; 2]));
}

#[test]
fn two_potions_on_two_ticks_take_two_ticks_of_looking_away() {
    let mut e = Env::new(Mode::SelfPlay, params(), 1);
    run(&mut e, 5, |_, _| IDLE);
    // Down; throw; throw the next one and look up; the sword. The second
    // click comes on the tick after the first: the button is let go and
    // pressed again between the two.
    run(&mut e, 1, |_, _| with(6, FLICK_DOWN));
    run(&mut e, 1, |_, _| with(9, 1));
    assert_eq!(e.m.c[0].lp.potions.count_ones(), 6);
    assert_eq!(obs(&e)[103], 1.0, "the next one can be asked for at once");
    run(&mut e, 1, |_, _| {
        let mut a = with(9, 1);
        a[6] = FLICK_UP;
        a
    });
    assert_eq!(e.m.c[0].lp.potions.count_ones(), 5);
    run(&mut e, 1, |_, _| IDLE);
    assert_eq!(e.m.c[0].lp.held, SLOT_SWORD);
    run(&mut e, 30, |_, _| IDLE);
    assert_eq!(e.m.s.potion_throws, [2, 0]);
    assert_eq!(e.m.s.p[0].health, 20.0, "six and twice seven");
    let ep = e.episode();
    assert_eq!((ep.potions, ep.splashes, ep.pot_away, ep.pot_double, ep.pot_health), ([2.0, 0.0], [2.0, 0.0], [2.0, 0.0], [1.0, 0.0], [12.0, 0.0]));
}

#[test]
fn three_potions_in_a_row_and_no_more_than_asked() {
    let mut e = Env::new(Mode::SelfPlay, params(), 1);
    e.m.s.p[0].health = 1.0;
    run(&mut e, 5, |_, _| IDLE);
    run(&mut e, 1, |_, _| with(6, FLICK_DOWN));
    run(&mut e, 3, |_, _| with(9, 1));
    run(&mut e, 1, |_, _| IDLE);
    assert_eq!(e.m.c[0].lp.held, SLOT_SWORD);
    run(&mut e, 30, |_, _| IDLE);
    assert_eq!(e.m.s.potion_throws, [3, 0]);
    assert_eq!(e.m.c[0].lp.potions.count_ones(), 4);
    assert_eq!(e.m.s.p[0].health, 20.0);
}

#[test]
fn the_kit_and_the_start_are_drawn_as_asked() {
    let e = Env::new(Mode::SelfPlay, params(), 1);
    let c = &e.m.s.cfg;
    assert!(!c.infinite_health);
    assert_eq!((c.armor, c.prot, c.feather, c.ench_damage), (20, 8, 18, 2.5));
    // Sharpness II through diamond armour with Protection II: 2.1 less a
    // share of 4 to 8 twenty-fifths, 1.6 on average.
    assert!((c.hit_health() - 1.596).abs() < 1e-3, "{}", c.hit_health());
    for i in 0..2 {
        assert_eq!((e.m.s.p[i].health, e.m.c[i].lp.health, e.m.c[1 - i].rp.health), (6.0, 6.0, 6.0));
        assert_eq!((e.m.s.p[i].potions.count_ones(), e.m.c[i].lp.potions.count_ones()), (7, 7));
    }
    // Without real health nothing of it is there.
    let e = Env::new(Mode::SelfPlay, Params::default(), 1);
    assert!(e.m.s.cfg.infinite_health);
    assert_eq!((e.m.s.p[0].potions, e.m.s.p[0].health, e.m.s.cfg.armor), (0, 20.0, 0));
    let o = obs(&e);
    assert_eq!((o[96], o[97], o[98], o[102], o[103]), (0.0, 1.0, 1.0, 0.0, 0.0));
}

#[test]
fn the_throw_action_throws_one_potion_and_takes_the_sword_again() {
    let mut e = Env::new(Mode::SelfPlay, params(), 1);
    run(&mut e, 5, |_, _| IDLE);
    // Two ticks to look at the ground.
    run(&mut e, 2, |_, _| with(6, DOWN));
    assert!(e.m.c[0].lp.l.pitch > 89.0);
    let o = obs(&e);
    assert_eq!((o[96], o[97], o[102], o[103]), (1.0, 0.3, 1.0, 1.0));
    // Told beforehand: it breaks here in two ticks and gives seven.
    assert!(o[122].abs() < 0.1 && o[123].abs() < 0.1, "{} {}", o[122], o[123]);
    assert_eq!((o[124], o[125], o[126], o[127]), (0.1, 0.875, 0.875, 0.0));
    let (before, _) = run(&mut e, 1, |_, _| with(9, 1));
    assert_ne!(e.m.c[0].lp.held, SLOT_SWORD);
    assert_eq!(e.m.c[0].lp.potions.count_ones(), 6);
    run(&mut e, 1, |_, _| IDLE);
    assert_eq!(e.m.c[0].lp.held, SLOT_SWORD);
    let (paid, _) = run(&mut e, 30, |_, _| IDLE);
    assert_eq!(e.m.s.potion_throws, [1, 0]);
    assert_eq!((e.m.s.p[0].health, e.m.c[0].lp.health, e.m.c[1].rp.health), (13.0, 13.0, 13.0));
    let ep = e.episode();
    assert_eq!((ep.potions, ep.splashes, ep.heal_self, ep.heal_gift, ep.pot_offer), ([1.0, 0.0], [1.0, 0.0], [7.0, 0.0], [0.0, 0.0], [7.0, 0.0]));
    // Seven health are worth what seven health lost to hits cost, less
    // half of that for the eighth, which the potion did not give.
    let hh = e.m.s.cfg.hit_health();
    let want = 7.0 / hh - 0.5 * 1.0 / hh;
    assert!((before + paid - want).abs() < 0.2, "paid {} + {}, expected {want}", before, paid);
    let o = obs(&e);
    assert!((o[97] - 0.65).abs() < 1e-6 && (o[102] - 6.0 / 7.0).abs() < 1e-6);
}

#[test]
fn asking_all_the_time_throws_them_one_after_the_other() {
    let mut p = params();
    p.potions = [3.0, 3.0];
    let mut e = Env::new(Mode::SelfPlay, p, 2);
    run(&mut e, 2, |_, _| with(6, DOWN));
    let mut a = with(9, 1);
    // The pearl is asked for as well: there is none, and the potion comes
    // first anyway.
    a[8] = 1;
    run(&mut e, 60, |_, _| a);
    assert_eq!(e.m.s.potion_throws, [3, 0]);
    assert_eq!((e.m.s.p[0].potions, e.m.c[0].lp.potions), (0, 0));
    assert_eq!(e.m.c[0].lp.held, SLOT_SWORD);
    // 6 + 7 + 7 = 20, and the third found nothing to heal.
    assert_eq!(e.m.s.p[0].health, 20.0);
    assert_eq!(e.episode().heal_self[0], 14.0);
    let o = obs(&e);
    assert_eq!((o[102], o[103]), (0.0, 0.0));
}

#[test]
fn a_potion_at_full_health_costs_its_thrower() {
    let mut p = params();
    p.start_hurt = 0.0;
    let mut e = Env::new(Mode::SelfPlay, p, 3);
    run(&mut e, 2, |_, _| with(6, DOWN));
    let (a, _) = run(&mut e, 1, |_, _| with(9, 1));
    let (b, _) = run(&mut e, 30, |_, _| IDLE);
    let hh = e.m.s.cfg.hit_health();
    assert!((a + b + 0.5 * 8.0 / hh).abs() < 0.2, "paid {}", a + b);
}

/// What a potion did not offer its thrower costs more with `w_miss`, needed
/// or not.
#[test]
fn the_part_of_a_potion_that_missed_its_thrower_costs_more() {
    let throw = |miss: f32, hurt: bool| {
        let mut p = params();
        p.start_hurt = if hurt { 1.0 } else { 0.0 };
        p.start_health = [6.0, 6.0];
        p.w_miss = miss;
        let mut e = Env::new(Mode::SelfPlay, p, 3);
        run(&mut e, 2, |_, _| with(6, DOWN));
        let (a, _) = run(&mut e, 1, |_, _| with(9, 1));
        let (b, _) = run(&mut e, 30, |_, _| IDLE);
        assert_eq!(e.episode().pot_offer[0], 7.0, "thrown at its feet, standing: seven of the eight");
        (a + b, e.m.s.cfg.hit_health())
    };
    for hurt in [false, true] {
        let ((plain, hh), (strict, _)) = (throw(0.0, hurt), throw(2.0, hurt));
        assert!((plain - strict - 2.0 * 1.0 / hh).abs() < 1e-3, "{plain} {strict}");
    }
    // Thrown ahead, where it reaches nobody: two points are counted, not eight.
    let far = |miss: f32| {
        let mut p = params();
        p.w_miss = miss;
        let mut e = Env::new(Mode::SelfPlay, p, 3);
        let (a, _) = run(&mut e, 1, |_, _| with(9, 1));
        let (b, _) = run(&mut e, 40, |_, _| IDLE);
        assert_eq!((e.episode().splashes[0], e.episode().pot_offer[0]), (1.0, 0.0));
        (a + b, e.m.s.cfg.hit_health())
    };
    let ((plain, hh), (strict, _)) = (far(0.0), far(2.0));
    assert!((plain - strict - 2.0 * 2.0 / hh).abs() < 1e-3, "{plain} {strict}");
}

/// Every tick the crosshair is off the opponent for a potion costs the
/// thrower, ten ticks before the throw and ten after it at most.
#[test]
fn looking_away_for_a_potion_costs_by_the_tick() {
    let quick = |w: f32| {
        let mut p = params();
        p.w_pot_time = w;
        let mut e = Env::new(Mode::SelfPlay, p, 1);
        run(&mut e, 5, |_, _| IDLE);
        let (a, _) = run(&mut e, 1, |_, _| with(6, FLICK_DOWN));
        let (b, _) = run(&mut e, 1, |_, _| {
            let mut a = with(9, 1);
            a[6] = FLICK_UP;
            a
        });
        let (c, _) = run(&mut e, 30, |_, _| IDLE);
        assert_eq!((e.episode().potions[0], e.episode().pot_away[0]), (1.0, 1.0));
        a + b + c
    };
    assert!((quick(0.0) - quick(0.1) - 0.1).abs() < 1e-4, "{} {}", quick(0.0), quick(0.1));
    // Fifteen ticks of looking at the ground, the throw, and the ground
    // again for as long: ten ticks are counted on each side.
    let slow = |w: f32| {
        let mut p = params();
        p.w_pot_time = w;
        let mut e = Env::new(Mode::SelfPlay, p, 1);
        run(&mut e, 5, |_, _| IDLE);
        let (a, _) = run(&mut e, 1, |_, _| with(6, FLICK_DOWN));
        let (b, _) = run(&mut e, 15, |_, _| IDLE);
        let (c, _) = run(&mut e, 1, |_, _| with(9, 1));
        let (d, _) = run(&mut e, 15, |_, _| IDLE);
        let (f, _) = run(&mut e, 1, |_, _| with(6, FLICK_UP));
        let (g, _) = run(&mut e, 30, |_, _| IDLE);
        assert_eq!(e.episode().potions[0], 1.0);
        a + b + c + d + f + g
    };
    assert!((slow(0.0) - slow(0.1) - 2.0).abs() < 1e-4, "{} {}", slow(0.0), slow(0.1));
    // A second potion right after the first: the ticks before it were
    // counted with the first, and ten more are counted after it.
    let two = |w: f32| {
        let mut p = params();
        p.w_pot_time = w;
        let mut e = Env::new(Mode::SelfPlay, p, 1);
        run(&mut e, 5, |_, _| IDLE);
        let (a, _) = run(&mut e, 1, |_, _| with(6, FLICK_DOWN));
        let (b, _) = run(&mut e, 3, |_, _| IDLE);
        let (c, _) = run(&mut e, 2, |_, _| with(9, 1));
        let (d, _) = run(&mut e, 40, |_, _| IDLE);
        assert_eq!(e.episode().potions[0], 2.0);
        a + b + c + d
    };
    assert!((two(0.0) - two(0.1) - 0.1 * (4.0 + 1.0 + 10.0)).abs() < 1e-4, "{} {}", two(0.0), two(0.1));
}

#[test]
fn the_opponent_sees_a_potion_coming_and_what_it_would_give() {
    let mut p = params();
    p.start_dist_min = 2.0;
    p.start_dist_max = 2.0;
    let mut e = Env::new(Mode::SelfPlay, p, 4);
    run(&mut e, 2, |_, _| with(6, DOWN));
    run(&mut e, 1, |_, _| with(9, 1));
    let mut seen = false;
    for _ in 0..12 {
        run(&mut e, 1, |_, _| IDLE);
        let mut o = [0.0f32; OBS_DIM];
        e.write_obs(1, &mut o);
        if o[115] == 1.0 {
            seen = true;
            // Two blocks away from where it breaks: four of the eight for
            // itself, seven for the thrower.
            assert!((o[120] - 0.5).abs() < 0.13 && (o[121] - 0.875).abs() < 0.13, "{} {}", o[120], o[121]);
        }
    }
    assert!(seen);
    let mut o = [0.0f32; OBS_DIM];
    e.write_obs(1, &mut o);
    assert!((o[107] - 1.0 / 7.0).abs() < 1e-6, "it counts the potions it has seen");
    assert_eq!(e.episode().heal_gift[0], 4.0);
    assert_eq!((e.m.s.p[0].health, e.m.s.p[1].health), (13.0, 10.0));
}

#[test]
fn a_duel_ends_when_a_player_dies() {
    let mut p = params();
    p.start_health = [3.0, 3.0];
    p.potions = [0.0, 0.0];
    p.bot_level = [4.0, 4.0];
    p.start_dist_min = 4.0;
    p.start_dist_max = 4.0;
    p.opp_cps_min = -1.0;
    let mut e = Env::new(Mode::Bot, p, 5);
    let mut flag = 0;
    let mut paid = 0.0;
    let mut ticks = 0;
    while flag == 0 && ticks < 400 {
        let (r, d) = run(&mut e, 1, |_, _| IDLE);
        paid += r;
        flag = d;
        ticks += 1;
    }
    // Three health against a bot that hits for about 1.6: two hits.
    assert_eq!(flag, 1, "told as the end of the duel, not as a cut by the clock");
    assert!(ticks < 100, "{ticks}");
    assert_eq!((e.total.deaths, e.total.episodes), ([1.0, 0.0], 1.0));
    assert!(paid < -10.0 && paid > -16.0, "paid {paid}");
    // The next duel has begun.
    assert!(e.m.s.p[0].alive() && e.m.s.p[1].alive());
}

#[test]
fn the_potion_pitch_of_the_bots_follows_from_the_flight() {
    // Standing: straight down, two ticks. Running: further ahead.
    let (x, t) = potion_reach(90.0, 1.62);
    assert!(x.abs() < 1e-9 && t == 2, "{x} after {t} ticks");
    assert_eq!(potion_pitch(0.0, 1.62), 90.0);
    let run = potion_pitch(0.33, 1.62);
    assert!((40.0..80.0).contains(&run), "{run}");
    let (x, t) = potion_reach(run, 1.62);
    assert!((x - 0.33 * t as f64).abs() < 0.25, "{x} after {t} ticks");
}

#[test]
fn a_hurt_bot_heals_itself() {
    let mut healed = 0;
    let mut quality = 0.0;
    let mut thrown = 0.0;
    for seed in 0..20 {
        let mut p = params();
        p.bot_level = [3.0 + (seed % 2) as f64, 3.0 + (seed % 2) as f64];
        p.start_dist_min = 8.0;
        p.start_dist_max = 8.0;
        p.prot = [3.0, 3.0];
        p.sharp = [1.0, 1.0];
        let mut e = Env::new(Mode::Bot, p, 100 + seed);
        // The agent stands there with full health; the bot starts at six.
        e.m.s.p[0].health = 20.0;
        e.m.c[0].lp.health = 20.0;
        let mut best = 6.0f32;
        for _ in 0..160 {
            let (_, d) = run(&mut e, 1, |_, _| IDLE);
            if d != 0 {
                break;
            }
            best = best.max(e.m.s.p[1].health);
        }
        let ep = e.episode();
        thrown += ep.splashes[1];
        quality += ep.pot_offer[1];
        if best >= 12.0 {
            healed += 1;
        }
    }
    assert!(healed >= 17, "healed in {healed} of 20 duels");
    // Most of each potion reaches the bot itself.
    assert!(quality / thrown > 5.5, "{quality} offered by {thrown} potions");
}

/// A runner turns its back on the opponent when it is hurt, heals itself
/// on the run and comes back; another bot of its level comes straight on.
#[test]
fn a_runner_heals_itself_out_of_reach_and_comes_back() {
    let dist = |e: &Env| {
        let (a, b) = (e.m.s.p[0].l.body.pos, e.m.s.p[1].l.body.pos);
        (a[0] - b[0]).hypot(a[2] - b[2])
    };
    let mut ran = 0;
    for seed in 0..10 {
        let duel = |runner: f64| {
            let mut p = params();
            p.bot_level = [4.0, 4.0];
            p.start_dist_min = 6.0;
            p.start_dist_max = 6.0;
            p.bot_runner = runner;
            let mut e = Env::new(Mode::Bot, p, 300 + seed);
            // The agent stands there with full health; the bot starts at six.
            e.m.s.p[0].health = 20.0;
            e.m.c[0].lp.health = 20.0;
            let (mut far, mut near, mut healed) = (0.0f64, f64::MAX, 0);
            for t in 0..300 {
                run(&mut e, 1, |_, _| IDLE);
                far = far.max(dist(&e));
                if e.m.s.p[1].health >= 16.0 && healed == 0 {
                    healed = t;
                }
                if healed != 0 {
                    near = near.min(dist(&e));
                }
            }
            (far, near, healed, e.bot.runner)
        };
        let (far, near, healed, runner) = duel(1.0);
        assert!(runner);
        if far > 12.0 && healed != 0 && near < 4.0 {
            ran += 1;
        }
        // Without it the bot takes a few steps away for its potion at most.
        let (far, _, healed, runner) = duel(0.0);
        assert!(!runner && far < 10.0, "{far}");
    }
    assert!(ran >= 8, "ran, healed and came back in {ran} of 10 duels");
}

#[test]
fn a_bot_throws_a_pearl_at_a_player_that_heals_itself() {
    // The agent is hurt and throws its potion straight up, which takes a
    // second and a half to come back down. The bot, eight blocks away,
    // sees it and comes over with a pearl, beside the agent or behind it.
    let (mut came, mut near, mut hits) = (0, 0, 0.0);
    for seed in 0..20 {
        let mut p = params();
        p.bot_level = [4.0, 4.0];
        p.pearl_prob = 1.0;
        p.start_dist_min = 8.0;
        p.start_dist_max = 8.0;
        let mut e = Env::new(Mode::Bot, p, 300 + seed);
        e.m.s.p[1].health = 20.0;
        e.m.c[1].lp.health = 20.0;
        e.m.c[0].rp.health = 20.0;
        run(&mut e, 1, |_, _| with(6, FLICK_UP));
        run(&mut e, 1, |_, _| with(9, 1));
        for _ in 0..50 {
            let (_, d) = run(&mut e, 1, |_, _| IDLE);
            if d != 0 {
                break;
            }
            if e.bot_state.pearl_kind == 4 && e.episode().landings[1] > 0.0 {
                came += 1;
                let (a, b) = (e.m.s.p[0].l.body.pos, e.m.s.p[1].l.body.pos);
                near += ((a[0] - b[0]).hypot(a[2] - b[2]) < 4.5) as i32;
                break;
            }
        }
        run(&mut e, 20, |_, _| IDLE);
        hits += e.episode().pot_hit[0];
    }
    assert!(came >= 16, "a pearl in {came} of 20 duels");
    assert!(near >= 13, "{near} of {came} came down by the agent");
    assert!(hits >= 10.0, "{hits} hits in the second after the potion");
}
