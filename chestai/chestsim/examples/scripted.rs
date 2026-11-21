//! A hand-written player, to check that the game can be won: walk to the
//! chest that matters, kill what it spawns, claim it. And a punch shot.
use chestsim::aabb::EYE_HEIGHT;
use chestsim::env::{Env, Params, PITCH_BINS, YAW_BINS};
use chestsim::game::{ChestState, Game, Input, PlayerStart, Cfg, MOB_KINDS};
use chestsim::mth::Rng;
use chestsim::world::World;

fn nearest_bin(bins: &[f32], want: f32) -> i32 {
    let mut b = 0;
    for (i, &x) in bins.iter().enumerate() {
        if (x - want).abs() < (bins[b] - want).abs() {
            b = i;
        }
    }
    b as i32
}

fn aim(g: &Game, at: [f64; 3]) -> (f32, f32) {
    let l = &g.cp.l;
    let eye = [l.body.pos[0], l.body.pos[1] + EYE_HEIGHT, l.body.pos[2]];
    let (dx, dy, dz) = (at[0] - eye[0], at[1] - eye[1], at[2] - eye[2]);
    let want = (dz.atan2(dx) * 180.0 / std::f64::consts::PI) as f32 - 90.0;
    let mut e = (want - l.yaw) % 360.0;
    if e > 180.0 { e -= 360.0 }
    if e < -180.0 { e += 360.0 }
    let h = (dx * dx + dz * dz).sqrt();
    let wp = -(dy.atan2(h) * 180.0 / std::f64::consts::PI) as f32;
    (e, wp - l.pitch)
}

fn act(env: &Env) -> [i32; 9] {
    let g = &env.g;
    let pos = g.cp.l.body.pos;
    let mut a = [1, 1, 0, 1, 7, 5, 0, 0, 0];
    // A mob near: fight it.
    let mob = g.view.mobs.iter().min_by(|x, y| {
        let dx = (x.pos[0] - pos[0]).powi(2) + (x.pos[2] - pos[2]).powi(2);
        let dy = (y.pos[0] - pos[0]).powi(2) + (y.pos[2] - pos[2]).powi(2);
        dx.partial_cmp(&dy).unwrap()
    });
    if let Some(m) = mob {
        let d = ((m.pos[0] - pos[0]).powi(2) + (m.pos[2] - pos[2]).powi(2)).sqrt();
        if d < 10.0 {
            let (ey, ep) = aim(g, [m.pos[0], m.pos[1] + m.h * 0.6, m.pos[2]]);
            a[4] = nearest_bin(&YAW_BINS, ey);
            a[5] = nearest_bin(&PITCH_BINS, ep);
            a[0] = if d > 2.5 { 2 } else { 1 };
            a[2] = g.cp.l.body.collided_h as i32;
            a[6] = (ey.abs() < 15.0 && d < 3.5) as i32;
            a[8] = if g.cp.slot != 0 { 1 } else { 0 };
            if g.view.hp < 10.0 && g.view.absorption == 0.0 && g.view.gapples > 0 && d > 3.0 {
                a[8] = 3;
                a[7] = 1;
            }
            return a;
        }
    }
    // Otherwise the chest that matters most: claimable, ready, soonest.
    let mut best = None;
    for (ci, c) in g.chests.iter().enumerate() {
        let cc = c.center();
        let d = ((cc[0] - pos[0]).powi(2) + (cc[2] - pos[2]).powi(2)).sqrt();
        let s = match c.state {
            ChestState::Claimable => d,
            ChestState::Ready => 100.0 + d,
            ChestState::Active => 50.0 + d,
            ChestState::Cooldown => 1000.0 + (c.ready_at - g.t) as f64 / 20.0 + d,
        };
        if best.map_or(true, |(b, _)| s < b) {
            best = Some((s, ci));
        }
    }
    if let Some((_, ci)) = best {
        let c = &g.chests[ci];
        let cc = c.center();
        let d = ((cc[0] - pos[0]).powi(2) + (cc[2] - pos[2]).powi(2)).sqrt();
        let (ey, ep) = aim(g, cc);
        a[4] = nearest_bin(&YAW_BINS, ey);
        if c.state == ChestState::Claimable {
            a[5] = nearest_bin(&PITCH_BINS, ep);
            a[0] = if d > 2.5 { 2 } else { 1 };
            a[7] = (d < 4.0 && ey.abs() < 10.0) as i32;
            a[6] = 0;
        } else {
            a[5] = nearest_bin(&PITCH_BINS, -g.cp.l.pitch);
            a[0] = if d > 3.0 { 2 } else { 1 };
        }
        a[2] = (g.cp.l.body.collided_h || g.world.in_water(&g.cp.l.body.bb)) as i32;
    }
    a
}

fn main() {
    let mut p = Params::default();
    p.extreme_share = 0.0;
    p.mob_hp_max = 30.0;
    p.mob_dmg_max = 1.0;
    p.w_horseman = 0.0;
    p.w_golem = 0.0;
    p.w_rider = 0.0;
    p.slow_prob = 0.0;
    p.blind_prob = 0.0;
    let mut tot = [0.0f64; 20];
    for seed in 0..20u64 {
        let mut env = Env::new(p.clone(), seed + 100);
        loop {
            let a = act(&env);
            let (_, done) = env.step(&a);
            if done != 0 {
                break;
            }
        }
        for i in 0..20 {
            tot[i] += env.stats[i];
        }
    }
    println!("scripted (easy zombies): episodes={} claims={} kills={} waves={} resets={} deaths={} hits={} leash_tps={} gapples={}",
        tot[0] + 20.0, tot[2], tot[3], tot[4], tot[5], tot[6], tot[10], tot[9], tot[14]);

    // Punch shot on flat ground: sprint forward, draw 6 ticks, release at -55 pitch.
    let world = World::flat(64, 32, 64, 8);
    let cfg = Cfg {
        up: 2, down: 2, jitter: 0, speed_amp: 1, cps: 8.0, sword_dmg: 8.0, punch: 2.0, arrow_self_dmg_mult: 0.35,
        leash_r: 6.0, leash_every: 10, reset_cd: (100, 200), pearl_cd: 0, pearl_dmg: 0.0, holo_range: 64.0,
        kind_weights: [1.0; MOB_KINDS], mob_hp: (20.0, 20.0), mob_dmg: (1.0, 1.0), mob_kb_resist: (0.0, 0.0),
        mob_kb_extra: (0.0, 0.0), mob_speed: (0.23, 0.23), mob_attack_cd: (20, 20), ranged_cd: (20, 60),
        proj_dmg: (1.0, 1.0), proj_punch: (0.0, 0.0), slow_prob: 0.0, blind_prob: 0.0, effect_ticks: (40, 80),
        wave_ticks: 300, silent_aim: true, gapple_waste_abs: 2.0,
    };
    let mut best = 0.0f64;
    for draw in 3..12 {
        for pitch in [-90.0f32, -80.0, -70.0, -60.0, -50.0, -40.0] {
            let start = PlayerStart { yaw: 0.0, hp: 20.0, gapples: 0, pearls: 0, potions: 0, arrows: 64 };
            let mut g = Game::new(world.clone(), &[[60, 8, 60]], [10.5, 8.0, 10.5], cfg.clone(), Rng::new(5), &[9999], start);
            g.cp.slot = 1;
            let mut fired = false;
            let mut t_fire = 0;
            let mut boost = 0.0f64;
            let z0;
            {
                z0 = g.cp.l.body.pos[2];
            }
            for t in 0..80 {
                let mut inp = Input { forward: 1.0, sprint: true, slot: 255, ..Default::default() };
                if t >= 10 && !fired {
                    if t < 10 + draw {
                        inp.use_ = true;
                        if t == 10 {
                            inp.dpitch = pitch;
                        }
                    } else {
                        fired = true;
                        t_fire = t;
                    }
                }
                let ev = g.step(&inp);
                if ev.self_punch > 0 {
                    let m = g.sp.motion;
                    boost = (m[0] * m[0] + m[2] * m[2]).sqrt();
                }
            }
            let moved = g.cp.l.body.pos[2] - z0;
            if boost > 0.0 {
                println!("draw {draw:2} pitch {pitch:4}: self-hit, server push {boost:.2} b/t, moved {moved:.1} blocks in 80 ticks (fired at {t_fire})");
            }
            best = best.max(boost);
        }
    }
    println!("best punch push {:.2}", best);
}
