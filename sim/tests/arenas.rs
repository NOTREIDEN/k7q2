//! Smaller arenas (`Params::small_arena_prob`): duels in arenas of a size
//! drawn per duel, with both players inside its walls.
use pvpsim::env::{new_duel, Mode, Params};
use pvpsim::mth::Rng;

#[test]
fn smaller_arenas_come_in_the_sizes_asked_for_with_both_players_inside() {
    let mut p = Params::default();
    p.small_arena_prob = 1.0;
    p.arena_half_min = 6.0;
    p.lesson_prob = 0.5;
    p.lessons = 511;
    let mut rng = Rng::new(7);
    let mut sizes = std::collections::BTreeSet::new();
    for _ in 0..400 {
        let (m, _, _, _) = new_duel(&p, &mut rng, Mode::Bot);
        let h = m.world.half;
        assert!((6.0..=24.0).contains(&h) && h.fract() == 0.0, "{h}");
        sizes.insert(h as i32);
        for i in 0..2 {
            let b = m.s.p[i].l.body.pos;
            assert!(b[0].abs() < h && b[2].abs() < h, "player {i} at {b:?} in an arena of half {h}");
        }
    }
    assert!(sizes.len() >= 15, "{sizes:?}");
}

#[test]
fn without_them_the_arena_is_the_one_set() {
    let p = Params::default();
    let mut rng = Rng::new(7);
    for _ in 0..50 {
        let (m, _, _, _) = new_duel(&p, &mut rng, Mode::Bot);
        assert_eq!(m.world.half, 24.0);
    }
}
