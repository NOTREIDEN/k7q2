//! Ender pearls as the trainer uses them: the throw action of the
//! environment, what an agent is told about a throw, and the pearls of the
//! scripted bots. How a pearl flies and lands is checked against the real
//! game in `parity.rs` and `reference.rs`.
use pvpsim::bot::{self, BotParams, BotState};
use pvpsim::env::{new_match, Env, Mode, Params, ACT_HEADS, OBS_DIM};
use pvpsim::mth::Rng;
use pvpsim::net::SLOT_SWORD;

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0];

fn params() -> Params {
    let mut p = Params::default();
    p.pearl_prob = 1.0;
    p.pearl_cd_s = [5.0, 5.0];
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
/// while player 1 stands still.
fn run(e: &mut Env, n: usize, mut f: impl FnMut(&Env, usize) -> [i32; ACT_HEADS]) {
    let mut k = 0;
    while k < n {
        if e.pending == 0 {
            let a = f(e, k);
            e.step(&a);
            k += 1;
        } else {
            e.step(&IDLE);
        }
    }
}

fn with(head: usize, v: i32) -> [i32; ACT_HEADS] {
    let mut a = IDLE;
    a[head] = v;
    a
}

#[test]
fn the_throw_action_throws_one_pearl_and_takes_the_sword_again() {
    let mut e = Env::new(Mode::SelfPlay, params(), 1);
    let start = e.m.c[0].lp.l.body.pos;
    run(&mut e, 5, |_, _| IDLE);
    run(&mut e, 1, |_, _| with(8, 1));
    // The pearls are in hand for the one tick of the throw.
    assert_ne!(e.m.c[0].lp.held, SLOT_SWORD);
    assert_eq!(e.m.c[0].lp.pearls, 15);
    run(&mut e, 1, |_, _| IDLE);
    assert_eq!(e.m.c[0].lp.held, SLOT_SWORD);
    run(&mut e, 60, |_, _| IDLE);
    assert_eq!(e.m.s.throws, [1, 0]);
    assert_eq!((e.m.s.p[0].pearls, e.m.c[0].lp.pearls), (15, 15));
    assert_eq!(e.m.s.p[0].held, SLOT_SWORD);
    assert_eq!((e.episode().pearls, e.episode().landings), ([1.0, 0.0], [1.0, 0.0]));
    // Thrown about level, it carried the player a good way.
    let now = e.m.c[0].lp.l.body.pos;
    let moved = (now[0] - start[0]).hypot(now[2] - start[2]);
    assert!(moved > 6.0 && moved < 20.0, "moved {moved}");
    // The opponent saw the pearl and the jump.
    assert!(e.m.c[1].rp.ticks_since_jump < 60);
}

#[test]
fn asking_all_the_time_throws_once_per_wait_and_is_never_refused() {
    let mut e = Env::new(Mode::SelfPlay, params(), 2);
    // Look straight down, so that every pearl lands at once and nothing
    // else happens.
    run(&mut e, 8, |_, _| with(6, 20));
    assert!(e.m.c[0].lp.l.pitch > 89.0);
    let mut held_pearls = 0;
    run(&mut e, 400, |e, _| {
        held_pearls += (e.m.c[0].lp.held != SLOT_SWORD) as i32;
        with(8, 1)
    });
    // Twenty seconds with five seconds between throws.
    assert_eq!(e.m.s.throws, [4, 0]);
    assert_eq!(e.episode().pearls[0], 4.0);
    assert_eq!(held_pearls, 4);
    assert_eq!((e.m.s.p[0].pearls, e.m.c[0].lp.pearls), (12, 12));
}

#[test]
fn a_throw_out_of_a_block_lets_go_of_the_button_first() {
    let mut e = Env::new(Mode::SelfPlay, params(), 3);
    run(&mut e, 10, |_, _| with(7, 1));
    assert!(e.m.c[0].lp.using_item);
    let mut a = with(7, 1);
    a[8] = 1;
    run(&mut e, 1, |_, _| a);
    // Tick one: pearls in hand, button up, nothing thrown yet.
    assert_eq!(e.m.c[0].lp.pearls, 16);
    run(&mut e, 1, |_, _| with(7, 1));
    assert_eq!(e.m.c[0].lp.pearls, 15);
    run(&mut e, 1, |_, _| with(7, 1));
    assert_eq!(e.m.c[0].lp.held, SLOT_SWORD);
    // The button is still held by the agent, so it blocks again.
    run(&mut e, 30, |_, _| with(7, 1));
    assert_eq!(e.m.s.throws, [1, 0]);
    assert!(e.m.c[0].lp.using_item && e.m.s.p[0].using_item);
    assert_eq!(e.m.c[0].lp.pearls, 15);
}

#[test]
fn nothing_is_thrown_without_pearls() {
    let mut p = params();
    p.pearl_prob = 0.0;
    let mut e = Env::new(Mode::SelfPlay, p, 4);
    run(&mut e, 100, |_, _| with(8, 1));
    assert_eq!(e.m.s.throws, [0, 0]);
    assert_eq!(e.m.c[0].lp.held, SLOT_SWORD);
    assert_eq!(e.episode().pearls[0], 0.0);
}

#[test]
fn the_agent_is_told_where_a_throw_would_put_it() {
    let (mut close, mut far) = (0, 0);
    for seed in 0..40 {
        let mut e = Env::new(Mode::SelfPlay, params(), 100 + seed);
        // Look 13 or 27 degrees down, then throw.
        e.m.c[0].lp.l.pitch = 0.0;
        run(&mut e, 1 + (seed as usize % 2), |_, _| with(6, 20));
        let from = e.m.c[0].lp.l.body.pos;
        let mut o = [0.0f32; OBS_DIM];
        e.write_obs(0, &mut o);
        let told = o[92] as f64 * 16.0;
        let ticks = (o[93] * 30.0).round() as i32;
        // The opponent stands twelve blocks ahead.
        assert!((o[94] as f64 * 8.0 - (told - 12.0)).abs() < 0.1 && o[95].abs() < 0.05, "seed {seed}: {} {}", o[94], o[95]);
        run(&mut e, 1, |_, _| with(8, 1));
        let mut landed = None;
        run(&mut e, 40, |e, k| {
            if landed.is_none() && e.episode().landings[0] > 0.0 {
                landed = Some((e.m.s.p[0].l.body.pos, k));
            }
            IDLE
        });
        let (at, _) = landed.expect("the pearl landed");
        let went = (at[0] - from[0]).hypot(at[2] - from[2]);
        assert!(told > 2.0 && told < 9.0 && ticks >= 1 && ticks <= 8, "seed {seed}: told {told} in {ticks} ticks");
        // The random part of a throw can end the flight a tick earlier or
        // later, which is up to a block and a half.
        assert!((went - told).abs() < 1.6, "seed {seed}: told {told}, went {went}");
        if (went - told).abs() < 0.15 {
            close += 1;
        } else {
            far += 1;
        }
    }
    assert!(close >= 32, "close {close}, far {far}");
}

#[test]
fn a_pearl_thrown_straight_up_does_not_put_its_thrower_into_the_floor() {
    let mut e = Env::new(Mode::SelfPlay, params(), 5);
    let floor = e.m.world.floor_y;
    run(&mut e, 8, |_, _| with(6, 0));
    assert!(e.m.c[0].lp.l.pitch < -89.0);
    run(&mut e, 1, |_, _| with(8, 1));
    let mut lowest = f64::MAX;
    run(&mut e, 300, |e, _| {
        lowest = lowest.min(e.m.s.p[0].l.body.pos[1]).min(e.m.c[0].lp.l.body.pos[1]);
        IDLE
    });
    assert_eq!(e.m.s.throws, [1, 0]);
    assert!(e.m.s.pearls.is_empty());
    assert!(lowest >= floor, "lowest {lowest}, floor {floor}");
}

/// Two bots fight, each with its own level and readiness to throw pearls at
/// the opponent and away from it. Returns for every pearl that landed how
/// far from the other player it put its thrower and how far the thrower
/// was moved.
fn bot_pearls(bots: [(u8, f64, f64); 2], duels: u64) -> Vec<(f64, f64)> {
    let mut p = params();
    p.start_dist_min = 2.0;
    p.start_dist_max = 14.0;
    let mut found = Vec::new();
    for d in 0..duels {
        let mut rng = Rng::new(0xBEA7 + d);
        let (mut m, _, _) = new_match(&p, &mut rng, Mode::SelfPlay);
        let bp = bots.map(|(level, attack, escape)| {
            let mut b = BotParams::level(level);
            b.pearl_attack = attack;
            b.pearl_escape = escape;
            b.pearl_cd = m.s.cfg.pearl_cooldown;
            b
        });
        let mut st = [BotState::default(); 2];
        let mut before = [m.s.p[0].l.body.pos, m.s.p[1].l.body.pos];
        let mut next = m.advance();
        let mut rounds = 0;
        while rounds < 1200 {
            let inp = bot::act(&m.c[next], &m.world, &m.ccfg, &bp[next], &mut st[next], &mut rng);
            m.client_tick(next, &inp);
            rounds += (next == 1) as u32;
            next = m.advance();
            for l in m.s.landings.drain(..) {
                let other = m.s.p[1 - l.thrower].l.body.pos;
                let near = (l.pos[0] - other[0]).hypot(l.pos[2] - other[2]);
                let moved = (l.pos[0] - before[l.thrower][0]).hypot(l.pos[2] - before[l.thrower][2]);
                found.push((near, moved));
            }
            m.s.hits.clear();
            before = [m.s.p[0].l.body.pos, m.s.p[1].l.body.pos];
        }
        for i in 0..2 {
            assert_eq!(m.s.p[i].pearls as u32 + m.s.throws[i], 16);
        }
    }
    found
}

#[test]
fn bots_throw_their_pearls_next_to_the_opponent() {
    // Only one of the two has pearls: a target that is thrown elsewhere in
    // the middle of a throw says nothing about the aim.
    let l = bot_pearls([(4, 1.0, 0.0), (4, 0.0, 0.0)], 40);
    let n = l.len() as f64;
    let near = l.iter().filter(|x| x.0 < 4.5).count() as f64;
    let mean = l.iter().map(|x| x.0).sum::<f64>() / n;
    println!("{} pearls, {:.0} % within 4.5 blocks of the opponent, mean {:.2}", n, 100.0 * near / n, mean);
    // More than two a duel, and nine in ten come down in fighting distance.
    assert!(n > 60.0, "{n} pearls");
    assert!(near / n > 0.9, "{near} of {n}");
    assert!(mean > 1.0 && mean < 3.5, "mean {mean}");
}

#[test]
fn bots_throw_their_pearls_away_from_a_combo() {
    // A strong bot without pearls against a weak one that only flees.
    let l = bot_pearls([(4, 0.0, 0.0), (2, 0.0, 1.0)], 30);
    let n = l.len() as f64;
    let far = l.iter().filter(|x| x.0 > 7.0).count() as f64;
    let moved = l.iter().map(|x| x.1).sum::<f64>() / n;
    println!("{} pearls, {:.0} % more than 7 blocks from the opponent, moved {:.1} blocks on average", n, 100.0 * far / n, moved);
    assert!(n > 20.0, "{n} pearls");
    assert!(far / n > 0.85, "{far} of {n}");
    assert!(moved > 8.0, "moved {moved}");
}
