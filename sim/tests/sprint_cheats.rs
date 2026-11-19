//! The sprint cheats of the bot's client (`Params::opp_sprint_reset`,
//! `Edge::sprint_packets`; `Params::opp_keep_sprint`, `Edge::keep_sprint`):
//! a sprint reset by packets before every attack, and a hit that neither
//! slows the client down nor stops its sprint. Off by default; the first
//! player's own game stays the plain one.
use pvpsim::client::{Client, ClientCfg, Edge, Input};
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};
use pvpsim::net::C2S;
use pvpsim::world::World;

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];

fn params() -> Params {
    let mut p = Params::default();
    p.start_yaw_noise = 0.0;
    p.ep_ticks_min = 100_000.0;
    p.ep_ticks_max = 100_000.0;
    p.kb_random = 0.0;
    p.opp_edge_prob = 0.0;
    p.opp_autoblock = 0.0;
    p.sword_wear = 0.0;
    p.insane_prob = 1.0;
    p.pearl_prob = 0.0;
    p.infinite_health = true;
    p
}

fn run(e: &mut Env, n: usize) {
    let (mut fo, mut fp) = (vec![0f32; OBS_DIM], vec![0f32; PRIV_DIM]);
    let mut k = 0;
    while k < n {
        if e.pending == 0 {
            k += 1;
        }
        e.step(&IDLE);
        if e.pending == 0 {
            e.take_outcome(&mut fo, &mut fp);
        }
    }
}

#[test]
fn no_sprint_cheat_unless_asked() {
    for seed in 0..20 {
        let e = Env::new(Mode::Bot, params(), seed);
        assert!(!e.m.c[1].edge.sprint_packets && !e.m.c[1].edge.keep_sprint);
    }
    let mut p = params();
    p.opp_sprint_reset = 1.0;
    p.opp_keep_sprint = 1.0;
    let e = Env::new(Mode::SelfPlay, p, 1);
    assert!(!e.m.c[1].edge.sprint_packets && !e.m.c[1].edge.keep_sprint);
    let e = Env::new(Mode::Bot, p, 1);
    assert!(e.m.c[1].edge.sprint_packets && e.m.c[1].edge.keep_sprint);
    assert_eq!(e.m.c[0].edge, Edge::default());
}

/// The insane bot's horizontal knockback and shortest reach can be set;
/// at their defaults the duel is drawn as before (`examples/golden`).
#[test]
fn insane_edge_ranges() {
    let mut p = params();
    p.insane_kb = [1.0, 1.0];
    p.insane_reach_min = 3.0;
    p.insane_reach_max = 3.0;
    let e = Env::new(Mode::Bot, p, 3);
    assert_eq!((e.m.c[1].edge.kb_taken, e.m.c[1].edge.reach), (1.0, 3.0));
    p.insane_kb = [0.0, 0.0];
    p.insane_reach_min = 3.8;
    p.insane_reach_max = 4.0;
    let e = Env::new(Mode::Bot, p, 3);
    assert_eq!(e.m.c[1].edge.kb_taken, 0.0);
    assert!((3.8..=4.0).contains(&e.m.c[1].edge.reach));
}

/// A client sprinting at the other player, who is just in reach: what one
/// tick with a click sends and what it does to the client's own run.
fn one_hit(edge: Edge) -> (Vec<C2S>, f64, f64, bool) {
    let world = World::arena(0.0, 40.0, true);
    let cfg = ClientCfg::with_sensitivity(0.5);
    let mut c = Client::new([0.0, 0.0, 0.0], 0.0, 0.0, -1, [0.0, 0.0, 30.0], 180.0, 0.0);
    c.edge = edge;
    let run = Input { forward: 1, sprint_key: true, ..Input::default() };
    let mut out = Vec::new();
    for _ in 0..20 {
        c.tick(&world, &run, &cfg, &mut out);
    }
    assert!(c.lp.l.sprinting);
    // The other player just ahead, in reach.
    let z = c.lp.l.body.pos[2];
    c.rp = pvpsim::client::RemotePlayer::spawn([0.0, 0.0, z + 2.5], 180.0, 0.0, true);
    let before = c.lp.l.body.motion[2];
    out.clear();
    c.tick(&world, &Input { attack: true, ..run }, &cfg, &mut out);
    let after = c.lp.l.body.motion[2];
    let state = c.lp.server_sprint_state;
    (out, before, after, state)
}

#[test]
fn a_sprint_reset_by_packets_comes_just_before_the_attack() {
    let (out, ..) = one_hit(Edge::default());
    assert!(out.contains(&C2S::Attack));
    assert!(!out.contains(&C2S::Sprint(false)), "{out:?}");
    let (out, ..) = one_hit(Edge { sprint_packets: true, ..Edge::default() });
    let at = out.iter().position(|p| *p == C2S::Attack).unwrap();
    assert!(at >= 2 && out[at - 2] == C2S::Sprint(false) && out[at - 1] == C2S::Sprint(true), "{out:?}");
}

#[test]
fn keep_sprint_keeps_the_speed_of_a_hit() {
    let (_, before, plain, _) = one_hit(Edge::default());
    let (_, _, kept, _) = one_hit(Edge { keep_sprint: true, ..Edge::default() });
    println!("speed {before:.3} before the hit; after it {plain:.3} plain, {kept:.3} with Keep Sprint");
    assert!(kept > before * 0.95, "{before} {kept}");
    assert!(plain < 0.9 * kept, "{plain} {kept}");
}

/// The share of the bot's hits on an idle first player that carry sprint
/// knockback, with its W-tap and block hits off, with and without the
/// sprint reset by packets, in an open field (no wall to pin the player
/// against, where the bot would stand on top of it and not sprint).
fn sprint_share(packets: bool) -> (f64, f64) {
    let mut p = params();
    p.insane_kb = [1.0, 1.0];
    p.start_dist_min = 4.0;
    p.start_dist_max = 4.0;
    p.arena_half = 200.0;
    p.walls = false;
    p.opp_sprint_reset = if packets { 1.0 } else { 0.0 };
    let (mut hits, mut sprint) = (0.0, 0.0);
    for seed in 0..4 {
        let mut e = Env::new(Mode::Bot, p, 40 + seed);
        e.bot.wtap = false;
        e.bot.blockhit = 0.0;
        e.bot.block_react = 0.0;
        run(&mut e, 400);
        let s = e.episode();
        hits += s.hits[1];
        sprint += s.sprint_hits[1];
    }
    (hits, sprint / hits.max(1.0))
}

#[test]
fn a_sprint_reset_by_packets_makes_every_hit_a_sprint_hit() {
    let (n0, plain) = sprint_share(false);
    let (n1, packets) = sprint_share(true);
    println!("{n0} hits, {plain:.2} with sprint knockback plain; {n1} hits, {packets:.2} with packets");
    assert!(n0 >= 40.0 && n1 >= 40.0);
    assert!(packets > 0.95, "{packets}");
    assert!(plain < 0.5, "{plain}");
}
