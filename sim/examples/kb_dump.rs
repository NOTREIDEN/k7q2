//! Hits as the simulator's server answers them, for the test server's
//! plugin (plugin/) to be checked against: for each, the knockback
//! numbers, where and how the two players were, and the velocity sent to
//! the one that was hit.
//!
//!     cargo run --release --example kb_dump -- hits.txt [count]
//!
//! One hit per line. `A` is the game's own knockback with other numbers
//! (`KbProfile`), `B` the kind server plugins set (`KbPlugin`). Whole
//! numbers are written as they are, the rest as the bits of the double or
//! float in hex.
use pvpsim::mth::Rng;
use pvpsim::net::{C2S, S2C};
use pvpsim::server::{KbPlugin, KbProfile, Server, ServerCfg, ServerPlayer};
use pvpsim::world::World;
use std::fmt::Write as _;

fn d(x: f64) -> String {
    format!("{:x}", x.to_bits())
}

fn f(x: f32) -> String {
    format!("{:x}", x.to_bits())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("the file to write");
    let count: usize = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(20000);
    let world = World::arena(4.0, 24.0, true);
    let mut rng = Rng::new(5);
    let mut text = String::new();
    let mut made = 0;
    while made < count {
        let plugin = rng.chance(0.5);
        let kb = if plugin || rng.chance(0.1) {
            KbProfile::VANILLA
        } else {
            let v = rng.range(0.2, 0.5);
            KbProfile {
                friction: rng.range(1.0, 4.0),
                horizontal: rng.range(0.2, 0.6),
                vertical: v,
                vertical_limit: if rng.chance(0.5) { v } else { rng.range(0.2, 0.5) },
                extra_horizontal: rng.range(0.0, 0.8) as f32,
                extra_vertical: rng.range(0.0, 0.2),
            }
        };
        let k = KbPlugin {
            h_ground: rng.range(0.2, 0.6),
            h_air: rng.range(0.2, 0.6),
            h_sprint: rng.range(0.0, 0.8),
            v_ground: rng.range(0.2, 0.5),
            v_air: rng.range(0.1, 0.5),
            v_sprint: rng.range(0.0, 0.2),
            stop_sprint: rng.chance(0.5),
            y_limit: if rng.chance(0.7) { rng.range(0.3, 1.5) } else { -1.0 },
            v_after_limit: rng.range(-0.2, 0.1),
        };
        // The attacker somewhere around the victim, looking anywhere.
        let ang = rng.range(0.0, std::f64::consts::TAU);
        let dist = if rng.chance(0.02) { 0.0 } else { rng.range(0.3, 3.5) };
        let vpos = [rng.range(-8.0, 8.0), 4.0 + if rng.chance(0.5) { 0.0 } else { rng.range(0.0, 2.0) }, rng.range(-8.0, 8.0)];
        let apos = [vpos[0] + dist * ang.cos(), 4.0 + if rng.chance(0.6) { 0.0 } else { rng.range(0.0, 1.2) }, vpos[2] + dist * ang.sin()];
        let yaw = rng.range(-720.0, 720.0) as f32;
        let mut a = ServerPlayer::new(apos, yaw, rng.range(-90.0, 90.0) as f32, 1);
        let mut v = ServerPlayer::new(vpos, rng.range(-180.0, 180.0) as f32, 0.0, 1);
        let server_sprint = rng.chance(0.5);
        let client_sprint = if rng.chance(0.7) { server_sprint } else { !server_sprint };
        a.l.sprinting = server_sprint;
        a.client_sprint = client_sprint;
        let ench = if rng.chance(0.2) { 1 + (rng.next_u64() % 2) as i32 } else { 0 };
        let motion = if rng.chance(0.3) {
            [0.0, -0.0784000015258789, 0.0]
        } else {
            [rng.range(-0.6, 0.6), rng.range(-0.8, 0.5), rng.range(-0.6, 0.6)]
        };
        v.l.body.motion = motion;
        let ground = rng.chance(0.5);
        v.l.body.on_ground = ground;
        v.last_ground_y = vpos[1] - if rng.chance(0.5) { 0.0 } else { rng.range(0.0, 1.6) };
        v.touched_top = rng.chance(0.15);
        let (last_ground, touched) = (v.last_ground_y, v.touched_top);
        let mut cfg = ServerCfg::default();
        cfg.kb_ench = ench;
        let mut s = Server::new(a, v, kb, cfg);
        if plugin {
            s.plugin = Some(k);
        }
        let mut out = Vec::new();
        let mut hit_rng = Rng::new(made as u64 + 1);
        s.handle(0, C2S::Attack, &world, &mut hit_rng, &mut out);
        let sent = out.iter().find_map(|(to, p)| match p {
            S2C::Velocity { own: true, v } if *to == 1 => Some(*v),
            _ => None,
        });
        // Out of reach, or the two on the very same spot, where the game draws a direction.
        let Some(sent) = sent else { continue };
        if dist == 0.0 {
            continue;
        }
        let mut line = String::new();
        if plugin {
            write!(
                line,
                "B {} {} {} {} {} {} {} {} {}",
                d(k.h_ground), d(k.h_air), d(k.h_sprint), d(k.v_ground), d(k.v_air), d(k.v_sprint), k.stop_sprint as i32, d(k.y_limit), d(k.v_after_limit)
            )
            .unwrap();
        } else {
            write!(
                line,
                "A {} {} {} {} {} {}",
                d(kb.friction), d(kb.horizontal), d(kb.vertical), d(kb.vertical_limit), f(kb.extra_horizontal), d(kb.extra_vertical)
            )
            .unwrap();
        }
        write!(
            line,
            " | {} {} {} {} {} {} {} | {} {} {} {} {} {} {} {} {} | {} {} {} {}",
            d(apos[0]), d(apos[1]), d(apos[2]), f(s.p[0].l.yaw), server_sprint as i32, client_sprint as i32, ench,
            d(vpos[0]), d(vpos[1]), d(vpos[2]), d(motion[0]), d(motion[1]), d(motion[2]), ground as i32, d(last_ground), touched as i32,
            sent[0], sent[1], sent[2], s.p[1].touched_top as i32
        )
        .unwrap();
        text.push_str(&line);
        text.push('\n');
        made += 1;
    }
    std::fs::write(path, text).unwrap();
    println!("{path}: {made} hits");
}
