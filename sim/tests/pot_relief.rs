//! A potion thrown low on health pays less for its aim
//! (`Params::pot_relief_hp`, `Params::pot_relief`), and the hardest bot in
//! the corner lessons and those with the back turned
//! (`Params::insane_lessons`).
use pvpsim::env::{new_duel, Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};
use pvpsim::mth::Rng;

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];

fn params(health: f64) -> Params {
    let mut p = Params::default();
    p.infinite_health = false;
    p.prot = [2.0, 2.0];
    p.sharp = [2.0, 2.0];
    p.potions = [7.0, 7.0];
    p.start_hurt = 1.0;
    p.start_health = [health, health];
    p.pearl_prob = 0.0;
    p.start_dist_min = 12.0;
    p.start_dist_max = 12.0;
    p.start_yaw_noise = 0.0;
    p.ep_ticks_min = 100_000.0;
    p.ep_ticks_max = 100_000.0;
    p.kb_random = 0.0;
    p.opp_edge_prob = 0.0;
    p.w_waste = 8.0;
    p.w_miss = 4.0;
    p.w_pot_far = 2.0;
    p
}

fn run(e: &mut Env, n: usize, mut f: impl FnMut(usize) -> [i32; ACT_HEADS]) -> [f32; 2] {
    let (mut fo, mut fp) = ([0.0f32; OBS_DIM], [0.0f32; PRIV_DIM]);
    let mut paid = [0.0f32; 2];
    let mut k = 0;
    while k < n {
        if e.pending == 0 {
            e.step(&f(k));
            k += 1;
        } else {
            e.step(&IDLE);
        }
        let (r, _) = e.take_outcome(&mut fo, &mut fp);
        paid[e.pending] += r;
    }
    paid
}

/// A potion thrown straight ahead, where it reaches nobody: what its
/// thrower was paid from the throw on, against what it was paid without
/// any of the accuracy costs.
fn thrown_away(p: Params) -> (f32, f32) {
    let go = |p: Params| {
        let mut e = Env::new(Mode::SelfPlay, p, 3);
        let mut a = IDLE;
        a[9] = 1;
        let x = run(&mut e, 1, |_| a);
        let y = run(&mut e, 40, |_| IDLE);
        assert_eq!((e.episode().splashes[0], e.episode().pot_offer[0]), (1.0, 0.0));
        (x[0] + y[0], e.m.s.cfg.hit_health())
    };
    let mut free = p.clone();
    free.w_waste = 0.0;
    free.w_miss = 0.0;
    free.w_pot_far = 0.0;
    let (plain, hh) = go(free);
    let (paid, _) = go(p);
    ((plain - paid) * hh, hh)
}

#[test]
fn a_potion_thrown_low_pays_the_share_asked_for_of_its_aim() {
    // All eight broke away: 8 * 8 + 4 * 2 + 2 * 8 = 88, in hits.
    let (full, _) = thrown_away(params(6.0));
    assert!((full - 88.0).abs() < 1e-2, "{full}");
    let mut p = params(6.0);
    p.pot_relief_hp = 8.0;
    p.pot_relief = 0.25;
    let (low, _) = thrown_away(p);
    assert!((low - 22.0).abs() < 1e-2, "{low}");
    // At the health asked for, too.
    let mut p = params(8.0);
    p.pot_relief_hp = 8.0;
    p.pot_relief = 0.25;
    let (at, _) = thrown_away(p);
    assert!((at - 22.0).abs() < 1e-2, "{at}");
}

#[test]
fn with_more_health_it_pays_all_of_it() {
    let mut p = params(12.0);
    p.pot_relief_hp = 8.0;
    p.pot_relief = 0.25;
    let (paid, _) = thrown_away(p);
    assert!((paid - 88.0).abs() < 1e-2, "{paid}");
}

#[test]
fn the_hardest_bot_comes_in_the_lessons_asked_for_only() {
    let insane = |lessons: u32, share: f64, mode: Mode| {
        let mut p = Params::default();
        p.lesson_prob = 1.0;
        p.lessons = lessons;
        p.insane_lessons = share;
        let mut rng = Rng::new(11);
        let mut n = 0;
        for _ in 0..200 {
            let (m, _, bot, lesson) = new_duel(&p, &mut rng, mode);
            assert!(lesson != 0);
            if bot.perfect_pot {
                n += 1;
                assert!(m.c[1].edge.reach >= 3.0);
                assert!(matches!(lesson, 7 | 8), "lesson {lesson}");
            }
        }
        n
    };
    assert_eq!(insane(64 | 128, 1.0, Mode::Bot), 200);
    let half = insane(64 | 128, 0.5, Mode::Bot);
    assert!((60..=140).contains(&half), "{half}");
    assert_eq!(insane(64 | 128, 0.0, Mode::Bot), 0);
    assert_eq!(insane(1 | 2 | 4 | 256, 1.0, Mode::Bot), 0);
    assert_eq!(insane(64 | 128, 1.0, Mode::SelfPlay), 0);
}

#[test]
fn off_by_default() {
    let p = Params::default();
    assert_eq!((p.pot_relief_hp, p.pot_relief, p.insane_lessons), (0.0, 1.0, 0.0));
    let mut p = Params::default();
    assert!(p.set(206, 9.0) && p.set(207, 0.3) && p.set(208, 0.4));
    assert_eq!((p.pot_relief_hp, p.pot_relief, p.insane_lessons), (9.0, 0.3, 0.4));
}
