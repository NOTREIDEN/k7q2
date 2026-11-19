//! The tool of a kit (`Params::tool_prob`): a fishing rod, snowballs or
//! eggs in a slot of their own, used through the last control.
use pvpsim::env::{Env, Mode, Params, ACT_HEADS};
use pvpsim::net::{Item, Kind};

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];

fn params(mix: [f64; 3]) -> Params {
    let mut p = Params::default();
    p.tool_prob = 1.0;
    p.tool_mix = mix;
    p.zero_hits_prob = 1.0;
    p.throw_carry_prob = 0.0;
    p.pearl_prob = 0.0;
    p.start_dist_min = 3.0;
    p.start_dist_max = 3.0;
    p.bot_level = [0.0, 0.0];
    p.ep_ticks_min = 4000.0;
    p.ep_ticks_max = 4000.0;
    p
}

/// Turn player 0 to face the opponent, level, and give the server a tick
/// to hear of it.
fn face(e: &mut Env) {
    let (a, b) = (e.m.c[0].lp.l.body.pos, e.m.s.p[1].l.body.pos);
    let lp = &mut e.m.c[0].lp;
    lp.l.yaw = ((-(b[0] - a[0])).atan2(b[2] - a[2]).to_degrees()) as f32;
    lp.l.pitch = 0.0;
    e.step(&IDLE);
}

fn tool(a: i32) -> [i32; ACT_HEADS] {
    let mut x = IDLE;
    x[12] = a;
    x
}

#[test]
fn a_rod_cast_at_the_opponent_hooks_it_and_the_sword_comes_back() {
    let mut e = Env::new(Mode::Bot, params([1.0, 0.0, 0.0]), 5);
    let t = e.m.c[0].lp.tool;
    assert_eq!(t.item, Item::Rod);
    assert_eq!(e.m.s.p[0].tool, t);
    face(&mut e);
    e.step(&tool(1));
    assert_eq!(e.m.c[0].lp.held, t.slot);
    let mut hooked = false;
    for _ in 0..12 {
        e.step(&IDLE);
        hooked |= e.m.s.thrown.iter().any(|h| h.kind == Kind::Hook && h.hook.as_ref().is_some_and(|k| k.caught == Some(1)));
    }
    assert!(hooked, "the hook never caught the opponent");
    assert!(e.episode().tool_hits[0] >= 1.0, "{:?}", e.episode().tool_hits);
    // Pulled back: worn by the catch.
    e.step(&tool(1));
    for _ in 0..3 {
        e.step(&IDLE);
    }
    assert!(e.m.s.p[0].tool.wear >= 3, "{:?}", e.m.s.p[0].tool);
    // Casts are what count as uses.
    assert_eq!(e.episode().tool_uses[0], 1.0);
    e.step(&tool(2));
    e.step(&IDLE);
    assert_eq!(e.m.c[0].lp.held, 0);
}

#[test]
fn snowballs_are_thrown_one_click_at_a_time_and_the_sword_comes_back_when_they_run_out() {
    let mut p = params([0.0, 1.0, 0.0]);
    p.tool_balls = 3.0;
    let mut e = Env::new(Mode::Bot, p, 7);
    assert_eq!(e.m.c[0].lp.tool.n, 3);
    face(&mut e);
    for _ in 0..12 {
        e.step(&tool(1));
    }
    // The client throws faster than the server's word on its stack comes
    // back, as the game's does, and some of its throws are of snowballs it
    // no longer has; the server makes three.
    for _ in 0..12 {
        e.step(&IDLE);
    }
    assert_eq!(e.episode().tool_uses[0], 3.0);
    assert_eq!(e.m.c[0].lp.tool.item, Item::Empty);
    assert_eq!(e.m.s.p[0].tool.item, Item::Empty);
    assert_eq!(e.m.c[0].lp.held, 0, "the sword is taken back");
    assert!(e.episode().tool_hits[0] >= 1.0, "{:?}", e.episode().tool_hits);
}

#[test]
fn no_tool_where_throws_carry_the_movement_nor_unless_asked() {
    let mut p = params([1.0, 1.0, 1.0]);
    p.throw_carry_prob = 1.0;
    let e = Env::new(Mode::Bot, p, 9);
    assert_eq!(e.m.c[0].lp.tool.item, Item::Empty);
    let e = Env::new(Mode::Bot, Params::default(), 9);
    assert_eq!(e.m.c[0].lp.tool.item, Item::Empty);
    assert!(!e.m.s.cfg.zero_hits);
}
