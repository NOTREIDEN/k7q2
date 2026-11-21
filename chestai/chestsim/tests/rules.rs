//! The rules behind the run-4 parameters: hits by the aimbot, the wave
//! timeout, damage pay only for own mobs, golden apples eaten too early.
use chestsim::env::{Env, Params, OBS_DIM, PARAM_NAMES, PRIV_DIM};
use chestsim::game::{Cfg, Game, Input, PlayerStart, MOB_KINDS};
use chestsim::mth::Rng;
use chestsim::world::World;

fn cfg(silent_aim: bool) -> Cfg {
    Cfg {
        up: 2, down: 2, jitter: 0, speed_amp: 1, cps: 8.0, sword_dmg: 8.0, punch: 2.0, arrow_self_dmg_mult: 0.35,
        leash_r: 6.0, leash_every: 10, reset_cd: (100, 200), pearl_cd: 0, pearl_dmg: 0.0, holo_range: 64.0,
        kind_weights: [1.0, 0.0, 0.0, 0.0], mob_hp: (200.0, 200.0), mob_dmg: (0.5, 0.5), mob_kb_resist: (0.0, 0.0),
        mob_kb_extra: (0.0, 0.0), mob_speed: (0.23, 0.23), mob_attack_cd: (20, 20), ranged_cd: (20, 60),
        proj_dmg: (1.0, 1.0), proj_punch: (0.0, 0.0), slow_prob: 0.0, blind_prob: 0.0, effect_ticks: (40, 80),
        wave_ticks: 300, silent_aim, gapple_waste_abs: 2.0,
    }
}

/// A player next to a ready chest on flat ground, looking away from it.
fn game(c: Cfg) -> Game {
    let _ = MOB_KINDS;
    let start = PlayerStart { yaw: 180.0, hp: 20.0, gapples: 8, pearls: 0, potions: 0, arrows: 64 };
    Game::new(World::flat(32, 24, 32, 8), &[[16, 8, 16]], [16.5, 8.0, 18.5], c, Rng::new(3), &[0], start)
}

/// Hits landed in `ticks` ticks of standing still with the sword, the view
/// turned away and the attack button never pressed.
fn hits(g: &mut Game, ticks: usize) -> (u32, f32, f32) {
    let (mut h, mut dmg, mut dmg_rival) = (0, 0.0, 0.0);
    for _ in 0..ticks {
        // The view keeps away from the chest and the mobs: straight down.
        let inp = Input { dpitch: 7.0, slot: 255, ..Default::default() };
        let ev = g.step(&inp);
        h += ev.hits;
        dmg += ev.dmg_dealt;
        dmg_rival += ev.dmg_rival;
    }
    (h, dmg, dmg_rival)
}

#[test]
fn silent_aim_hits_without_aim_or_click() {
    let mut g = game(cfg(true));
    let (h, dmg, dmg_rival) = hits(&mut g, 120);
    assert!(h >= 3, "the aimbot landed {h} hits");
    assert!(dmg > 0.0 && dmg_rival == 0.0);
    // At most cps clicks a second.
    assert!(g.cp.clicks as f64 <= 8.0 * 120.0 / 20.0 + 1.0);
    let mut g = game(cfg(false));
    assert_eq!(hits(&mut g, 120).0, 0, "without the aimbot no click, no hit");
}

#[test]
fn silent_aim_needs_the_sword() {
    let mut g = game(cfg(true));
    g.cp.slot = 1;
    assert_eq!(hits(&mut g, 120).0, 0);
}

#[test]
fn silent_aim_leaves_rival_waves_alone() {
    let mut g = game(cfg(true));
    // Another player at the chest sets the wave off: it is theirs.
    g.add_rivals(1);
    let (h, dmg, _) = hits(&mut g, 30);
    assert!(g.chests[0].rival_wave);
    assert!(!g.view.mobs.is_empty() && g.view.mobs.iter().all(|m| m.rival));
    let (h2, dmg2, _) = hits(&mut g, 90);
    assert_eq!((h + h2, dmg + dmg2), (0, 0.0), "a rival wave's mobs were hit");
}

#[test]
fn wave_resets_after_its_own_time() {
    for (wt, silent) in [(300i64, false), (200, false)] {
        let mut c = cfg(silent);
        c.wave_ticks = wt;
        let mut g = game(c);
        let mut reset_at = None;
        for t in 0..500 {
            let ev = g.step(&Input { slot: 255, ..Default::default() });
            if ev.resets > 0 {
                reset_at = Some(t);
                break;
            }
        }
        let since = g.chests[0].active_since;
        assert_eq!(reset_at.map(|t| t as i64 - since), Some(wt), "wave of {wt} ticks");
    }
}

#[test]
fn wave_time_is_drawn_per_episode() {
    let mut seen = Vec::new();
    for s in 0..40 {
        let e = Env::new(Params::default(), s);
        let w = e.g.cfg.wave_ticks;
        assert!((300..=400).contains(&w), "{w}");
        seen.push(w);
    }
    seen.sort();
    assert!(seen[0] < seen[39]);
    let mut p = Params::default();
    p.wave_s_min = 10.0;
    p.wave_s_max = 10.0;
    assert_eq!(Env::new(p, 1).g.cfg.wave_ticks, 200);
}

#[test]
fn gapple_with_absorption_left_is_waste() {
    let mut g = game(cfg(true));
    g.sp.hp = 10.0;
    g.sp.absorption = 3.0;
    g.sp.abs_until = 10_000;
    g.cp.slot = 2;
    let mut waste = 0;
    let mut eaten = 0;
    for _ in 0..60 {
        let ev = g.step(&Input { use_: true, slot: 255, ..Default::default() });
        waste += ev.gapple_waste;
        eaten += ev.gapples;
        if eaten > 0 {
            break;
        }
    }
    assert_eq!((eaten, waste), (1, 1));
    // With little absorption left it is not.
    let mut g = game(cfg(true));
    g.sp.hp = 10.0;
    g.sp.absorption = 1.0;
    g.sp.abs_until = 10_000;
    g.cp.slot = 2;
    let mut waste = 0;
    let mut eaten = 0;
    for _ in 0..60 {
        let ev = g.step(&Input { use_: true, slot: 255, ..Default::default() });
        waste += ev.gapple_waste;
        eaten += ev.gapples;
        if eaten > 0 {
            break;
        }
    }
    assert_eq!((eaten, waste), (1, 0));
}

#[test]
fn params_and_layout() {
    for n in ["silent_aim", "wave_s_min", "wave_s_max", "w_dmg_rival", "gapple_waste_abs", "w_punch", "punch_mode", "punch_gain", "punch_window", "punch_every_s"] {
        let i = PARAM_NAMES.iter().position(|x| *x == n).unwrap_or_else(|| panic!("{n}"));
        let mut p = Params::default();
        assert!(p.set(i as i32, 0.25));
    }
    assert_eq!((OBS_DIM, PRIV_DIM), (676, 62));
    let p = Params::default();
    assert_eq!((p.silent_aim, p.wave_s_min, p.wave_s_max, p.w_dmg_rival, p.gapple_waste_abs), (1.0, 15.0, 20.0, 0.0, 2.0));
    assert_eq!((p.w_punch, p.punch_mode, p.punch_gain, p.punch_window, p.punch_every_s), (1.0, 1.0, 3.0, 30.0, 5.0));
    // The game's clicks stay at 6-9 a second, as the bot's in the real game.
    assert_eq!((p.cps_min, p.cps_max), (6.0, 9.0));
}

#[test]
fn episodes_run_with_and_without_the_aimbot() {
    for sa in [0.0, 1.0] {
        let mut p = Params::default();
        p.silent_aim = sa;
        p.rival_prob = 1.0;
        let mut e = Env::new(p, 11);
        let mut obs = vec![0f32; OBS_DIM];
        let mut pv = vec![0f32; PRIV_DIM];
        for _ in 0..3000 {
            let a = e.teacher();
            let (r, done) = e.step(&a);
            assert!(r.is_finite());
            e.write_obs(&mut obs, &mut pv);
            assert!(obs.iter().chain(pv.iter()).all(|x| x.is_finite()));
            if done != 0 {
                e.reset();
            }
        }
    }
}

