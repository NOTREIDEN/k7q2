//! An opponent that takes little knockback takes as little of its vertical
//! part too (`Params::opp_kb_v`), as a Velocity cheat does.
use pvpsim::env::{new_duel, Mode, Params};
use pvpsim::mth::Rng;

#[test]
fn off_by_default() {
    let p = Params::default();
    assert_eq!(p.opp_kb_v, 0.0);
    let mut rng = Rng::new(5);
    for _ in 0..200 {
        let (m, _, _, _) = new_duel(&p, &mut rng, Mode::Bot);
        assert_eq!(m.c[1].edge.kb_v_taken, 1.0);
        assert_eq!(m.c[0].edge.kb_v_taken, 1.0);
    }
    let mut p = Params::default();
    assert!(p.set(219, 0.5) && p.opp_kb_v == 0.5);
}

#[test]
fn it_takes_the_same_share_up_as_sideways() {
    let mut p = Params::default();
    p.insane_prob = 1.0;
    p.opp_kb_v = 1.0;
    let mut rng = Rng::new(9);
    for _ in 0..200 {
        let (m, _, _, _) = new_duel(&p, &mut rng, Mode::Bot);
        let e = m.c[1].edge;
        assert!(e.kb_taken < 0.4);
        assert_eq!(e.kb_v_taken, e.kb_taken);
        assert_eq!(m.c[0].edge.kb_v_taken, 1.0);
    }
}
