//! The bot that trades (`Params::bot_trader`): a block in the opponent's
//! turn, a critical hit in its own, and its hits as many as before.
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};
use pvpsim::net::Item;

const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];
const STEPS: usize = 900;

fn params(trader: f64) -> Params {
    let mut p = Params::default();
    p.pearl_prob = 0.0;
    p.start_dist_min = 3.0;
    p.start_dist_max = 3.0;
    p.insane_prob = 1.0;
    p.ep_ticks_min = 2000.0;
    p.ep_ticks_max = 2000.0;
    p.bot_trader = trader;
    // Hits that hardly move anybody, where trades are what it is for.
    p.kb_random = 0.0;
    p.tiny_kb_prob = 1.0;
    p.tiny_kb_h = [0.08, 0.32];
    p.tiny_kb_v = [0.15, 0.35];
    p
}

struct Run {
    hits: f64,
    crits: f64,
    /// Rounds in which the bot blocked with its sword while player 0 could
    /// not be hurt (its own hit's turn), and while it could.
    block_their_turn: usize,
    block_my_turn: usize,
    trader: bool,
    spacer: bool,
    trader_duels: f64,
}

/// Player 0 stands still and never hits back.
fn run(p: Params, seed: u64) -> Run {
    let mut e = Env::new(Mode::Bot, p, seed);
    let (mut fobs, mut fpriv) = (vec![0f32; OBS_DIM], vec![0f32; PRIV_DIM]);
    let (mut a, mut b) = (0, 0);
    let (trader, spacer) = (e.bot.trader, e.bot.spacer);
    for _ in 0..STEPS {
        e.step(&IDLE);
        let (_, d) = e.take_outcome(&mut fobs, &mut fpriv);
        assert_eq!(d, 0);
        let s = &e.m.s.p[1];
        if s.using_item && s.in_hand().0 == Item::Sword {
            // Whether player 0 may be hurt again: its hurt time as the
            // bot's client sees it at the end of the round (the bot acts
            // on what it saw before the round, one more).
            if e.m.c[1].rp.hurt_time >= 1 {
                a += 1;
            } else {
                b += 1;
            }
        }
    }
    let st = *e.episode();
    Run {
        hits: st.hits[1],
        crits: st.crits[1],
        block_their_turn: a,
        block_my_turn: b,
        trader,
        spacer,
        trader_duels: st.trader_duels,
    }
}

#[test]
fn off_by_default() {
    let p = Params::default();
    assert_eq!(p.bot_trader, 0.0);
    for seed in 1..6 {
        let r = run(params(0.0), seed);
        assert!(!r.trader);
        assert_eq!(r.trader_duels, 0.0);
    }
}

#[test]
fn a_trader_blocks_in_the_opponents_turn_and_crits_in_its_own() {
    let mut plain = (0.0, 0.0, 0, 0);
    let mut trade = (0.0, 0.0, 0, 0);
    for seed in 1..5 {
        let mut p0 = params(0.0);
        p0.bot_spacer = 0.0;
        let a = run(p0, seed);
        let mut p1 = params(1.0);
        p1.bot_spacer = 1.0;
        let b = run(p1, seed);
        assert!(b.trader && !b.spacer, "a trader does not space its combos");
        plain = (plain.0 + a.hits, plain.1 + a.crits, plain.2 + a.block_their_turn, plain.3 + a.block_my_turn);
        trade = (trade.0 + b.hits, trade.1 + b.crits, trade.2 + b.block_their_turn, trade.3 + b.block_my_turn);
    }
    eprintln!("plain {plain:?} trader {trade:?}");
    assert!(plain.0 >= 40.0, "the bot should keep hitting: {plain:?}");
    // Nearly as many hits as before (a block slows it down), up to every
    // other one critical (a jump lasts longer than a hurt time).
    assert!(trade.0 >= 0.8 * plain.0, "{plain:?} {trade:?}");
    assert!(trade.1 >= 0.35 * trade.0, "{trade:?}");
    assert!(plain.1 <= 0.2 * plain.0, "{plain:?}");
    // Blocks in the turn its hit gives the opponent, and the button up
    // when it may hit again.
    assert!(trade.2 as f64 >= 2.0 * trade.0, "{trade:?}");
    assert!(trade.2 > 5 * plain.2.max(1), "{plain:?} {trade:?}");
    assert!(trade.3 * 10 <= trade.2, "{trade:?}");
}

#[test]
fn nothing_but_the_choices_of_its_own_turns() {
    // With neither a block nor a jump wanted, a trader plays as the plain
    // bot apart from the draws for those choices: as many hits.
    let mut a = 0.0;
    let mut b = 0.0;
    for seed in 1..5 {
        let mut p0 = params(0.0);
        p0.bot_spacer = 0.0;
        a += run(p0, seed).hits;
        let mut p1 = params(1.0);
        p1.trade_block = 0.0;
        p1.trade_crit = 0.0;
        b += run(p1, seed).hits;
    }
    assert!((b - a).abs() <= 0.1 * a, "{a} {b}");
}

/// Potions the bot has thrown after `steps` rounds of a duel it starts with
/// 9 health, against a player 0 that stands still.
fn potions_thrown(trader: f64, heal: f64, seed: u64, steps: usize) -> f64 {
    let mut p = params(trader);
    p.infinite_health = false;
    p.health_prob = 1.0;
    p.potions = [6.0, 6.0];
    p.start_hurt = 1.0;
    p.hurt_who = 2;
    p.start_health = [9.0, 9.0];
    p.trade_heal = heal;
    let mut e = Env::new(Mode::Bot, p, seed);
    let (mut fobs, mut fpriv) = (vec![0f32; OBS_DIM], vec![0f32; PRIV_DIM]);
    assert_eq!(e.m.s.p[1].health, 9.0);
    for _ in 0..steps {
        e.step(&IDLE);
        e.take_outcome(&mut fobs, &mut fpriv);
    }
    e.episode().potions[1]
}

#[test]
fn a_trader_heals_itself_before_it_is_one_hit_from_death() {
    let (mut plain, mut own, mut early) = (0.0, 0.0, 0.0);
    for seed in 1..5 {
        // The hardest bot waits for 4 to 6; told 10 (9 to 11), a trader
        // heals at 9.
        plain += potions_thrown(0.0, 10.0, seed, 200);
        own += potions_thrown(1.0, 0.0, seed, 200);
        early += potions_thrown(1.0, 10.0, seed, 200);
    }
    assert_eq!(plain, 0.0);
    assert_eq!(own, 0.0);
    assert!(early >= 2.0, "{early}");
}
