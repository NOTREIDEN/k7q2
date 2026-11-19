//! Cheats of the bot's client: a long backtrack (`Params::bt_prob`), a
//! timer (`Params::timer_prob`, `sim::Timer`) and pearl spam
//! (`Params::pearl_spam_prob`, `BotParams::spam`). Off by default; the
//! first player's own game stays the plain one.
use pvpsim::client::{Client, ClientCfg, Input, RemotePlayer, MAX_BACKTRACK};
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};
use pvpsim::net::Item;
use pvpsim::sim::{Timer, TICK_US};
use pvpsim::world::World;

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];

fn params() -> Params {
    let mut p = Params::default();
    p.start_yaw_noise = 0.0;
    p.ep_ticks_min = 100_000.0;
    p.ep_ticks_max = 100_000.0;
    p.kb_random = 0.0;
    p.opp_edge_prob = 0.0;
    p.opp_autoblock = 0.0;
    p.sword_wear = 0.0;
    p.insane_prob = 1.0;
    p
}

/// Player 0 makes `n` ticks with the actions `f` gives, `f` seeing the
/// duel before each.
fn run(e: &mut Env, n: usize, mut f: impl FnMut(&mut Env, usize) -> [i32; ACT_HEADS]) {
    let (mut fo, mut fp) = (vec![0f32; OBS_DIM], vec![0f32; PRIV_DIM]);
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
            e.take_outcome(&mut fo, &mut fp);
        }
    }
}

#[test]
fn no_cheat_unless_asked() {
    for seed in 0..20 {
        let e = Env::new(Mode::Bot, params(), seed);
        assert!(!e.m.timer[0].on && !e.m.timer[1].on);
        assert!(!e.bot.spam);
        assert!(!e.m.s.p[0].no_pearl_cd && !e.m.s.p[1].no_pearl_cd);
        assert_eq!(e.m.c[1].edge.backtrack, 0);
    }
    // Asked for, against a bot only: never in self-play.
    let mut p = params();
    p.bt_prob = 1.0;
    p.timer_prob = 1.0;
    p.pearl_spam_prob = 1.0;
    let e = Env::new(Mode::SelfPlay, p, 1);
    assert!(!e.m.timer[1].on && !e.bot.spam && e.m.c[1].edge.backtrack == 0);
    let e = Env::new(Mode::Bot, p, 1);
    assert!(e.m.timer[1].on && !e.m.timer[0].on && e.bot.spam);
    assert!(e.m.s.p[1].no_pearl_cd && !e.m.s.p[0].no_pearl_cd);
    assert_eq!(e.m.c[0].edge, pvpsim::client::Edge::default());
}

#[test]
fn backtrack_of_600_ms_is_twelve_ticks() {
    let mut p = params();
    p.bt_prob = 1.0;
    p.bt_ms = [600.0, 600.0];
    assert_eq!(Env::new(Mode::Bot, p, 2).m.c[1].edge.backtrack, 12);
    p.bt_ms = [5000.0, 5000.0];
    assert_eq!(Env::new(Mode::Bot, p, 2).m.c[1].edge.backtrack as usize, MAX_BACKTRACK);
}

/// A client 2.5 blocks from the other player, who is then put 20 blocks
/// away: on how many of the ticks that follow it can still hit where it
/// showed them.
fn ticks_still_hit(backtrack: u8) -> usize {
    let world = World::arena(0.0, 40.0, true);
    let cfg = ClientCfg::with_sensitivity(0.5);
    let mut c = Client::new([0.0, 0.0, 0.0], 0.0, 0.0, -1, [0.0, 0.0, 2.5], 180.0, 0.0);
    c.edge.backtrack = backtrack;
    assert!(c.pick(&world, &cfg).entity);
    c.rp = RemotePlayer::spawn([20.0, 0.0, 2.5], 180.0, 0.0, true);
    let mut out = Vec::new();
    let mut n = 0;
    for _ in 0..30 {
        c.tick(&world, &Input::default(), &cfg, &mut out);
        if !c.pick(&world, &cfg).entity {
            break;
        }
        n += 1;
    }
    n
}

#[test]
fn a_backtrack_reaches_back_as_many_ticks_as_it_says() {
    assert_eq!(ticks_still_hit(0), 0);
    assert_eq!(ticks_still_hit(4), 3);
    assert_eq!(ticks_still_hit(12), 11);
    assert_eq!(ticks_still_hit(16), 15);
}

/// A bot whose timer is always on at `rate`, 6 blocks from an idle first
/// player.
fn timed(rate: f64) -> Env {
    let mut p = params();
    p.start_dist_min = 6.0;
    p.start_dist_max = 6.0;
    let mut e = Env::new(Mode::Bot, p, 7);
    e.m.timer[1] = Timer { on: true, rate: [rate, rate], burst_us: [60_000_000; 2], gap_us: [0; 2], blink: 0.0, range: [0.0, 100.0], ..Timer::default() };
    e
}

#[test]
fn a_timer_runs_the_bot_faster_and_the_server_takes_it() {
    for rate in [1.5, 2.0] {
        let mut e = timed(rate);
        let (c0, c1) = (e.m.client_ticks[0], e.m.client_ticks[1]);
        let mut accepted = true;
        run(&mut e, 200, |e, _| {
            // A movement packet the server did not take puts the player
            // back where it was and waits for the client to answer.
            accepted &= e.m.s.p[1].has_moved;
            IDLE
        });
        let (d0, d1) = (e.m.client_ticks[0] - c0, e.m.client_ticks[1] - c1);
        assert_eq!(d0, 200, "the first player's clock is the game's");
        let got = d1 as f64 / d0 as f64;
        assert!((got - rate).abs() < 0.05, "{rate}: {got}");
        assert!(accepted, "every movement packet was taken");
        assert!(e.m.timer[1].ticks as f64 >= 0.95 * d1 as f64);
        assert_eq!(e.total.timer_ticks + e.episode().timer_ticks, e.m.timer[1].ticks as f64);
    }
}

/// How far the bot gets across the ground towards an idle first player 20
/// blocks off, as the server has it, in `n` ticks.
fn chase(rate: Option<f64>, n: usize) -> f64 {
    let mut p = params();
    p.start_dist_min = 20.0;
    p.start_dist_max = 20.0;
    p.arena_half = 60.0;
    p.walls = false;
    p.pearl_prob = 0.0;
    let mut e = Env::new(Mode::Bot, p, 9);
    if let Some(r) = rate {
        e.m.timer[1] = Timer { on: true, rate: [r, r], burst_us: [60_000_000; 2], gap_us: [0; 2], blink: 0.0, range: [0.0, 100.0], ..Timer::default() };
    }
    run(&mut e, 10, |_, _| IDLE);
    let a = e.m.s.p[1].l.body.pos;
    run(&mut e, n, |_, _| IDLE);
    let b = e.m.s.p[1].l.body.pos;
    (b[0] - a[0]).hypot(b[2] - a[2])
}

#[test]
fn a_timer_closes_the_distance_faster() {
    let plain = chase(None, 10);
    let fast = chase(Some(2.0), 10);
    println!("ten ticks: {plain:.2} blocks plain, {fast:.2} with a timer of 2");
    assert!(fast > 1.7 * plain && plain > 1.0, "{plain} {fast}");
}

#[test]
fn timer_bursts_come_and_go() {
    let mut p = params();
    p.timer_prob = 1.0;
    p.timer_blink = 0.3;
    p.start_dist_min = 8.0;
    p.start_dist_max = 8.0;
    let (mut bursts, mut ticks, mut rounds) = (0, 0u32, 0u32);
    for seed in 0..6 {
        let mut e = Env::new(Mode::Bot, p, 100 + seed);
        assert!(e.m.timer[1].on);
        let c0 = e.m.client_ticks;
        run(&mut e, 600, |_, _| IDLE);
        bursts += e.m.timer[1].bursts;
        ticks += e.m.client_ticks[1] - c0[1];
        rounds += e.m.client_ticks[0] - c0[0];
        // Never longer than the longest burst in a row: the period is back
        // to the game's once it is over.
        let t = &e.m.timer[1];
        assert!(t.until - (e.m.next_client[1] - TICK_US) <= 1_500_000);
    }
    let rate = ticks as f64 / rounds as f64;
    println!("{bursts} bursts, the bot's clock {rate:.2} times the game's");
    assert!(bursts >= 6, "{bursts}");
    assert!(rate > 1.03 && rate < 2.0, "{rate}");
}

/// Ticks until a pearl spammer 5 blocks away first throws, when it sees the
/// first player with a potion in hand or with the sword, and why it threw
/// (`BotState::pearl_kind`).
fn first_pearl(potting: bool, seed: u64) -> (usize, u8) {
    let mut p = params();
    p.pearl_spam_prob = 1.0;
    p.pearl_prob = 1.0;
    p.pearl_cd_s = [15.0, 15.0];
    p.start_dist_min = 5.0;
    p.start_dist_max = 5.0;
    let mut e = Env::new(Mode::Bot, p, seed);
    assert!(e.bot.spam && e.bot.pearl_cd == 0);
    let mut first = None;
    run(&mut e, 80, |e, k| {
        if potting {
            e.m.c[1].rp.item = Item::Potion;
        }
        if first.is_none() && e.bot_state.pearl_stage != 0 {
            first = Some((k, e.bot_state.pearl_kind));
        }
        IDLE
    });
    first.unwrap_or((80, 0))
}

#[test]
fn a_pearl_spammer_goes_for_a_player_that_pots() {
    let pot: Vec<(usize, u8)> = (0..16).map(|s| first_pearl(true, s)).collect();
    let sword: Vec<(usize, u8)> = (0..16).map(|s| first_pearl(false, s)).collect();
    println!("first pearl (tick, kind) with a potion in hand {pot:?}, with the sword {sword:?}");
    let mean = |v: &[(usize, u8)]| v.iter().map(|x| x.0).sum::<usize>() as f64 / v.len() as f64;
    assert!(mean(&pot) <= 3.0, "{pot:?}");
    assert!(mean(&sword) > mean(&pot) + 3.0, "{pot:?} {sword:?}");
    assert!(pot.iter().all(|x| x.1 == 4), "{pot:?}");
    assert!(sword.iter().all(|x| x.1 != 4), "{sword:?}");
    // At it, next to it and behind it.
    assert!(sword.iter().any(|x| x.1 == 5) && sword.iter().any(|x| x.1 == 1), "{sword:?}");
}

#[test]
fn a_pearl_spammer_has_no_cooldown_and_many_pearls() {
    let mut p = params();
    p.pearl_spam_prob = 1.0;
    p.pearl_spam_pearls = 30.0;
    p.pearl_prob = 1.0;
    p.pearl_cd_s = [15.0, 15.0];
    p.start_dist_min = 10.0;
    p.start_dist_max = 10.0;
    let mut e = Env::new(Mode::Bot, p, 5);
    assert_eq!(e.m.s.p[1].pearls, 30);
    // Thirty seconds of the first player walking circles, against a bot
    // that cannot die: a cooldown of fifteen seconds would let three
    // through at most.
    e.m.s.p[1].immortal = true;
    run(&mut e, 600, |_, k| {
        let mut a = IDLE;
        a[0] = 1;
        a[5] = if (k / 40) % 2 == 0 { 16 } else { 13 };
        a
    });
    let n = e.m.s.throws[1];
    println!("the spammer threw {n} pearls in thirty seconds");
    assert!(n >= 6, "{n}");
    assert_eq!(e.m.s.p[1].pearl_wait, 0);
}
