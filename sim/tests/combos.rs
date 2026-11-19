//! Being comboed (`Params::w_comboed`), runs that end by time
//! (`Params::combo_gap`), the stats of combos, and the bot that spaces its
//! combos (`Params::bot_spacer`).
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};

const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];
const STEPS: usize = 900;

fn params() -> Params {
    let mut p = Params::default();
    p.pearl_prob = 0.0;
    p.start_dist_min = 3.0;
    p.start_dist_max = 3.0;
    p.bot_level = [4.0, 4.0];
    p.ep_ticks_min = 2000.0;
    p.ep_ticks_max = 2000.0;
    p.w_combo = 0.2;
    p
}

/// Player 0 stands still while the bot hits it, never hitting back: its
/// rewards added up, the rounds (steps) on which the bot's hits landed, and
/// the duel's stats.
fn idle(p: Params, seed: u64) -> (f32, Vec<usize>, pvpsim::env::Stats) {
    let mut e = Env::new(Mode::Bot, p, seed);
    let (mut fobs, mut fpriv) = (vec![0f32; OBS_DIM], vec![0f32; PRIV_DIM]);
    let mut total = 0.0;
    let mut at = Vec::new();
    let mut seen = 0.0;
    for s in 0..STEPS {
        e.step(&IDLE);
        let (r, d) = e.take_outcome(&mut fobs, &mut fpriv);
        assert_eq!(d, 0);
        total += r;
        let h = e.episode().hits[1];
        assert_eq!(e.episode().hits[0], 0.0);
        if h > seen {
            assert_eq!(h, seen + 1.0);
            at.push(s);
            seen = h;
        }
    }
    (total, at, *e.episode())
}

/// What `w_comboed` of one takes for runs split where a hit comes more
/// than `gap` rounds after the one before (0: never split).
fn expected(at: &[usize], gap: usize, free: f64, cap: f64) -> f32 {
    let mut cost = 0.0;
    let mut k = 0;
    for (i, &s) in at.iter().enumerate() {
        k = if gap > 0 && i > 0 && s - at[i - 1] > gap { 1 } else { k + 1 };
        cost += (k as f64 - free).clamp(0.0, cap);
    }
    cost as f32
}

#[test]
fn being_comboed_costs_the_victim_by_the_length_of_the_run() {
    let (plain, at, st) = idle(params(), 3);
    assert!(at.len() >= 20, "the bot should keep hitting: {}", at.len());
    // One run all along: counted once, every hit from the third on.
    assert_eq!(st.combo3_taken[0], 1.0);
    assert_eq!(st.combo_hits_taken[0], at.len() as f64 - 2.0);
    assert_eq!(st.combo3_taken[1], 0.0);
    assert_eq!(st.combo_breaks, [0.0, 0.0]);
    assert_eq!(st.combo_escapes, [0.0, 0.0]);
    for (w, free, cap) in [(1.0f32, 2.0, 4.0), (0.5, 1.0, 2.0), (0.3, 0.0, 10.0)] {
        let mut p = params();
        p.w_comboed = w;
        p.comboed_free = free;
        p.comboed_cap = cap;
        let (r, at2, _) = idle(p, 3);
        // It draws nothing and changes nothing but the victim's reward.
        assert_eq!(at, at2);
        let want = w * expected(&at, 0, free, cap);
        assert!((plain - r - want).abs() < 1e-2 * want.max(1.0), "{w} {free} {cap}: {plain} {r} {want}");
    }
}

#[test]
fn a_run_ends_when_the_next_hit_is_late() {
    let (plain, at, _) = idle(params(), 5);
    // Shorter than the invulnerable ticks: every hit starts a run.
    let mut p = params();
    p.w_comboed = 1.0;
    p.combo_gap = 5.0;
    let (r, at2, st) = idle(p, 5);
    assert_eq!(at, at2);
    assert_eq!(st.combo3_taken[0], 0.0);
    assert_eq!(st.combo_hits_taken[0], 0.0);
    assert_eq!(st.combo_escapes[0], 0.0);
    // `w_combo` pays the attacker nothing either, which leaves player 0's
    // reward at what the hits cost.
    assert!(r > plain, "{r} {plain}");
    // In between: runs split where the bot took longer than 12 rounds.
    let gap = 12;
    let mut p = params();
    p.w_comboed = 1.0;
    p.combo_gap = gap as f64;
    p.w_combo = 0.0;
    let (r, at3, st) = idle(p, 5);
    assert_eq!(at, at3);
    let mut q = params();
    q.w_combo = 0.0;
    let (base, _, _) = idle(q, 5);
    let want = expected(&at, gap, 2.0, 4.0);
    assert!((base - r - want).abs() < 1e-2 * want.max(1.0), "{base} {r} {want}");
    let runs = 1 + at.windows(2).filter(|w| w[1] - w[0] > gap).count();
    assert!(runs > 1 && runs < at.len(), "{runs} runs of {} hits", at.len());
    // The runs of two or more that ran out, each counted for the victim.
    let mut escapes = 0;
    let mut k = 1;
    for w in at.windows(2) {
        if w[1] - w[0] > gap {
            escapes += (k >= 2) as i32;
            k = 1;
        } else {
            k += 1;
        }
    }
    assert!((st.combo_escapes[0] - escapes as f64).abs() <= 1.0, "{} {escapes}", st.combo_escapes[0]);
}

/// The feet to feet distances of the bot's hits on a player that stands
/// still, and how many there were.
fn hit_dist(spacer: f64, seed: u64) -> (f64, usize) {
    let mut p = params();
    p.insane_prob = 1.0;
    p.bot_spacer = spacer;
    let mut e = Env::new(Mode::Bot, p, seed);
    let (mut fobs, mut fpriv) = (vec![0f32; OBS_DIM], vec![0f32; PRIV_DIM]);
    let (mut sum, mut n, mut seen) = (0.0, 0, 0.0);
    for _ in 0..STEPS {
        e.step(&IDLE);
        e.take_outcome(&mut fobs, &mut fpriv);
        let h = e.episode().hits[1];
        if h > seen {
            seen = h;
            let (a, b) = (e.m.s.p[0].l.body.pos, e.m.s.p[1].l.body.pos);
            sum += (a[0] - b[0]).hypot(a[2] - b[2]);
            n += 1;
        }
    }
    (sum / n.max(1) as f64, n)
}

#[test]
fn a_spacer_hits_from_further_away_and_as_often() {
    let (mut plain, mut spaced) = ((0.0, 0), (0.0, 0));
    for seed in 1..4 {
        let a = hit_dist(0.0, seed);
        let b = hit_dist(1.0, seed);
        plain = (plain.0 + a.0, plain.1 + a.1);
        spaced = (spaced.0 + b.0, spaced.1 + b.1);
    }
    let (dp, ds) = (plain.0 / 3.0, spaced.0 / 3.0);
    eprintln!("dist {dp} {ds} hits {} {}", plain.1, spaced.1);
    assert!(ds > dp + 0.3, "{dp} {ds}");
    assert!(spaced.1 as f64 >= 0.8 * plain.1 as f64, "{} {}", plain.1, spaced.1);
}
