//! Retries of lost duels: the moment a few seconds before the first player
//! died is played again, as a new episode.
use pvpsim::env::{Env, Mode, Params, ACT_HEADS};
use pvpsim::mth::Rng;

#[test]
fn a_lost_moment_is_played_again() {
    let mut p = Params::default();
    // Real health, a strong bot, a first player that does nothing useful.
    p.infinite_health = false;
    p.retry_prob = 1.0;
    p.bot_level = [4.0, 4.0];
    let mut e = Env::new(Mode::Bot, p, 9);
    let mut rng = Rng::new(4);
    let act = [0i32; ACT_HEADS];
    let mut episodes = 0.0;
    for _ in 0..40_000 {
        let _ = rng.next_u64();
        e.step(&act);
        if e.total.episodes > episodes {
            episodes = e.total.episodes;
        }
        if e.total.retries >= 2.0 {
            break;
        }
    }
    assert!(e.total.deaths[0] >= 1.0, "the first player died: {:?}", e.total.deaths);
    assert!(e.total.retries >= 1.0, "and played that again: {} retries in {episodes} episodes", e.total.retries);
}
