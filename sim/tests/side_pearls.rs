//! Pearls to the side of an opponent that pots: bots that turn their back
//! and run with the potion before they throw it (`Params::pot_turn_prob`),
//! where a pearl is counted as landing (`Stats::pearl_front` and the
//! rest), the cost of a pearl into the opponent's sword that loses the next
//! hit (`Params::w_pearl_front`), and the cost given back for a pearl that
//! took the opponent's potion (`Params::w_pearl_pot`).
use pvpsim::bot;
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};
use pvpsim::net::{Item, Kind, SLOT_SWORD};

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];

fn with(head: usize, v: i32) -> [i32; ACT_HEADS] {
    let mut a = IDLE;
    a[head] = v;
    a
}

fn params() -> Params {
    let mut p = Params::default();
    p.pearl_prob = 1.0;
    p.pearl_cd_s = [5.0, 5.0];
    p.start_yaw_noise = 0.0;
    p.ep_ticks_min = 100_000.0;
    p.ep_ticks_max = 100_000.0;
    p.kb_random = 0.0;
    p.opp_edge_prob = 0.0;
    p.opp_autoblock = 0.0;
    p.sword_wear = 0.0;
    p
}

/// Player 0 makes `n` ticks with the actions `f` gives; in self-play
/// player 1 stands still. Returns player 0's rewards added up.
fn run(e: &mut Env, n: usize, mut f: impl FnMut(&mut Env, usize) -> [i32; ACT_HEADS]) -> f32 {
    let (mut fo, mut fp) = (vec![0f32; OBS_DIM], vec![0f32; PRIV_DIM]);
    let mut total = 0.0;
    let mut k = 0;
    while k < n {
        if e.pending == 0 {
            let a = f(e, k);
            e.step(&a);
            k += 1;
        } else {
            e.step(&IDLE);
        }
        if e.pending == 0 {
            total += e.take_outcome(&mut fo, &mut fp).0;
        }
    }
    total
}

/// The first time player 0 sees the bot with a healing potion in hand while
/// it has its back turned (more than 100 degrees from looking at player 0,
/// as the server has it: a client turns a player it sees only part of the
/// way round when the turn comes in one go, as the game does): for how many
/// ticks in a row, in each of `duels` duels.
fn runs_before_the_throw(turn: bool, duels: u64) -> Vec<i32> {
    let mut found = Vec::new();
    for d in 0..duels {
        let mut p = params();
        p.infinite_health = false;
        p.potions = [6.0, 6.0];
        p.pearl_prob = 0.0;
        p.insane_prob = 1.0;
        p.start_dist_min = 3.0;
        p.start_dist_max = 5.0;
        if turn {
            p.pot_turn_prob = 1.0;
            p.pot_turn_hurt = 1.0;
        }
        let mut e = Env::new(Mode::Bot, p, 40 + d);
        assert_eq!(e.bot.pot_turn, turn);
        if !turn {
            // As hurt as the bot that pots with a turn starts.
            e.m.s.p[1].set_health(3.5);
        }
        let mut back = 0;
        let mut first = None;
        run(&mut e, 100, |e, _| {
            let c = &e.m.c[0];
            let (to_me, _) = bot::angles_to(e.m.s.p[1].l.body.pos, e.m.s.p[0].l.body.pos);
            let off = pvpsim::mth::wrap_deg(to_me - e.m.s.p[1].l.yaw as f64).abs();
            if c.rp.item == Item::Potion && off > 100.0 {
                back += 1;
            } else if back > 0 && first.is_none() {
                first = Some(back);
            }
            IDLE
        });
        assert!(e.episode().potions[1] + e.total.potions[1] > 0.0, "the bot potted");
        found.push(first.unwrap_or(0));
    }
    found
}

#[test]
fn a_bot_that_pots_with_a_turn_runs_with_its_potion_before_it_throws() {
    let turn = runs_before_the_throw(true, 12);
    let plain = runs_before_the_throw(false, 12);
    println!("ticks with its back turned and the potion in hand: {turn:?}, without the turn {plain:?}");
    // Up to a second, a quarter of it at least, give or take what the view
    // takes to come round and the ping: time to see it and throw a pearl.
    let mean = turn.iter().sum::<i32>() as f64 / turn.len() as f64;
    assert!(turn.iter().all(|&k| k >= 3 && k <= 24), "{turn:?}");
    assert!(mean >= 7.0, "{mean}");
    // The hardest bot otherwise throws as it turns.
    assert!(plain.iter().all(|&k| k <= 3), "{plain:?}");
}

#[test]
fn bots_pot_with_a_turn_only_when_asked_and_only_from_level_3() {
    for (prob, level, runner, want) in [(0.0, 4.0, 0.0, false), (1.0, 4.0, 0.0, true), (1.0, 2.0, 0.0, false), (1.0, 3.0, 1.0, false)] {
        let mut p = params();
        p.pot_turn_prob = prob;
        p.pot_turn_run_s = 0.5;
        p.bot_level = [level, level];
        p.bot_runner = runner;
        let e = Env::new(Mode::Bot, p, 3);
        assert_eq!(e.bot.pot_turn, want, "{prob} {level} {runner}");
        if want {
            assert_eq!(e.bot.pot_run, 10);
        }
    }
    // Never in self-play.
    let mut p = params();
    p.pot_turn_prob = 1.0;
    assert!(!Env::new(Mode::SelfPlay, p, 3).bot.pot_turn);
}

/// Player 0 turns its view `yaw` degrees off player 1, who stands
/// `apart` blocks away facing it, and throws a pearl that comes down
/// `reach` blocks ahead. Returns the episode's landing counts: front, side,
/// back, faced, and all landings.
fn land(apart: f64, yaw: f32, reach: f64) -> [f64; 5] {
    let mut p = params();
    p.start_dist_min = apart;
    p.start_dist_max = apart;
    let mut e = Env::new(Mode::SelfPlay, p, 7);
    let floor = e.m.world.floor_y;
    run(&mut e, 2, |_, _| IDLE);
    let eye = e.m.c[0].lp.eye();
    e.m.c[0].lp.l.yaw += yaw;
    e.m.c[0].lp.l.pitch = bot::pearl_pitch(reach, eye[1] - floor) as f32;
    run(&mut e, 3, |_, _| IDLE);
    run(&mut e, 1, |_, _| with(8, 1));
    run(&mut e, 40, |_, _| IDLE);
    let s = e.episode();
    assert_eq!(s.pearls[0], 1.0);
    [s.pearl_front[0], s.pearl_side[0], s.pearl_back[0], s.pearl_faced[0], s.landings[0]]
}

#[test]
fn a_pearl_is_counted_by_where_it_lands_next_to_the_other_player() {
    // Straight at it: in front, into its sword.
    assert_eq!(land(7.0, 0.0, 6.0), [1.0, 0.0, 0.0, 1.0, 1.0]);
    // Forty degrees round, as far: to its side, out of its view.
    assert_eq!(land(7.0, 40.0, 7.0), [0.0, 1.0, 0.0, 0.0, 1.0]);
    assert_eq!(land(7.0, -40.0, 7.0), [0.0, 1.0, 0.0, 0.0, 1.0]);
    // Away from it: not at it at all.
    assert_eq!(land(7.0, 180.0, 9.0), [0.0, 0.0, 0.0, 0.0, 1.0]);
}

/// Player 0 throws a pearl at a level 4 bot that comes at it, landing in
/// front of it, and stands there. Its rewards over five seconds, and the
/// pearls into the bot's sword after which the bot hit first.
fn into_the_sword(w_front: f32) -> (f32, f64) {
    let mut p = params();
    p.bot_level = [4.0, 4.0];
    p.bot_pearl = 0.0;
    p.start_dist_min = 8.0;
    p.start_dist_max = 8.0;
    p.w_pearl_front = w_front;
    let mut e = Env::new(Mode::Bot, p, 5);
    let floor = e.m.world.floor_y;
    let eye = e.m.c[0].lp.eye();
    e.m.c[0].lp.l.pitch = bot::pearl_pitch(5.0, eye[1] - floor) as f32;
    let mut total = run(&mut e, 2, |_, _| IDLE);
    total += run(&mut e, 1, |_, _| with(8, 1));
    total += run(&mut e, 100, |_, _| IDLE);
    let s = e.episode();
    assert_eq!((s.landings[0], s.pearl_faced[0]), (1.0, 1.0));
    (total, s.pearl_faced_lost[0])
}

#[test]
fn a_pearl_into_the_sword_that_loses_the_next_hit_costs_what_it_is_set_to() {
    let (free, lost0) = into_the_sword(0.0);
    let (paid, lost1) = into_the_sword(2.0);
    assert_eq!((lost0, lost1), (1.0, 1.0));
    assert!((free - paid - 2.0).abs() < 1e-3, "free {free} paid {paid}");
}

/// Player 0, hurt, throws a pearl at its feet next to a hurt level 2 bot,
/// which then pots at its own feet and heals both. Then, if `kill`, the
/// bot dies. Player 0's rewards, its pearl's potions taken and the health
/// they took.
fn take_a_potion(cost: f32, w_pot: f32, kill: bool, seed: u64) -> (f32, f64, f64) {
    let mut p = params();
    p.infinite_health = false;
    p.potions = [6.0, 6.0];
    p.bot_level = [2.0, 2.0];
    p.bot_pearl = 0.0;
    p.start_dist_min = 2.0;
    p.start_dist_max = 2.0;
    p.w_pearl_cost = cost;
    p.w_pearl_pot = w_pot;
    let mut e = Env::new(Mode::Bot, p, seed);
    e.m.s.p[0].set_health(15.0);
    e.m.c[0].lp.l.pitch = 90.0;
    let mut total = run(&mut e, 3, |_, _| IDLE);
    total += run(&mut e, 1, |_, _| with(8, 1));
    let mut hurt = false;
    total += run(&mut e, 40, |e, _| {
        if !hurt && e.episode().landings[0] > 0.0 {
            hurt = true;
            e.m.s.p[1].set_health(5.0);
        }
        IDLE
    });
    if kill {
        e.m.s.p[1].set_health(0.0);
    }
    total += run(&mut e, 2, |_, _| IDLE);
    let s = e.total;
    let s = if kill { s } else { *e.episode() };
    (total, s.pearl_pots[0], s.pearl_deny[0])
}

#[test]
fn a_pearl_that_took_the_other_players_potion_gives_back_its_cost_and_no_more() {
    let mut taken = 0;
    for seed in 0..6 {
        let (kept, n0, took0) = take_a_potion(1.0, 0.0, false, seed);
        let (back, n1, took1) = take_a_potion(1.0, 1.0, false, seed);
        // Counted whether or not anything is given back for it.
        assert_eq!((n0, took0), (n1, took1));
        if n1 == 0.0 {
            assert_eq!(kept, back);
            continue;
        }
        taken += 1;
        // The share of the cost for the share of the potion's eight points
        // taken, and never more than the cost.
        let want = (took1 as f32 / 8.0).min(1.0);
        assert!(took1 > 0.0 && took1 <= 8.0 * n1, "took {took1}");
        assert!((back - kept - want).abs() < 1e-3, "seed {seed}: kept {kept} back {back} took {took1}");
        // A kill afterwards gives back what is left of the cost, so the
        // pearl has cost nothing either way.
        let (kept, _, _) = take_a_potion(1.0, 0.0, true, seed);
        let (back, _, _) = take_a_potion(1.0, 1.0, true, seed);
        assert!((back - kept).abs() < 1e-3, "seed {seed}: after a kill kept {kept} back {back}");
        let (free, _, _) = take_a_potion(0.0, 1.0, true, seed);
        assert!((free - back).abs() < 1e-3, "seed {seed}: free {free} back {back}");
    }
    assert!(taken >= 3, "{taken} of 6");
}

#[test]
fn nothing_is_given_back_for_a_potion_far_from_the_thrower() {
    // The same, but the pearl takes player 0 far off before the bot pots.
    let mut p = params();
    p.infinite_health = false;
    p.potions = [6.0, 6.0];
    p.bot_level = [2.0, 2.0];
    p.bot_pearl = 0.0;
    p.start_dist_min = 2.0;
    p.start_dist_max = 2.0;
    p.w_pearl_cost = 1.0;
    p.w_pearl_pot = 1.0;
    let mut e = Env::new(Mode::Bot, p, 1);
    e.m.c[0].lp.l.yaw += 180.0;
    e.m.c[0].lp.l.pitch = -10.0;
    run(&mut e, 3, |_, _| IDLE);
    run(&mut e, 1, |_, _| with(8, 1));
    let mut hurt = false;
    run(&mut e, 60, |e, _| {
        if !hurt && e.episode().landings[0] > 0.0 {
            hurt = true;
            e.m.s.p[1].set_health(5.0);
        }
        IDLE
    });
    let s = e.episode();
    assert_eq!(s.landings[0], 1.0);
    assert!(s.splashes[1] >= 1.0, "the bot potted");
    let q = (e.m.s.p[0].l.body.pos, e.m.s.p[1].l.body.pos);
    assert!((q.0[0] - q.1[0]).hypot(q.0[2] - q.1[2]) > 8.0);
    assert_eq!((s.pearl_pots[0], s.pearl_deny[0]), (0.0, 0.0));
    // Nor is a pearl counted as thrown on a potion.
    assert_eq!(s.pearl_on_pot[0], 0.0);
}

#[test]
fn a_pearl_thrown_while_the_other_player_holds_a_potion_is_counted() {
    let mut p = params();
    p.infinite_health = false;
    p.potions = [6.0, 6.0];
    p.insane_prob = 1.0;
    p.pot_turn_prob = 1.0;
    p.pot_turn_hurt = 1.0;
    p.pot_turn_run_s = 2.0;
    p.start_dist_min = 4.0;
    p.start_dist_max = 4.0;
    let mut e = Env::new(Mode::Bot, p, 2);
    let mut thrown = false;
    run(&mut e, 80, |e, _| {
        let c = &e.m.c[0];
        let on = c.rp.item == Item::Potion && !c.thrown.iter().any(|t| t.kind == Kind::Potion);
        if on && !thrown && e.m.s.p[1].held != SLOT_SWORD {
            thrown = true;
            return with(8, 1);
        }
        IDLE
    });
    assert!(thrown);
    // The duel may be over by now (the bot hits hard): what it counted.
    let (s, t) = (e.episode(), &e.total);
    assert_eq!((s.pearls[0] + t.pearls[0], s.pearl_on_pot[0] + t.pearl_on_pot[0]), (1.0, 1.0));
}
