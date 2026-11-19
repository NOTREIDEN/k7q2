//! The bot that does not hit into a block (`Params::bot_select`) and the
//! one that punishes a drink or a meal (`Params::bot_punish`): both off by
//! default, set by their ids, and the selector holds its click while it
//! sees player 0 block, but no longer than `select_wait` ticks.
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};

const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];
const STEPS: usize = 600;

fn params(select: f64) -> Params {
    let mut p = Params::default();
    p.pearl_prob = 0.0;
    p.start_dist_min = 3.0;
    p.start_dist_max = 3.0;
    p.insane_prob = 1.0;
    p.ep_ticks_min = 2000.0;
    p.ep_ticks_max = 2000.0;
    p.kb_random = 0.0;
    p.tiny_kb_prob = 1.0;
    p.tiny_kb_h = [0.08, 0.32];
    p.tiny_kb_v = [0.15, 0.35];
    p.bot_select = select;
    p.select_block = 0.0;
    p
}

/// The bot's hits on player 0, which stands still and, with `block`,
/// holds its sword up for good.
fn hits(p: Params, seed: u64, block: bool) -> (f64, bool) {
    let mut e = Env::new(Mode::Bot, p, seed);
    if block {
        e.m.s.p[0].autoblock = true;
        e.m.s.p[0].set_using(true);
    }
    let (mut fobs, mut fpriv) = (vec![0f32; OBS_DIM], vec![0f32; PRIV_DIM]);
    let select = e.bot.select;
    for _ in 0..STEPS {
        e.step(&IDLE);
        let (_, d) = e.take_outcome(&mut fobs, &mut fpriv);
        if d != 0 {
            break;
        }
    }
    (e.episode().hits[1], select)
}

#[test]
fn off_by_default() {
    let p = Params::default();
    assert_eq!(p.bot_select, 0.0);
    assert_eq!(p.bot_punish, 0.0);
    for seed in 1..4 {
        let e = Env::new(Mode::Bot, params(0.0), seed);
        assert!(!e.bot.select && !e.bot.punish);
    }
}

#[test]
fn set_by_id() {
    let mut p = Params::default();
    for (id, v) in [(225, 1.0), (226, 5.0), (227, 0.25), (228, 0.5)] {
        assert!(p.set(id, v));
    }
    assert_eq!((p.bot_select, p.select_wait, p.select_block, p.bot_punish), (1.0, 5.0, 0.25, 0.5));
    let mut q = params(1.0);
    q.bot_punish = 1.0;
    let e = Env::new(Mode::Bot, q, 3);
    assert!(e.bot.select && e.bot.punish);
    assert_eq!(e.bot.select_wait, 8);
    // Not in self-play.
    let e = Env::new(Mode::SelfPlay, q, 3);
    assert!(!e.bot.select && !e.bot.punish);
}

#[test]
fn waits_for_the_block_to_drop() {
    let (mut plain_open, mut plain_shut, mut sel_open, mut sel_shut) = (0.0, 0.0, 0.0, 0.0);
    for seed in 1..9 {
        plain_open += hits(params(0.0), seed, false).0;
        plain_shut += hits(params(0.0), seed, true).0;
        let (h, s) = hits(params(1.0), seed, false);
        assert!(s);
        sel_open += h;
        sel_shut += hits(params(1.0), seed, true).0;
    }
    // Against an open guard it hits as often as ever.
    assert!(sel_open > 0.85 * plain_open, "{sel_open} vs {plain_open}");
    // Against a block that never drops it waits, but still hits.
    assert!(sel_shut < 0.8 * plain_shut, "{sel_shut} vs {plain_shut}");
    assert!(sel_shut > 0.3 * plain_shut, "{sel_shut} vs {plain_shut}");
}
