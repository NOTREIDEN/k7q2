//! The bot that pots as a perfect bot does (`Params::bot_pot_exact`): all
//! eight points of every potion onto itself, none of them wasted on health
//! it is not missing, and none of them given to the opponent.
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};
use pvpsim::net::Kind;

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];

fn params(exact: f64, dist: f64) -> Params {
    let mut p = Params::default();
    p.infinite_health = false;
    p.health_prob = 1.0;
    p.prot = [2.0, 2.0];
    p.sharp = [2.0, 2.0];
    p.potions = [7.0, 7.0];
    p.start_hurt = 0.0;
    p.pearl_prob = 0.0;
    p.insane_prob = 1.0;
    p.start_dist_min = dist;
    p.start_dist_max = dist;
    p.ep_ticks_min = 100_000.0;
    p.ep_ticks_max = 100_000.0;
    p.bot_pot_exact = exact;
    p
}

struct Run {
    exact: bool,
    splashes: f64,
    given: f64,
    over: f64,
    gift: f64,
}

/// Player 0 stands still and never hits; the bot is put down to `hp`
/// health whenever it has none of its potions in the air and is above
/// that, for `steps` ticks.
fn run(p: Params, seed: u64, hp: f32, steps: usize) -> Run {
    let mut e = Env::new(Mode::Bot, p, seed);
    let (mut fobs, mut fpriv) = (vec![0f32; OBS_DIM], vec![0f32; PRIV_DIM]);
    let exact = e.bot.pot_exact;
    for k in 0..steps {
        let flying = e.m.s.thrown.iter().any(|t| t.kind == Kind::Potion && t.thrower == 1);
        if k % 40 == 0 && !flying && e.m.s.p[1].health > hp + 4.0 && e.m.s.p[1].potions != 0 {
            e.m.s.p[1].health = hp;
            e.m.s.p[1].last_health = hp;
            e.m.c[1].lp.health = hp;
            e.m.c[0].rp.health = hp;
        }
        e.step(&IDLE);
        let _ = e.take_outcome(&mut fobs, &mut fpriv);
    }
    let st = *e.episode();
    Run { exact, splashes: st.splashes[1], given: st.pot_given[1], over: st.pot_over[1], gift: st.heal_gift[1] }
}

#[test]
fn off_by_default() {
    assert_eq!(Params::default().bot_pot_exact, 0.0);
    for seed in 1..4 {
        let e = Env::new(Mode::Bot, params(0.0, 6.0), seed);
        assert!(!e.bot.pot_exact);
    }
}

#[test]
fn on_when_asked() {
    for seed in 1..4 {
        let e = Env::new(Mode::Bot, params(1.0, 6.0), seed);
        assert!(e.bot.pot_exact);
    }
}

/// With the opponent away it gets nearly all eight points of its potions
/// (more than the old perfect potter), never more than it was missing,
/// and gives the opponent next to nothing.
#[test]
fn all_eight_points() {
    let (mut old, mut new, mut gift) = ((0.0, 0.0), (0.0, 0.0), 0.0);
    for seed in 1..6 {
        // The old perfect potter heals itself at 4 to 6 health only.
        let r = run(params(0.0, 10.0), seed, 4.5, 1500);
        old.0 += r.splashes;
        old.1 += r.given;
        let r = run(params(1.0, 10.0), seed, 8.0, 1500);
        assert!(r.exact);
        assert_eq!(r.over, 0.0, "seed {seed}: overheal");
        new.0 += r.splashes;
        new.1 += r.given;
        gift += r.gift;
    }
    assert!(new.0 >= 20.0, "only {} potions", new.0);
    let (a, b) = (old.1 / old.0.max(1.0), new.1 / new.0.max(1.0));
    eprintln!("health per potion: perfect_pot {a:.2} ({} potions), pot_exact {b:.2} ({} potions, {gift:.1} given away)", old.0, new.0);
    assert!(gift <= 0.1 * new.0, "{gift:.1} health given to the opponent");
    assert!(b >= 7.3, "{b:.2} health per potion");
    assert!(b > a, "{b:.2} against {a:.2}");
}

/// Never a potion while it is missing less than eight points.
#[test]
fn no_overheal() {
    for seed in 1..4 {
        let r = run(params(1.0, 10.0), seed, 13.0, 1200);
        assert_eq!(r.splashes, 0.0, "seed {seed}: potted at 13 health");
    }
}
