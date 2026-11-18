//! Terrain: slabs are walked onto, full blocks jumped onto, walls stop a
//! player, the void ends the duel, what the agent is shown of the ground,
//! and duels drawn on terrain.
use pvpsim::agent::{ground, ground_grid, GRID_FWD, GRID_RIGHT, NO_GROUND};
use pvpsim::client::{ClientCfg, Input};
use pvpsim::env::{new_duel, Mode, Params};
use pvpsim::mth::Rng;
use pvpsim::server::{KbProfile, ServerCfg};
use pvpsim::sim::{Match, NetCfg, Spawn};
use pvpsim::world::{World, FULL, SLAB};

/// An arena with terrain made by `make` on a blank floor at height 0.
fn world(make: impl Fn(&mut pvpsim::world::Terrain)) -> World {
    let mut w = World::arena(0.0, 24.0, true);
    let mut t = w.blank_terrain();
    make(&mut t);
    w.set_terrain(t);
    w
}

fn duel(w: World, a: [f64; 3], yaw_a: f32, b: [f64; 3]) -> Match {
    let net = NetCfg { up_us: [10_000; 2], down_us: [10_000; 2], jitter_us: [0; 2] };
    let mut m = Match::new(
        w,
        [Spawn { pos: a, yaw: yaw_a, pitch: 0.0 }, Spawn { pos: b, yaw: 0.0, pitch: 0.0 }],
        -1,
        KbProfile::VANILLA,
        ServerCfg::default(),
        ClientCfg::with_sensitivity(0.5),
        net,
        [0, 17_000, 31_000],
        7,
    );
    m.s.p[0].update_counter = 2;
    m.s.p[1].update_counter = 2;
    m
}

fn step(m: &mut Match, i0: &Input) {
    let idle = Input::default();
    for _ in 0..2 {
        let id = m.advance();
        m.client_tick(id, if id == 0 { i0 } else { &idle });
    }
}

fn walk(jump: bool) -> Input {
    let mut i = Input::default();
    i.forward = 1;
    i.sprint_key = true;
    i.jump = jump;
    i
}

/// Walking south (yaw 0 is +z) from z 0.5 towards something at z 3.
fn run_into(make: impl Fn(&mut pvpsim::world::Terrain), jump: bool, ticks: usize) -> Match {
    let w = world(make);
    let mut m = duel(w, [0.5, 0.0, 0.5], 0.0, [10.5, 0.0, 10.5]);
    for _ in 0..ticks {
        step(&mut m, &walk(jump));
    }
    m
}

#[test]
fn a_slab_is_walked_onto_without_a_jump() {
    let m = run_into(|t| t.set(0, 0, 3, SLAB), false, 30);
    let p = m.c[0].lp.l.body.pos;
    assert!(p[2] > 4.0, "walked on past the slab: {p:?}");
    // Stepped up onto it on the way and off its far side again.
    let m = run_into(
        |t| {
            for z in 3..10 {
                t.set(0, 0, z, SLAB);
            }
        },
        false,
        20,
    );
    let p = m.c[0].lp.l.body.pos;
    assert!(p[2] > 3.5 && p[2] < 10.0, "on the slabs: {p:?}");
    assert_eq!(p[1], 0.5, "standing on a slab");
    assert_eq!(m.s.p[0].l.body.pos[1], 0.5, "and the server agrees");
}

#[test]
fn a_full_block_stops_a_walk_and_is_jumped_onto() {
    let block = |t: &mut pvpsim::world::Terrain| {
        for z in 3..10 {
            t.set(0, 0, z, FULL);
        }
    };
    let m = run_into(block, false, 30);
    let p = m.c[0].lp.l.body.pos;
    assert!((p[2] - 2.7).abs() < 1e-6 && p[1] == 0.0, "stopped against the block: {p:?}");
    let m = run_into(block, true, 30);
    let p = m.c[0].lp.l.body.pos;
    assert!(p[2] > 3.3 && p[1] >= 1.0, "jumped up onto it: {p:?}");
}

#[test]
fn the_void_ends_the_duel() {
    let m = run_into(
        |t| {
            for x in -2..3 {
                for z in 3..6 {
                    t.dig_void(x, z);
                }
            }
        },
        false,
        60,
    );
    assert!(m.c[0].lp.l.body.pos[1] < -10.0, "fell: {:?}", m.c[0].lp.l.body.pos);
    assert!(!m.s.p[0].alive(), "fallen out of the world");
    assert!(m.s.p[1].alive());
}

#[test]
fn a_pit_is_fallen_into_and_stepped_out_of_when_shallow() {
    let pit = |depth: i32| {
        move |t: &mut pvpsim::world::Terrain| {
            for x in -2..3 {
                for z in 3..3 + 2 * depth {
                    for y in -depth..0 {
                        t.set(x, y, z, pvpsim::world::AIR);
                    }
                }
            }
        }
    };
    let m = run_into(pit(1), true, 40);
    let p = m.c[0].lp.l.body.pos;
    assert!(p[2] > 5.0 && p[1] >= 0.0, "through a pit one deep: {p:?}");
    let m = run_into(pit(3), true, 40);
    let p = m.c[0].lp.l.body.pos;
    assert!(p[2] < 9.0 && p[1] < -2.0, "stuck in a pit three deep: {p:?}");
}

#[test]
fn the_ground_shown_around_a_player() {
    let w = world(|t| {
        t.set(0, 0, 2, FULL);
        t.set(1, 0, 2, SLAB);
        t.set(-1, 0, 2, FULL);
        t.set(-1, 1, 2, FULL);
        t.dig_void(0, -2);
        for y in -2..0 {
            t.set(0, y, 3, pvpsim::world::AIR);
        }
    });
    assert_eq!(ground(&w, 0, 0, 0.0), 0.0);
    assert_eq!(ground(&w, 0, 2, 0.0), 1.0);
    assert_eq!(ground(&w, 1, 2, 0.0), 0.5);
    assert_eq!(ground(&w, -1, 2, 0.0), 2.0);
    assert_eq!(ground(&w, 0, -2, 0.0), NO_GROUND);
    assert_eq!(ground(&w, 0, 3, 0.0), -2.0);
    // On the slab, the floor is half a block down.
    assert_eq!(ground(&w, 0, 0, 0.5), -0.5);
    // The arena wall.
    assert_eq!(ground(&w, 24, 0, 0.0), 2.0);
    // Looking south (yaw 0): forward is +z, right is -x.
    let mut o = vec![0.0f32; GRID_FWD.len() * GRID_RIGHT.len()];
    ground_grid(&w, [0.5, 0.0, 0.5], 0.0, &mut o);
    let at = |o: &[f32], f: f64, r: f64| {
        let i = GRID_FWD.iter().position(|&v| v == f).unwrap();
        let j = GRID_RIGHT.iter().position(|&v| v == r).unwrap();
        o[i * GRID_RIGHT.len() + j] * 3.0
    };
    assert_eq!(at(&o, 0.0, 0.0), 0.0);
    assert_eq!(at(&o, 2.0, 0.0), 1.0);
    assert_eq!(at(&o, 2.0, 1.0), 2.0, "x -1 is to the right");
    assert_eq!(at(&o, 2.0, -1.0), 0.5);
    assert_eq!(at(&o, -2.0, 0.0), NO_GROUND as f32);
    // Turned around, the same blocks are behind.
    ground_grid(&w, [0.5, 0.0, 0.5], 180.0, &mut o);
    assert_eq!(at(&o, -2.0, 0.0), 1.0);
    assert_eq!(at(&o, 2.0, 0.0), NO_GROUND as f32);
}

#[test]
fn duels_on_terrain_start_on_open_ground() {
    let mut p = Params::default();
    p.terrain_prob = 1.0;
    p.terrain_amount = 1.0;
    let mut rng = Rng::new(3);
    let mut blocks = 0usize;
    for _ in 0..200 {
        let (m, _, _, _) = new_duel(&p, &mut rng, Mode::SelfPlay);
        let t = m.world.terrain().expect("terrain");
        blocks += t.cells.iter().filter(|&&b| b != 0).count();
        for s in &m.s.p {
            let pos = s.l.body.pos;
            let (x, z) = (pos[0].floor() as i32, pos[2].floor() as i32);
            for dx in -1..=1 {
                for dz in -1..=1 {
                    assert_eq!(ground(&m.world, x + dx, z + dz, pos[1]), 0.0, "open around {pos:?}");
                }
            }
        }
    }
    assert!(blocks > 0);
}
