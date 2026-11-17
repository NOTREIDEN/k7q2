//! Splash potions of Instant Health II, checked against numbers that follow
//! from the 1.8.9 source. The bit-exact comparison with the running game is
//! in the recordings (`tests/parity.rs`, scenes `pot*`).
use pvpsim::client::{ClientCfg, Input};
use pvpsim::net::{potion_mask, Item, Kind};
use pvpsim::server::{splash_heal, KbProfile, ServerCfg};
use pvpsim::sim::{Match, NetCfg, Spawn};
use pvpsim::world::World;

/// Two players `apart` blocks from each other, facing each other, with
/// seven potions each.
fn duel(cfg: ServerCfg, apart: f64) -> Match {
    let net = NetCfg { up_us: [10_000; 2], down_us: [10_000; 2], jitter_us: [0; 2] };
    let mut m = Match::new(
        World::arena(0.0, 200.0, true),
        [Spawn { pos: [0.0, 0.0, 0.0], yaw: 0.0, pitch: 0.0 }, Spawn { pos: [0.0, 0.0, apart], yaw: 180.0, pitch: 0.0 }],
        -1,
        KbProfile::VANILLA,
        cfg,
        ClientCfg::with_sensitivity(0.5),
        net,
        [0, 17_000, 31_000],
        7,
    );
    for i in 0..2 {
        m.s.p[i].update_counter = 2;
        m.s.p[i].potions = potion_mask(7);
        m.c[i].lp.potions = potion_mask(7);
    }
    m
}

fn real() -> ServerCfg {
    ServerCfg { infinite_health: false, sword_wear: 0.0, hunger: false, natural_regen: false, ..ServerCfg::default() }
}

fn step(m: &mut Match, i0: &Input, i1: &Input) {
    for _ in 0..2 {
        let id = m.advance();
        m.client_tick(id, if id == 0 { i0 } else { i1 });
    }
}

fn idle(m: &mut Match, n: usize) {
    let i = Input::default();
    for _ in 0..n {
        step(m, &i, &i);
    }
}

/// Player 0 turns its view by `dy` mouse counts (600 is a quarter turn at
/// this sensitivity), takes the potion in hotbar slot `slot` in hand,
/// throws it, and takes the sword again.
fn throw(m: &mut Match, dy: i32, slot: u8) {
    let i = Input::default();
    step(m, &Input { dy, ..Default::default() }, &i);
    step(m, &Input { slot_key: slot + 1, use_item: true, ..Default::default() }, &i);
    step(m, &Input { slot_key: 1, ..Default::default() }, &i);
}

#[test]
fn the_share_falls_with_the_distance() {
    // Whole points: all eight within a quarter of a block, one less for
    // every half block after that, nothing from four blocks on.
    let at = [0.0, 0.0, 0.0];
    let got: Vec<f32> = [0.0, 0.24, 0.26, 0.74, 0.76, 1.9, 3.2, 3.74, 3.76, 3.99, 4.0, 5.0].iter().map(|d| splash_heal(at, [*d, 0.0, 0.0], false)).collect();
    assert_eq!(got, [8.0, 8.0, 7.0, 7.0, 6.0, 4.0, 2.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    // The player it broke on gets all of it wherever its feet are.
    assert_eq!(splash_heal(at, [0.0, -1.7, 0.0], true), 8.0);
}

#[test]
fn a_potion_at_ones_own_feet_gives_seven() {
    // Thrown straight down from a standing player it breaks on the third
    // tick. Its reach is measured from where it was a tick before that,
    // almost half a block up and a little to the side: seven, not eight.
    let mut m = duel(real(), 12.0);
    m.s.p[0].set_health(5.0);
    idle(&mut m, 4);
    throw(&mut m, 600, 2);
    assert_eq!(m.c[0].lp.potions, potion_mask(7) & !(1 << 2), "the client takes it off at once");
    idle(&mut m, 10);
    assert_eq!(m.s.potion_throws, [1, 0]);
    assert_eq!(m.s.splashes.len(), 1);
    let s = m.s.splashes[0];
    assert_eq!((s.ticks, s.on_player, s.offered, s.healed), (3, None, [7.0, 0.0], [7.0, 0.0]));
    assert!((s.pos[1] - 0.475).abs() < 0.02, "{:?}", s.pos);
    assert_eq!(m.s.p[0].health, 12.0);
    assert_eq!(m.c[0].lp.health, 12.0, "the thrower is told");
    assert_eq!(m.c[1].rp.health, 12.0, "and so is the other player");
    assert_eq!((m.s.p[0].potions, m.c[0].lp.potions), (potion_mask(7) & !(1 << 2), potion_mask(7) & !(1 << 2)));
    assert!(m.s.thrown.is_empty() && m.c[0].thrown.is_empty() && m.c[1].thrown.is_empty());
}

#[test]
fn a_potion_thrown_up_comes_down_on_its_thrower_and_gives_everything() {
    let mut m = duel(real(), 12.0);
    m.s.p[0].set_health(5.0);
    idle(&mut m, 4);
    throw(&mut m, -600, 3);
    let mut seen = false;
    for _ in 0..60 {
        idle(&mut m, 1);
        seen |= m.c[1].thrown.iter().any(|e| e.kind == Kind::Potion && !e.mine);
    }
    assert!(seen, "the other player saw it fly");
    assert_eq!(m.s.splashes.len(), 1);
    let s = m.s.splashes[0];
    assert_eq!((s.on_player, s.offered, s.healed), (Some(0), [8.0, 0.0], [8.0, 0.0]));
    assert!(s.ticks > 10, "{}", s.ticks);
    assert_eq!(m.s.p[0].health, 13.0);
}

#[test]
fn a_potion_heals_the_opponent_too_and_never_past_twenty() {
    let mut m = duel(real(), 1.5);
    m.s.p[0].set_health(16.0);
    m.s.p[1].set_health(3.0);
    idle(&mut m, 4);
    throw(&mut m, 600, 2);
    idle(&mut m, 10);
    let s = m.s.splashes[0];
    // 1.5 blocks away and half a block below the potion: five for the
    // other player. The thrower was only missing four.
    assert_eq!((s.offered, s.healed), ([7.0, 5.0], [4.0, 5.0]));
    assert_eq!((m.s.p[0].health, m.s.p[1].health), (20.0, 8.0));
    assert_eq!((m.c[0].rp.health, m.c[1].lp.health), (8.0, 8.0));
}

#[test]
fn a_slot_holds_one_potion_and_the_other_player_sees_what_is_in_hand() {
    let mut m = duel(real(), 6.0);
    idle(&mut m, 4);
    let i = Input::default();
    step(&mut m, &Input { slot_key: 5, ..Default::default() }, &i);
    idle(&mut m, 4);
    assert_eq!(m.c[1].rp.item, Item::Potion);
    step(&mut m, &Input { use_item: true, ..Default::default() }, &i);
    idle(&mut m, 6);
    assert_eq!(m.c[1].rp.item, Item::Empty);
    // Clicking again with the empty slot in hand does nothing.
    step(&mut m, &Input { use_item: true, ..Default::default() }, &i);
    idle(&mut m, 30);
    assert_eq!(m.s.potion_throws, [1, 0]);
    assert_eq!((m.s.p[0].potions, m.c[0].lp.potions), (potion_mask(7) & !(1 << 4), potion_mask(7) & !(1 << 4)));
    // All seven, one after the other.
    for slot in [2u8, 3, 5, 6, 7, 8] {
        throw(&mut m, 0, slot);
        idle(&mut m, 5);
    }
    idle(&mut m, 40);
    assert_eq!(m.s.potion_throws, [7, 0]);
    assert_eq!((m.s.p[0].potions, m.c[0].lp.potions), (0, 0));
    assert_eq!(m.s.splashes.len(), 7);
}

#[test]
fn with_infinite_health_a_potion_changes_nothing() {
    let mut m = duel(ServerCfg { sword_wear: 0.0, ..ServerCfg::default() }, 2.0);
    idle(&mut m, 4);
    throw(&mut m, 600, 2);
    idle(&mut m, 10);
    assert_eq!(m.s.splashes.len(), 1);
    assert_eq!(m.s.splashes[0].healed, [0.0, 0.0]);
    assert_eq!((m.s.p[0].health, m.s.p[1].health), (20.0, 20.0));
}
