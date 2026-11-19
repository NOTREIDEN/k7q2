//! The pearls of the best bots (`Params::pearl_punish`,
//! `bot::PearlPunish`): the throw that lands on the spot whatever the
//! server adds to it (`bot::pearl_aim`), and a drink begun far from the
//! hardest bot that it answers with a pearl into reach.
use pvpsim::aabb::EYE_HEIGHT;
use pvpsim::bot;
use pvpsim::env::{Env, Mode, Params, ACT_HEADS, OBS_DIM, PRIV_DIM};
use pvpsim::net::Kind;
use pvpsim::server::throw_heading;

/// Stand still, mouse at rest, no button down.
const IDLE: [i32; ACT_HEADS] = [0, 0, 0, 0, 0, 13, 10, 0, 0, 0, 0, 0, 0];
const DRINK: i32 = 2;

/// Where the server puts the thrower of a pearl thrown along `yaw` and
/// `pitch` from `feet` with `carry` added to it, on a floor at `floor_y`.
fn land(feet: [f64; 3], yaw: f64, pitch: f64, carry: [f64; 2], floor_y: f64) -> [f64; 2] {
    let (mut pos, dir) = throw_heading(Kind::Pearl, feet, yaw as f32, pitch as f32);
    let mut m = [dir[0] * 1.5 + carry[0], dir[1] * 1.5, dir[2] * 1.5 + carry[1]];
    for _ in 0..200 {
        if pos[1] + m[1] <= floor_y {
            break;
        }
        for k in 0..3 {
            pos[k] += m[k];
            m[k] *= 0.99;
        }
        m[1] -= 0.03;
    }
    [pos[0], pos[2]]
}

#[test]
fn the_aim_lands_on_the_spot() {
    let floor = 4.0;
    let feet = [1.0, floor, -2.0];
    let eye = [feet[0], floor + EYE_HEIGHT, feet[2]];
    let (mut aimed, mut plain, mut n) = (0.0, 0.0, 0.0);
    for (dx, dz) in [(3.0, 2.0), (-6.0, 4.0), (0.5, -11.0), (14.0, 9.0), (-18.0, -3.0), (2.0, 24.0)] {
        for carry in [[0.0, 0.0], [0.25, -0.1], [-0.3, 0.3]] {
            let q = [feet[0] + dx, floor, feet[2] + dz];
            let (yaw, pitch, t) = bot::pearl_aim(eye, q, carry);
            assert!(t > 0 && t < 100);
            let at = land(feet, yaw, pitch, carry, floor);
            let err = (at[0] - q[0]).hypot(at[1] - q[2]);
            // Where the thrower lands is where the pearl was at the start
            // of the tick it hit in: some spots lie between two ticks.
            assert!(err < 1.5, "({dx}, {dz}) carried {carry:?}: {err} blocks off");
            aimed += err;
            // The plain throw at the spot, as the other bots aim.
            let (yaw0, _) = bot::angles_to(eye, q);
            let pitch0 = bot::pearl_pitch((q[0] - eye[0]).hypot(q[2] - eye[2]), EYE_HEIGHT);
            let at0 = land(feet, yaw0, pitch0, carry, floor);
            plain += (at0[0] - q[0]).hypot(at0[1] - q[2]);
            n += 1.0;
        }
    }
    let (aimed, plain) = (aimed / n, plain / n);
    assert!(aimed < 0.45, "the aimed pearl lands {aimed} blocks from the spot on average");
    assert!(aimed < 0.5 * plain, "the plain throw lands {plain} blocks off, the aimed one {aimed}");
}

fn params(punish: bool) -> Params {
    let mut p = Params::default();
    p.infinite_health = false;
    p.potions = [7.0, 7.0];
    p.start_hurt = 0.0;
    p.pearl_prob = 1.0;
    p.pearl_cd_s = [5.0, 5.0];
    p.start_dist_min = 13.0;
    p.start_dist_max = 13.0;
    p.start_yaw_noise = 0.0;
    p.ep_ticks_min = 100_000.0;
    p.ep_ticks_max = 100_000.0;
    p.kb_random = 0.0;
    p.speed_end_prob = 1.0;
    p.speed_left_s = [0.0, 0.0];
    p.drinks = [2.0, 2.0];
    p.insane_prob = 1.0;
    if punish {
        p.pearl_punish = 1.0;
        // No pearl back to a far opponent: only the drink is answered.
        p.pearl_punish_far = 100.0;
    }
    p
}

/// Player 0 starts to drink on its first tick and keeps still. The first
/// landing of the bot's pearl within 40 ticks: how far from player 0 it
/// put the bot, and whether player 0 was still drinking.
fn drink_answered(punish: bool, seed: u64) -> Option<(f64, bool)> {
    let mut e = Env::new(Mode::Bot, params(punish), seed);
    assert_eq!(e.bot.pearl_punish.on, punish);
    // The bot has its Speed and nothing to drink: nothing else to do.
    e.m.s.p[1].drinks = 0;
    e.m.c[1].lp.drinks = 0;
    let (mut fo, mut fp) = (vec![0f32; OBS_DIM], vec![0f32; PRIV_DIM]);
    let mut k = 0;
    let mut was = e.m.s.p[1].l.body.pos;
    while k < 40 {
        if e.pending == 0 {
            let mut a = IDLE;
            if k == 0 {
                a[9] = DRINK;
            }
            e.step(&a);
            k += 1;
        } else {
            e.step(&IDLE);
        }
        if e.pending == 0 {
            e.take_outcome(&mut fo, &mut fp);
        }
        let (b, a) = (e.m.s.p[1].l.body.pos, e.m.s.p[0].l.body.pos);
        if (b[0] - was[0]).hypot(b[2] - was[2]) > 3.0 {
            return Some(((b[0] - a[0]).hypot(b[2] - a[2]), e.m.c[0].lp.consuming()));
        }
        was = b;
    }
    None
}

#[test]
fn a_far_drink_is_answered_with_a_pearl_into_reach() {
    let (mut answered, mut near, mut plain_near) = (0, 0.0, 0.0);
    for seed in 0..12 {
        if let Some((d, drinking)) = drink_answered(true, 300 + seed) {
            assert!(d < 3.0, "seed {seed}: landed {d} blocks away");
            assert!(drinking, "seed {seed}: landed after the drink");
            answered += 1;
            near += d;
        }
        // The plain hardest bot throws at a drink too, without the aim.
        plain_near += drink_answered(false, 300 + seed).map_or(13.0, |(d, _)| d);
    }
    assert!(answered >= 10, "{answered} of 12 drinks answered");
    let (near, plain_near) = (near / answered as f64, plain_near / 12.0);
    eprintln!("lands {near:.2} blocks from the drinker, the plain bot {plain_near:.2}");
    assert!(near < plain_near, "lands {near} blocks from the drinker on average, the plain bot {plain_near}");
}

#[test]
fn off_by_default() {
    let p = Params::default();
    assert_eq!(p.pearl_punish, 0.0);
    assert_eq!(bot::BotParams::level(4).pearl_punish, bot::PearlPunish::OFF);
    let e = Env::new(Mode::Bot, params(false), 1);
    assert!(!e.bot.pearl_punish.on);
    let mut p = Params::default();
    assert!(p.set(230, 0.5) && p.set(231, 10.0) && p.set(232, 8.0) && p.set(233, 12.0) && p.set(234, 0.3));
    assert_eq!((p.pearl_punish, p.pearl_punish_hp, p.pearl_punish_far, p.pearl_punish_rush, p.pearl_punish_noise), (0.5, 10.0, 8.0, 12.0, 0.3));
    // Only the hardest bot punishes.
    let mut p = params(true);
    p.insane_prob = 0.0;
    p.bot_level = [4.0, 4.0];
    assert!(!Env::new(Mode::Bot, p, 1).bot.pearl_punish.on);
}
