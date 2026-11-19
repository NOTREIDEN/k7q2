//! Cobwebs as 1.8.9 has them: nothing to collide with, a whole block to a
//! ray, and a move that starts in one goes a quarter as far sideways and a
//! twentieth as far up or down, with the motion gone (`Entity.moveEntity`,
//! `BlockWeb.onEntityCollidedWithBlock`). The recordings of the real game
//! in `parity.rs` (scenes `web*`) check them to the bit.
use pvpsim::aabb::Aabb;
use pvpsim::body::Body;
use pvpsim::client::{ClientCfg, Input};
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM};
use pvpsim::agent::WEB_AT;
use pvpsim::server::{KbProfile, ServerCfg};
use pvpsim::sim::{Match, NetCfg, Spawn};
use pvpsim::world::{RayHit, World};

#[test]
fn a_move_that_starts_in_a_web_is_slowed_and_loses_its_motion() {
    let w = World::with_blocks_and_webs(0.0, &[], &[[1, 0, 0]]);
    let mut b = Body::new([0.5, 0.0, 0.5]);
    b.motion = [0.2, 0.0, 0.0];
    // Out of it, the whole way, and into it: the box reaches past x = 1.
    b.move_entity(&w, 0.3, 0.0, 0.0, true);
    assert!((b.pos[0] - 0.8).abs() < 1e-12 && b.in_web, "{:?} {}", b.pos, b.in_web);
    // In it: a quarter, and no motion left.
    b.motion = [0.2, 0.0, 0.0];
    b.move_entity(&w, 0.2, 0.0, 0.0, true);
    assert!((b.pos[0] - 0.85).abs() < 1e-12, "{:?}", b.pos);
    assert_eq!(b.motion, [0.0; 3]);
    assert!(b.in_web);
    // Up: a twentieth.
    b.move_entity(&w, 0.0, 0.42, 0.0, true);
    assert!((b.pos[1] - 0.42 * 0.05000000074505806).abs() < 1e-12, "{:?}", b.pos);
    // Back out of it: the move that leaves it is still slowed, the next one is not.
    b.set_position([0.75, 0.0, 0.5]);
    b.move_entity(&w, -0.2, 0.0, 0.0, true);
    assert!((b.pos[0] - 0.7).abs() < 1e-12 && !b.in_web, "{:?} {}", b.pos, b.in_web);
    b.move_entity(&w, -0.2, 0.0, 0.0, true);
    assert!((b.pos[0] - 0.5).abs() < 1e-12, "{:?}", b.pos);
}

#[test]
fn a_web_is_nothing_to_collide_with_and_a_whole_block_to_a_ray() {
    let w = World::with_blocks_and_webs(0.0, &[], &[[0, 0, 0]]);
    let mut boxes = Vec::new();
    w.colliding_boxes(&Aabb::player([0.5, 0.0, 0.5]), &mut boxes);
    assert!(boxes.iter().all(|b| b.max[1] <= 0.0), "{boxes:?}");
    assert!(!w.collides(&Aabb::player([0.5, 0.0, 0.5])));
    // Down onto it from above: the ray stops on its top.
    match w.ray_trace_blocks([0.5, 3.0, 0.5], [0.5, -0.5, 0.5], false) {
        RayHit::Block(h) => assert!((h[1] - 1.0).abs() < 1e-12, "{h:?}"),
        r => panic!("{r:?}"),
    }
    // Through it sideways: it is in the way.
    assert!(w.blocked([-2.0, 0.5, 0.5], [3.0, 0.5, 0.5]));
    assert!(!w.blocked([-2.0, 1.5, 0.5], [3.0, 1.5, 0.5]));
    // The box of a player standing next to it, touching it, is not in it.
    assert!(!w.in_web(&Aabb::player([1.3, 0.0, 0.5])));
    assert!(w.in_web(&Aabb::player([1.29, 0.0, 0.5])));
}

fn duel(world: World) -> Match {
    let net = NetCfg { up_us: [10_000; 2], down_us: [10_000; 2], jitter_us: [0; 2] };
    let mut m = Match::new(
        world,
        [Spawn { pos: [0.5, 0.0, 0.5], yaw: 0.0, pitch: 0.0 }, Spawn { pos: [0.5, 0.0, 3.0], yaw: 180.0, pitch: 0.0 }],
        1,
        KbProfile::VANILLA,
        ServerCfg { sword_wear: 0.0, ..ServerCfg::default() },
        ClientCfg::with_sensitivity(0.5),
        net,
        [0, 17_000, 31_000],
        7,
    );
    m.s.p[0].update_counter = 2;
    m.s.p[1].update_counter = 2;
    m
}

fn step(m: &mut Match, i0: &Input, i1: &Input) {
    for _ in 0..2 {
        let id = m.advance();
        m.client_tick(id, if id == 0 { i0 } else { i1 });
    }
}

/// How far player 1 is knocked by one hit of player 0, standing where it is.
fn knocked(world: World) -> f64 {
    let mut m = duel(world);
    let idle = Input::default();
    for _ in 0..6 {
        step(&mut m, &idle, &idle);
    }
    let at = m.c[1].lp.l.body.pos;
    step(&mut m, &Input { attack: true, ..Default::default() }, &idle);
    for _ in 0..12 {
        step(&mut m, &idle, &idle);
    }
    assert_eq!(m.s.hits.len(), 1);
    let p = m.c[1].lp.l.body.pos;
    (p[0] - at[0]).hypot(p[2] - at[2])
}

#[test]
fn a_player_in_a_web_is_hardly_knocked_back_and_hardly_runs() {
    let open = knocked(World::arena(0.0, 50.0, true));
    let caught = knocked(World::with_blocks_and_webs(0.0, &[], &[[0, 0, 3], [0, 1, 3]]));
    assert!(open > 1.0 && caught < 0.25 * open, "{open} {caught}");
    // Sprinting forward for a second, in a web and out of one.
    let run = |world: World| {
        let mut m = duel(world);
        let go = Input { forward: 1, sprint_key: true, ..Default::default() };
        let idle = Input::default();
        let at = m.c[0].lp.l.body.pos;
        for _ in 0..20 {
            step(&mut m, &go, &idle);
        }
        let p = m.c[0].lp.l.body.pos;
        (p[0] - at[0]).hypot(p[2] - at[2])
    };
    let free = run(World::arena(0.0, 50.0, true));
    let stuck = run(World::with_blocks_and_webs(0.0, &[], &[[0, 0, 0], [0, 0, 1]]));
    assert!(free > 3.0 && stuck < 1.5, "{free} {stuck}");
    // And the server takes the slow moves without a word.
}

#[test]
fn cobweb_duels_show_the_webs_and_some_start_caught_in_one() {
    let mut p = Params::default();
    p.web_prob = 1.0;
    p.web_trap_prob = 1.0;
    let mut e = Env::new(Mode::Bot, p, 9);
    let a: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];
    e.step(&a);
    let mut o = [0.0f32; OBS_DIM];
    e.write_obs(0, &mut o);
    assert_eq!(o[WEB_AT], 1.0);
    // The block it stands in is in the middle of the grid's third row.
    assert!(o[WEB_AT + 2..OBS_DIM].iter().any(|&v| v > 0.0));
    assert!(e.m.world.has_webs());
    // Without the parameter, none.
    let e = Env::new(Mode::Bot, Params::default(), 9);
    assert!(!e.m.world.has_webs());
}
