//! What the latency of a player does to its potions and pearls.
//!
//! The server throws a potion from where it has the player and breaks it
//! against where it has the player some ticks later. A player's moves reach
//! the server late, all of them by the same time, so the same throw gives
//! the same health whatever the ping: only the news of it comes later. What
//! does change with the ping is everything the server does in between: a
//! hit moves its victim on the server a round trip later.
//!
//! A client sees its own potion a round trip after the throw, where the
//! server had it, and the copy it flies itself stays one step of the
//! flight behind the server's. What an agent is told of a potion in flight
//! allows for both.
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};

const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0];
/// A quarter turn down in one tick.
const FLICK_DOWN: i32 = 28;
const PINGS: [f64; 4] = [20.0, 110.0, 200.0, 290.0];

fn params(ping: f64) -> Params {
    let mut p = Params::default();
    p.infinite_health = false;
    p.prot = [2.0, 2.0];
    p.sharp = [2.0, 2.0];
    p.potions = [7.0, 7.0];
    p.start_hurt = 1.0;
    p.start_health = [6.0, 6.0];
    p.pearl_prob = 0.0;
    p.start_dist_min = 30.0;
    p.start_dist_max = 30.0;
    p.arena_half = 40.0;
    p.start_yaw_noise = 0.0;
    p.ep_ticks_min = 100_000.0;
    p.ep_ticks_max = 100_000.0;
    p.kb_random = 0.0;
    p.opp_edge_prob = 0.0;
    p.agent_ping_min_ms = ping;
    p.agent_ping_max_ms = ping;
    p.ping_min_ms = 20.0;
    p.ping_max_ms = 20.0;
    p.jitter_max_ms = 0.0;
    p
}

/// Let each player make its ticks with the actions `f` and `g` give, until
/// player 0 has made `n` and is next again. An agent is told what it sees
/// when it is next, so that is when the tests look.
fn both(e: &mut Env, n: usize, mut f: impl FnMut(usize) -> [i32; ACT_HEADS], mut g: impl FnMut(usize) -> [i32; ACT_HEADS]) {
    let (mut fo, mut fp) = ([0.0f32; OBS_DIM], [0.0f32; PRIV_DIM]);
    let (mut k, mut k1) = (0, 0);
    while k < n || e.pending != 0 {
        if e.pending == 0 {
            let a = f(k);
            e.step(&a);
            k += 1;
        } else {
            let a = g(k1);
            e.step(&a);
            k1 += 1;
        }
        e.take_outcome(&mut fo, &mut fp);
    }
}

fn run(e: &mut Env, n: usize, f: impl FnMut(usize) -> [i32; ACT_HEADS]) {
    both(e, n, f, |_| IDLE);
}

fn obs(e: &Env, id: usize) -> [f32; OBS_DIM] {
    let mut o = [0.0f32; OBS_DIM];
    e.write_obs(id, &mut o);
    o
}

/// What player 0 was told and what came of it when it ran straight ahead
/// with Speed II, turned its view by `turn` (pitch choices, one a tick),
/// threw a potion and ran on.
struct Thrown {
    /// Before the throw: the health foreseen if it keeps running.
    before: f32,
    /// On each tick it saw its potion: the health foreseen.
    flying: Vec<f32>,
    /// What the potion offered it.
    offered: f32,
    /// Own ticks from the throw to the tick it was told of its health
    /// (-1: never), and to the first on which it saw its potion.
    told: i32,
    seen: i32,
}

fn sprint_throw(ping: f64, jitter: f64, seed: u64, turn: &[i32], strafe: i32) -> Thrown {
    let mut p = params(ping);
    p.jitter_max_ms = jitter;
    let mut e = Env::new(Mode::SelfPlay, p, seed);
    let go = |pitch: i32, item: i32| {
        let mut a = IDLE;
        a[0] = 1;
        a[1] = strafe;
        a[3] = 1;
        a[6] = pitch;
        a[9] = item;
        a
    };
    run(&mut e, 20, |_| go(10, 0));
    for p in turn {
        run(&mut e, 1, |_| go(*p, 0));
    }
    let before = obs(&e, 0)[125] * 8.0;
    let health = e.m.c[0].lp.health;
    run(&mut e, 1, |_| go(10, 1));
    let (mut flying, mut told, mut seen) = (Vec::new(), -1, -1);
    for t in 1..30 {
        let o = obs(&e, 0);
        if o[108] == 1.0 {
            flying.push(o[113] * 8.0);
            if seen < 0 {
                seen = t;
            }
        }
        if told < 0 && e.m.c[0].lp.health != health {
            told = t;
        }
        run(&mut e, 1, |_| go(10, 0));
    }
    Thrown { before, flying, offered: e.episode().pot_offer[0] as f32, told, seen }
}

/// Views to throw with: from at the feet to far ahead and a little up.
const TURNS: [&[i32]; 9] = [&[20, 20, 19, 16, 14], &[20, 20], &[24], &[25], &[26], &[28], &[20, 20, 20, 20], &[10], &[1]];

fn throws(ping: f64, jitter: f64) -> Vec<Thrown> {
    let mut v = Vec::new();
    for turn in TURNS {
        for strafe in 0..3 {
            for seed in 0..4 {
                v.push(sprint_throw(ping, jitter, seed, turn, strafe));
            }
        }
    }
    v
}

#[test]
fn the_same_throw_gives_the_same_at_any_ping_and_the_news_comes_later() {
    let all: Vec<Vec<Thrown>> = PINGS.iter().map(|p| throws(*p, 0.0)).collect();
    // Every amount from nothing to all eight is among them.
    let mut amounts = [0; 9];
    for r in &all[0] {
        amounts[r.offered as usize] += 1;
    }
    assert!(amounts.iter().all(|n| *n > 0), "{amounts:?}");
    let mut later = [0.0; 4];
    let mut n = 0.0;
    for k in 0..all[0].len() {
        for i in 1..4 {
            assert_eq!(all[i][k].offered, all[0][k].offered, "throw {k} at {} ms", PINGS[i]);
            assert!(all[i][k].told >= all[i - 1][k].told && all[i][k].seen > all[i - 1][k].seen, "throw {k} at {} ms", PINGS[i]);
        }
        if all[0][k].told >= 0 {
            n += 1.0;
            for i in 0..4 {
                later[i] += (all[i][k].told - all[0][k].told) as f64;
            }
        }
    }
    // Two ticks for every hundred milliseconds.
    for i in 1..4 {
        let (got, want) = (later[i] / n, (PINGS[i] - PINGS[0]) / 50.0);
        assert!((got - want).abs() < 0.6, "{} ms: told {got:.2} ticks later", PINGS[i]);
    }
    // The potion itself is seen a round trip and a tick after the throw.
    for (i, ping) in PINGS.iter().enumerate() {
        let lag = (ping / 50.0).floor() as i32 + 1;
        assert!(all[i].iter().all(|r| r.seen == lag || r.seen == lag + 1), "{ping} ms");
    }
}

#[test]
fn what_a_potion_in_flight_will_give_is_foreseen_at_any_ping() {
    for ping in PINGS {
        let all = throws(ping, 0.0);
        let (mut before, mut ticks) = (0, 0);
        for (k, r) in all.iter().enumerate() {
            // Before the throw the game's random part of it is not known:
            // a point more or less now and then.
            assert!((r.before - r.offered).abs() <= 1.0, "{ping} ms, throw {k}: {} foreseen, {} offered", r.before, r.offered);
            before += (r.before == r.offered) as usize;
            // Once it flies there is nothing left to guess, also on the
            // ticks the copy flies on after the server's has broken.
            assert!(r.flying.len() >= 3, "{ping} ms, throw {k}");
            assert!(r.flying.iter().all(|h| *h == r.offered), "{ping} ms, throw {k}: {:?} foreseen, {} offered", r.flying, r.offered);
            ticks += r.flying.len();
        }
        assert!(before * 10 >= all.len() * 9, "{ping} ms: {before} of {}", all.len());
        assert!(ticks > 600);
    }
    // Packets that do not all take the same time move the server's picture
    // of the player by a tick now and then.
    let all = throws(200.0, 30.0);
    let (mut right, mut ticks) = (0, 0);
    for r in &all {
        assert!(r.flying.iter().all(|h| (*h - r.offered).abs() <= 2.0), "{:?} foreseen, {} offered", r.flying, r.offered);
        right += r.flying.iter().filter(|h| **h == r.offered).count();
        ticks += r.flying.len();
    }
    assert!(right * 10 >= ticks * 8, "{right} of {ticks}");
}

/// The health a potion thrown at its own feet offers a player standing 2.6
/// blocks from an opponent with Knockback II on its sword, who hits it
/// `after` of its own ticks after the throw (before it, if negative).
fn knocked(ping: f64, after: i32) -> f32 {
    let mut p = params(ping);
    p.start_dist_min = 2.6;
    p.start_dist_max = 2.6;
    p.kb_ench = 2;
    let mut e = Env::new(Mode::SelfPlay, p, 3);
    both(&mut e, 10, |_| IDLE, |_| IDLE);
    let mut down = IDLE;
    down[6] = FLICK_DOWN;
    both(&mut e, 1, |_| down, |_| IDLE);
    let (mut throw, mut hit) = (IDLE, IDLE);
    throw[9] = 1;
    hit[4] = 1;
    both(&mut e, 24, |k| if k == 8 { throw } else { IDLE }, |k| if k as i32 == 8 + after { hit } else { IDLE });
    assert_eq!(e.episode().hits, [0.0, 1.0]);
    e.episode().pot_offer[0] as f32
}

#[test]
fn the_higher_the_ping_the_earlier_a_hit_has_to_come_to_cost_a_potion() {
    // The last moment a hit still takes something off the seven points.
    let last = |ping: f64| (-8..8).rev().find(|a| knocked(ping, *a) < 7.0).unwrap();
    let (low, mid, high) = (last(20.0), last(200.0), last(300.0));
    assert!(low >= mid && mid >= high && low >= high + 2, "{low} {mid} {high}");
    for ping in [20.0, 200.0, 300.0] {
        // Well before the throw the player is in the air and far from its
        // potion when it breaks; well after, the potion has broken.
        assert!(knocked(ping, -4) <= 3.0 && knocked(ping, 4) == 7.0, "{ping} ms");
    }
    // A hit at the moment of the throw or a tick before it: on a good
    // connection the player is gone before its potion breaks, on a slow one
    // the server does not have it moving yet.
    assert!(knocked(20.0, 0) < 7.0 && knocked(300.0, 0) == 7.0);
    assert!(knocked(20.0, -1) <= 4.0 && knocked(300.0, -1) == 7.0);
}

#[test]
fn where_a_pearl_in_flight_will_land_is_foreseen_at_any_ping() {
    for ping in [20.0, 200.0] {
        for seed in 0..6 {
            let mut p = params(ping);
            p.pearl_prob = 1.0;
            p.pearl_cd_s = [5.0, 5.0];
            p.start_dist_min = 28.0;
            p.start_dist_max = 28.0;
            let mut e = Env::new(Mode::SelfPlay, p, seed);
            run(&mut e, 4, |_| IDLE);
            let mut a = IDLE;
            a[8] = 1;
            let from = e.m.c[0].lp.l.body.pos;
            let yaw = (e.m.c[0].lp.l.yaw as f64).to_radians();
            run(&mut e, 1, |_| a);
            // What the thrower and the other player are told while it
            // flies: how far ahead of the thrower it comes down.
            let (mut mine, mut theirs) = (Vec::new(), Vec::new());
            let mut landed = None;
            for _ in 0..40 {
                let (o, o1) = (obs(&e, 0), obs(&e, 1));
                if o[77] == 1.0 && e.m.c[0].lp.l.body.pos == from {
                    mine.push(o[78] as f64 * 8.0);
                }
                if o1[83] == 1.0 {
                    // Seen from the other side, 28 blocks ahead and
                    // facing the thrower.
                    theirs.push(28.0 - o1[87] as f64 * 8.0);
                }
                if landed.is_none() && e.episode().landings[0] > 0.0 {
                    let at = e.m.s.p[0].l.body.pos;
                    landed = Some(-yaw.sin() * (at[0] - from[0]) + yaw.cos() * (at[2] - from[2]));
                }
                run(&mut e, 1, |_| IDLE);
            }
            let went = landed.expect("the pearl landed");
            assert!(went > 4.0 && mine.len() >= 3 && theirs.len() >= 3, "{ping} ms, seed {seed}: {went} {} {}", mine.len(), theirs.len());
            for told in mine.iter().chain(&theirs) {
                assert!((told - went).abs() < 0.1, "{ping} ms, seed {seed}: went {went:.2}, told {mine:.2?} and {theirs:.2?}");
            }
        }
    }
}
