//! Ticks of play written out with everything the game client sees and the
//! observation the agent got, for the Java mod to check that it builds the
//! same observation from the same view.
//!
//!     cargo run --release --example dump -- out.txt [episodes] [ticks]
//!
//! One record per line, numbers separated by spaces:
//!   W sx sy sz <cells as hex>         a new episode's world
//!   C n  (x y z)*n                    its chests' blocks
//!   F ...                             one tick (see `frame`)
use chestsim::env::{Env, Params, OBS_DIM, PRIV_DIM, ACT_HEADS};
use chestsim::game::ProjKind;
use std::fmt::Write as _;
use std::io::Write;

fn frame(e: &Env, obs: &[f32]) -> String {
    let g = &e.g;
    let l = &g.cp.l;
    let v = &g.view;
    let mut s = String::from("F");
    let mut p = |x: f64| {
        let _ = write!(s, " {}", x);
    };
    p(v.tick as f64);
    for i in 0..3 {
        p(l.body.pos[i]);
    }
    for i in 0..3 {
        p(l.body.motion[i]);
    }
    p(l.yaw as f64);
    p(l.pitch as f64);
    p(l.body.on_ground as i32 as f64);
    p(l.body.collided_h as i32 as f64);
    p(l.sprinting as i32 as f64);
    p(v.hp as f64);
    p(v.absorption as f64);
    p(v.regen_left as f64);
    p(v.slow_left as f64);
    p(v.blind_left as f64);
    p(v.gapples as f64);
    p(v.pearls as f64);
    p(v.potions as f64);
    p(v.arrows as f64);
    p(v.pearl_cd_left as f64);
    p(g.cp.slot as f64);
    p(g.cp.using as i32 as f64);
    p(g.cp.use_ticks as f64);
    p(g.cp.tokens);
    p(g.cfg.speed_amp as f64);
    p(e.ping_ms);
    p(g.cfg.cps);
    p(g.cfg.punch);
    p(g.cfg.leash_r);
    p(e.extreme as i32 as f64);
    p(e.since_hurt as f64);
    let (pm, pc) = g.pick();
    p(pm.map_or(-1.0, |x| x as f64));
    p(pc.map_or(-1.0, |x| x as f64));
    p(v.rivals.len() as f64);
    for r in &v.rivals {
        for i in 0..3 {
            p(r[i]);
        }
    }
    p(v.mobs.len() as f64);
    for m in &v.mobs {
        p(m.id as f64);
        p(m.kind as i32 as f64);
        for i in 0..3 {
            p(m.pos[i]);
        }
        for i in 0..3 {
            p(m.motion[i]);
        }
        p(m.hp as f64);
        p(m.max_hp as f64);
        p(m.hurt_time as f64);
        p(m.mounted as i32 as f64);
        p(m.hw);
        p(m.h);
    }
    // Holograms the client can see now: distance to the chest within range.
    let pos = l.body.pos;
    let mut seen = Vec::new();
    for (ci, c) in g.chests.iter().enumerate() {
        let cc = c.center();
        let d = ((cc[0] - pos[0]).powi(2) + (cc[1] - pos[1]).powi(2) + (cc[2] - pos[2]).powi(2)).sqrt();
        if d > e.holo_range {
            continue;
        }
        if let Some(&(st, secs)) = v.holo.get(ci) {
            seen.push((ci, st as i32, secs));
        }
    }
    p(seen.len() as f64);
    for (ci, st, secs) in seen {
        p(ci as f64);
        p(st as f64);
        p(secs as f64);
    }
    p(v.projs.len() as f64);
    for pr in &v.projs {
        p(match pr.0 {
            ProjKind::Arrow => 0.0,
            ProjKind::Snowball => 1.0,
            ProjKind::Pearl => 2.0,
            ProjKind::Potion => 3.0,
        });
        p(pr.1 as f64);
        for i in 0..3 {
            p(pr.2[i]);
        }
        for i in 0..3 {
            p(pr.3[i]);
        }
    }
    p(obs.len() as f64);
    for &x in obs {
        p(x as f64);
    }
    s
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).cloned().unwrap_or("dump.txt".into());
    let eps: u64 = args.get(2).and_then(|x| x.parse().ok()).unwrap_or(4);
    let ticks: usize = args.get(3).and_then(|x| x.parse().ok()).unwrap_or(600);
    let mut f = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
    let mut obs = vec![0f32; OBS_DIM];
    let mut pv = vec![0f32; PRIV_DIM];
    for ep in 0..eps {
        let mut p = Params::default();
        p.rival_prob = 0.7;
        p.extreme_share = if ep % 2 == 1 { 1.0 } else { 0.0 };
        let mut e = Env::new(p, 1000 + ep * 7);
        let w = &e.g.world;
        let mut line = format!("W {} {} {} ", w.sx, w.sy, w.sz);
        for c in w.cells() {
            let _ = write!(line, "{:x}", c);
        }
        writeln!(f, "{line}").unwrap();
        let mut cl = format!("C {}", e.g.chests.len());
        for c in &e.g.chests {
            let _ = write!(cl, " {} {} {}", c.block[0], c.block[1], c.block[2]);
        }
        writeln!(f, "{cl}").unwrap();
        e.write_obs(&mut obs, &mut pv);
        writeln!(f, "{}", frame(&e, &obs)).unwrap();
        let mut rng = chestsim::mth::Rng::new(ep + 99);
        for _ in 0..ticks {
            let mut a = e.teacher();
            // Now and then something the teacher would not do.
            if rng.chance(0.1) {
                let dims = chestsim::env::ACT_DIMS;
                for h in 0..ACT_HEADS {
                    if rng.chance(0.3) {
                        a[h] = (rng.f64() * dims[h] as f64) as i32;
                    }
                }
            }
            let (_, done) = e.step(&a);
            if done != 0 {
                break;
            }
            e.write_obs(&mut obs, &mut pv);
            writeln!(f, "{}", frame(&e, &obs)).unwrap();
        }
    }
}
