//! Checks against numbers that are well known for 1.8.9 and against
//! behaviours read from the game source. The bit-exact comparison against
//! the running game lives in the parity rig, not here.
use pvpsim::client::{ClientCfg, Input};
use pvpsim::net::{C2S, S2C};
use pvpsim::server::{KbProfile, ServerCfg};
use pvpsim::sim::{Match, NetCfg, Spawn};
use pvpsim::world::World;

fn duel(a: [f64; 3], yaw_a: f32, b: [f64; 3], yaw_b: f32, speed_amp: i8, ping_ms: i64) -> Match {
    let net = NetCfg { up_us: [ping_ms * 500; 2], down_us: [ping_ms * 500; 2], jitter_us: [0; 2] };
    let mut m = Match::new(
        World::arena(0.0, 200.0, true),
        [Spawn { pos: a, yaw: yaw_a, pitch: 0.0 }, Spawn { pos: b, yaw: yaw_b, pitch: 0.0 }],
        speed_amp,
        KbProfile::VANILLA,
        ServerCfg::default(),
        ClientCfg::with_sensitivity(0.5),
        net,
        [0, 17_000, 31_000],
        7,
    );
    // Fixed tracker phase so the tests do not depend on the seed.
    m.s.p[0].update_counter = 2;
    m.s.p[1].update_counter = 2;
    m
}

/// Tick both clients once with the given inputs.
fn step(m: &mut Match, i0: &Input, i1: &Input) {
    for _ in 0..2 {
        let id = m.advance();
        m.client_tick(id, if id == 0 { i0 } else { i1 });
    }
}

fn run_speed(inp: Input, speed_amp: i8, ticks: usize) -> f64 {
    let mut m = duel([0.0, 0.0, 0.0], 0.0, [50.0, 0.0, 0.0], 0.0, speed_amp, 20);
    let idle = Input::default();
    let mut last = 0.0;
    for _ in 0..ticks {
        let z0 = m.c[0].lp.l.body.pos[2];
        step(&mut m, &inp, &idle);
        last = m.c[0].lp.l.body.pos[2] - z0;
    }
    last * 20.0
}

#[test]
fn mouse_one_count_is_point_fifteen_degrees() {
    let c = ClientCfg::with_sensitivity(0.5);
    assert_eq!(c.mouse_f1, 1.0);
}

#[test]
fn ground_speeds() {
    let walk = Input { forward: 1, ..Default::default() };
    let sprint = Input { forward: 1, sprint_key: true, ..Default::default() };
    let w = run_speed(walk, -1, 200);
    let s = run_speed(sprint, -1, 200);
    assert!((w - 4.317).abs() < 0.002, "walk {w}");
    assert!((s - 5.612).abs() < 0.002, "sprint {s}");
    // Speed II multiplies the attribute by 1.4.
    let w2 = run_speed(walk, 1, 200);
    let s2 = run_speed(sprint, 1, 200);
    assert!((w2 - 4.317 * 1.4).abs() < 0.003, "walk speed II {w2}");
    assert!((s2 - 5.612 * 1.4).abs() < 0.003, "sprint speed II {s2}");
}

#[test]
fn sprint_jump_average_speed() {
    let mut m = duel([0.0, 0.0, 0.0], 0.0, [50.0, 0.0, 0.0], 0.0, -1, 20);
    let inp = Input { forward: 1, sprint_key: true, jump: true, ..Default::default() };
    let idle = Input::default();
    for _ in 0..200 {
        step(&mut m, &inp, &idle);
    }
    let z0 = m.c[0].lp.l.body.pos[2];
    for _ in 0..240 {
        step(&mut m, &inp, &idle);
    }
    let v = (m.c[0].lp.l.body.pos[2] - z0) / 240.0 * 20.0;
    assert!((v - 7.127).abs() < 0.02, "sprint jump {v}");
}

#[test]
fn jump_height_and_airtime() {
    let mut m = duel([0.0, 0.0, 0.0], 0.0, [50.0, 0.0, 0.0], 0.0, -1, 20);
    let idle = Input::default();
    let jump = Input { jump: true, ..Default::default() };
    step(&mut m, &jump, &idle);
    assert!(!m.c[0].lp.l.body.on_ground);
    let mut apex: f64 = 0.0;
    let mut air = 1;
    while !m.c[0].lp.l.body.on_ground {
        apex = apex.max(m.c[0].lp.l.body.pos[1]);
        step(&mut m, &idle, &idle);
        air += 1;
        assert!(air < 40);
    }
    assert!((apex - 1.2492).abs() < 0.0005, "apex {apex}");
    assert_eq!(air, 12, "a jump lasts 12 ticks");
}

/// Attacker at the origin looking down +Z at a victim 2.5 blocks away.
fn first_velocity(sprint_first: bool) -> [i32; 3] {
    let mut m = duel([0.0, 0.0, 0.0], 0.0, [0.0, 0.0, 2.5], 180.0, -1, 20);
    let idle = Input::default();
    if sprint_first {
        // One tick of sprinting forward so the server learns the sprint
        // state, then stand still for a tick so the click is the only thing
        // that happens. Sprint needs W, so the attacker walks 1 tick.
        let go = Input { forward: 1, sprint_key: true, ..Default::default() };
        step(&mut m, &go, &idle);
        let hit = Input { forward: 1, sprint_key: true, attack: true, ..Default::default() };
        step(&mut m, &hit, &idle);
    } else {
        let hit = Input { attack: true, ..Default::default() };
        step(&mut m, &hit, &idle);
    }
    for _ in 0..6 {
        step(&mut m, &idle, &idle);
        if m.c[1].lp.ticks_since_kb < 1000 {
            let k = m.c[1].lp.last_kb;
            return [(k[0] * 8000.0).round() as i32, (k[1] * 8000.0).round() as i32, (k[2] * 8000.0).round() as i32];
        }
    }
    panic!("no knockback arrived");
}

#[test]
fn knockback_packet_values() {
    // Standing hit on a standing target: 0.4 away, 0.3608 up.
    assert_eq!(first_velocity(false), [0, 2886, 3200]);
    // Sprint hit: 0.4 + 0.5 away and 0.4608 up, capped by nothing.
    let v = first_velocity(true);
    assert_eq!(v[0], 0);
    assert_eq!(v[1], 3686);
    // The game divides by the distance rounded to a float, so the packet
    // can land one unit under 0.9 depending on where the attacker stands.
    assert!(v[2] == 7200 || v[2] == 7199, "{v:?}");
}

#[test]
fn invulnerability_lasts_ten_server_ticks() {
    let mut m = duel([0.0, 0.0, 0.0], 0.0, [0.0, 0.0, 2.0], 180.0, -1, 20);
    // Pin the victim so knockback does not carry it out of reach.
    let idle = Input::default();
    let hit = Input { attack: true, ..Default::default() };
    let mut hit_ticks = vec![];
    for _ in 0..45 {
        m.c[1].lp.l.body.motion = [0.0, -0.0784000015258789, 0.0];
        step(&mut m, &hit, &idle);
        for h in m.s.hits.drain(..) {
            assert_eq!(h.attacker, 0);
            hit_ticks.push(m.server_ticks);
        }
    }
    assert!(hit_ticks.len() >= 4, "{hit_ticks:?}");
    for w in hit_ticks.windows(2) {
        assert_eq!(w[1] - w[0], 10, "{hit_ticks:?}");
    }
}

#[test]
fn sprint_knockback_needs_a_sprint_reset() {
    // Hold W and sprint into a pinned victim, clicking every tick. Only the
    // first hit carries sprint knockback: the client re-sprints on its own
    // but never tells the server.
    let mut m = duel([0.0, 0.0, 0.0], 0.0, [0.0, 0.0, 2.6], 180.0, -1, 20);
    let idle = Input::default();
    let go = Input { forward: 1, sprint_key: true, ..Default::default() };
    let hit = Input { forward: 1, sprint_key: true, attack: true, ..Default::default() };
    step(&mut m, &go, &idle);
    let mut sprint_flags = vec![];
    for _ in 0..40 {
        m.c[1].lp.l.body.motion = [0.0, -0.0784000015258789, 0.0];
        // Keep the attacker from walking through the victim.
        m.c[0].lp.l.body.motion[2] = 0.0;
        step(&mut m, &hit, &idle);
        for h in m.s.hits.drain(..) {
            sprint_flags.push(h.sprint);
        }
    }
    assert!(sprint_flags.len() >= 3, "{sprint_flags:?}");
    assert!(sprint_flags[0]);
    assert!(sprint_flags[1..].iter().all(|s| !s), "{sprint_flags:?}");

    // Same again, but release W for one tick after each registered hit.
    let mut m = duel([0.0, 0.0, 0.0], 0.0, [0.0, 0.0, 2.6], 180.0, -1, 20);
    step(&mut m, &go, &idle);
    let mut sprint_flags = vec![];
    let mut tap = 0;
    for _ in 0..40 {
        m.c[1].lp.l.body.motion = [0.0, -0.0784000015258789, 0.0];
        m.c[0].lp.l.body.motion[2] = 0.0;
        let inp = if tap > 0 { Input { sprint_key: true, attack: true, ..Default::default() } } else { hit };
        if tap > 0 {
            tap -= 1;
        }
        step(&mut m, &inp, &idle);
        if m.c[0].rp.hurt_time == 9 {
            tap = 1;
        }
        for h in m.s.hits.drain(..) {
            sprint_flags.push(h.sprint);
        }
    }
    assert!(sprint_flags.len() >= 3, "{sprint_flags:?}");
    assert!(sprint_flags.iter().all(|s| *s), "{sprint_flags:?}");
}

#[test]
fn attack_reach_is_three_blocks_from_the_eye() {
    // The victim's box is widened by 0.1 for the pick, so its near face is
    // 0.4 from its centre.
    let idle = Input::default();
    let hit = Input { attack: true, ..Default::default() };
    for (dz, expect) in [(3.39, true), (3.41, false)] {
        let mut m = duel([0.0, 0.0, 0.0], 0.0, [0.0, 0.0, dz], 180.0, -1, 20);
        let id = m.advance();
        assert_eq!(id, 0);
        let r = m.client_tick(0, &hit);
        assert_eq!(r.attacked, expect, "dz {dz} reach {}", r.pick.dist);
        let _ = idle;
    }
}

#[test]
fn server_echoes_sprint_metadata_to_the_player_itself() {
    let mut m = duel([0.0, 0.0, 0.0], 0.0, [30.0, 0.0, 0.0], 0.0, 1, 20);
    let mut out = vec![];
    let world = m.world.clone();
    m.s.handle(0, C2S::Sprint(true), &world, &mut m.rng, &mut out);
    m.s.tick(&mut out);
    assert!(out.contains(&(0, S2C::Meta { own: true, sprinting: true, using: false })), "{out:?}");
    assert!(out.contains(&(1, S2C::Meta { own: false, sprinting: true, using: false })), "{out:?}");
    let base = 0.1f32 as f64;
    assert!(out.contains(&(0, S2C::Props { own: true, base, speed_amp: 1, sprint: true })), "{out:?}");
}

#[test]
fn deterministic() {
    let run = || {
        let mut m = duel([0.0, 0.0, 0.0], 10.0, [1.0, 0.0, 3.0], 170.0, 1, 60);
        let a = Input { forward: 1, strafe: 1, sprint_key: true, attack: true, dx: 3, dy: -1, jump: true, extra_clicks: 0, use_item: false };
        let b = Input { forward: 1, strafe: -1, sprint_key: true, attack: true, dx: -2, dy: 1, jump: false, extra_clicks: 0, use_item: false };
        for _ in 0..400 {
            step(&mut m, &a, &b);
        }
        (m.c[0].lp.l.body.pos, m.c[1].lp.l.body.pos, m.s.p[0].l.body.motion, m.s.p[1].l.body.motion)
    };
    let x = run();
    let y = run();
    assert_eq!(format!("{x:?}"), format!("{y:?}"));
}

#[test]
fn block_ray_reports_the_last_cell_border_in_open_air() {
    use pvpsim::world::RayHit;
    let w = World::arena(0.0, 200.0, true);
    // Level ray along +Z from z = 0.3: borders at 1, 2, 3, 4; the last one
    // is the MISS point.
    match w.ray_trace_blocks([0.5, 1.62, 0.3], [0.5, 1.62, 4.8], true) {
        RayHit::Miss(p) => assert_eq!(p, [0.5, 1.62, 4.0]),
        other => panic!("{other:?}"),
    }
    // A ray that stays inside one cell gives null.
    assert_eq!(w.ray_trace_blocks([0.2, 1.2, 0.2], [0.8, 1.8, 0.8], true), RayHit::None);
    // Looking down hits the floor at its top face.
    match w.ray_trace_blocks([0.5, 1.62, 0.5], [0.5, -2.0, 0.5], true) {
        RayHit::Block(p) => assert_eq!(p[1], 0.0),
        other => panic!("{other:?}"),
    }
    assert!(!w.blocked([0.5, 1.62, 0.5], [3.5, 1.62, 6.5]));
    assert!(w.blocked([0.5, 1.62, 0.5], [3.5, -0.5, 6.5]));
}

/// Player 0 hits player 1, who holds the right button from the start.
/// Returns the damage of the first hit and whether the server counted it
/// as blocked.
fn first_hit_on(blocking: bool) -> (f32, bool) {
    let mut m = duel([0.0, 0.0, 0.0], 0.0, [0.0, 0.0, 2.5], 180.0, -1, 20);
    let idle = Input::default();
    let hold = Input { use_item: blocking, ..Default::default() };
    for _ in 0..4 {
        step(&mut m, &idle, &hold);
    }
    let hit = Input { attack: true, ..Default::default() };
    step(&mut m, &hit, &hold);
    for _ in 0..4 {
        step(&mut m, &idle, &hold);
        if let Some(h) = m.s.hits.first() {
            return (h.damage, h.blocked);
        }
    }
    panic!("no hit");
}

#[test]
fn a_blocked_hit_does_half_the_damage_plus_a_half() {
    // EntityPlayer.damageEntity: (1 + damage) * 0.5 while blocking.
    assert_eq!(first_hit_on(false), (8.0, false));
    assert_eq!(first_hit_on(true), (4.5, true));
}

#[test]
fn blocking_slows_down_and_swallows_clicks() {
    let mut m = duel([0.0, 0.0, 0.0], 0.0, [0.0, 0.0, 2.5], 180.0, -1, 20);
    // An unbreakable sword, so that the hit does not make the server send
    // it again (see the test below).
    m.s.cfg.sword_wear = 0.0;
    let idle = Input::default();
    // The button goes down: the block starts on that tick, after the click
    // of the same tick has been made.
    let id = m.advance();
    assert_eq!(id, 0);
    let r = m.client_tick(0, &Input { forward: 1, attack: true, use_item: true, ..Default::default() });
    assert!(r.attacked && r.used && m.c[0].lp.using_item);
    assert_eq!(m.c[0].lp.mi_forward, 0.2);
    let id = m.advance();
    m.client_tick(id, &idle);
    // Held: clicks do nothing.
    m.advance();
    let r = m.client_tick(0, &Input { forward: 1, attack: true, use_item: true, ..Default::default() });
    assert!(!r.swung && r.swallowed == 1 && m.c[0].lp.using_item);
    let id = m.advance();
    m.client_tick(id, &idle);
    // Let go: the block ends, but the click of that tick is still lost.
    m.advance();
    let r = m.client_tick(0, &Input { forward: 1, attack: true, ..Default::default() });
    assert!(!r.swung && !m.c[0].lp.using_item);
    assert_eq!(m.c[0].lp.mi_forward, 1.0);
    let id = m.advance();
    m.client_tick(id, &idle);
    m.advance();
    let r = m.client_tick(0, &Input { forward: 1, attack: true, ..Default::default() });
    assert!(r.swung);
}

#[test]
fn a_block_hit_resets_the_sprint() {
    // As `sprint_knockback_needs_a_sprint_reset`, but W stays down and the
    // right button is tapped for one tick after each hit that registered.
    let mut m = duel([0.0, 0.0, 0.0], 0.0, [0.0, 0.0, 2.6], 180.0, -1, 20);
    let idle = Input::default();
    let go = Input { forward: 1, sprint_key: true, ..Default::default() };
    let hit = Input { forward: 1, sprint_key: true, attack: true, ..Default::default() };
    step(&mut m, &go, &idle);
    let mut sprint_flags = vec![];
    let mut tap = 0;
    for _ in 0..60 {
        m.c[1].lp.l.body.motion = [0.0, -0.0784000015258789, 0.0];
        m.c[0].lp.l.body.motion[2] = 0.0;
        let inp = if tap > 0 { Input { use_item: true, ..hit } } else { hit };
        if tap > 0 {
            tap -= 1;
        }
        step(&mut m, &inp, &idle);
        if m.c[0].rp.hurt_time == 9 {
            tap = 1;
        }
        for h in m.s.hits.drain(..) {
            sprint_flags.push(h.sprint);
        }
    }
    assert!(sprint_flags.len() >= 4, "{sprint_flags:?}");
    assert!(sprint_flags.iter().all(|s| *s), "{sprint_flags:?}");
}

#[test]
fn the_sword_coming_back_ends_the_block_on_the_client_only() {
    // Seen in the recordings of the real game: a hit costs the sword
    // durability, the server sends the sword again, and a client that is
    // blocking when it arrives drops the block without telling the server.
    // If the button is then let go before it clicks again, the server keeps
    // the player blocking.
    let mut m = duel([0.0, 0.0, 0.0], 0.0, [0.0, 0.0, 2.5], 180.0, -1, 20);
    let idle = Input::default();
    let hold = Input { use_item: true, ..Default::default() };
    step(&mut m, &Input { attack: true, use_item: true, ..Default::default() }, &idle);
    assert!(m.c[0].lp.using_item);
    let mut dropped_at = None;
    for t in 0..6 {
        step(&mut m, &hold, &idle);
        if !m.c[0].lp.using_item {
            dropped_at = Some(t);
            break;
        }
    }
    assert!(dropped_at.is_some(), "the client kept blocking");
    for _ in 0..10 {
        step(&mut m, &idle, &idle);
    }
    assert!(!m.c[0].lp.using_item);
    assert!(m.s.p[0].using_item, "the server was told to stop");
    assert!(m.c[1].rp.using, "the other player does not see the block");
    // An unbreakable sword is never sent again, so nothing of this happens.
    let mut m = duel([0.0, 0.0, 0.0], 0.0, [0.0, 0.0, 2.5], 180.0, -1, 20);
    m.s.cfg.sword_wear = 0.0;
    step(&mut m, &Input { attack: true, use_item: true, ..Default::default() }, &idle);
    for _ in 0..6 {
        step(&mut m, &hold, &idle);
        assert!(m.c[0].lp.using_item);
    }
    for _ in 0..10 {
        step(&mut m, &idle, &idle);
    }
    assert!(!m.s.p[0].using_item);
}
