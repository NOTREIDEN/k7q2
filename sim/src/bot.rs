//! Scripted opponents. They read the same client state a mod could read and
//! produce the same kind of input as the learning agent. They give early
//! training a target, and later serve as fixed yardsticks.
use crate::aabb::{V3, EYE_HEIGHT};
use crate::client::{Client, ClientCfg, Input, INV_SHUT};
use crate::mth::{self, Rng};
use crate::net::{refill_click, shift_click, Item, Kind, SLOT_FOOD, SLOT_SWORD, WINDOW_BAR};
use crate::world::World;

#[derive(Clone, Copy, Debug)]
pub struct BotParams {
    pub level: u8,
    /// Largest turn per tick, degrees.
    pub max_turn: f64,
    /// Standard deviation of the aim error added each tick, degrees.
    pub aim_noise: f64,
    /// Chance to click on a tick where the crosshair will be on target.
    pub click_prob: f64,
    pub strafe: bool,
    pub wtap: bool,
    pub jump_reset: bool,
    /// Chance to follow a hit with a short block (a block hit) instead of
    /// letting go of W.
    pub blockhit: f64,
    /// Chance to block for a moment when the opponent starts a swing
    /// within reach.
    pub block_react: f64,
    /// How readily it throws a pearl to get at the opponent: always next to
    /// it or behind it by the way the opponent faces, where it has to turn
    /// the furthest to hit back. 1 is about once per cooldown when there is
    /// room for it. With the same readiness it throws one as soon as it
    /// sees the opponent heal itself or run off hurt, to knock it away from
    /// its own potion.
    pub pearl_attack: f64,
    /// How readily it throws a pearl after three hits taken in a row: to
    /// the side or the back of the opponent most of the time if it also
    /// throws pearls to attack, and away from the opponent otherwise.
    pub pearl_escape: f64,
    /// Client ticks it waits between pearls: the server's cooldown.
    pub pearl_cd: i32,
    /// Ticks it aims ahead of the opponent on top of the flight of the
    /// pearl: the turn, the click, the way to the server and the age of
    /// what it sees of the opponent.
    pub pearl_lead: f64,
    /// How readily it throws a potion at its own feet once it is hurt
    /// enough. 0 is never. From level 3 on it turns its back and runs
    /// while it does, if the opponent is near; level 4 also waits for a
    /// moment in which the opponent is not on top of it, unless it is
    /// nearly dead.
    pub potion: f64,
    /// How readily it drinks its potion of Speed when the Speed is over or
    /// nearly, and eats when it is hungry (0 never).
    pub drink: f64,
    /// How readily it goes to its inventory for more potions once it has
    /// too few at hand (0 never). It stands still with the screen open,
    /// so the better bots wait for room, as for a drink.
    pub refill: f64,
    /// A runner: once hurt enough to heal itself it turns its back on the
    /// opponent and runs for as long as it has potions to throw, keeping
    /// off the walls, and comes back healed. With the same Speed nobody
    /// catches it on foot; a pearl does.
    pub runner: bool,
    /// It runs rings round the opponent: sideways one way for seconds on
    /// end, just inside its reach.
    pub circler: bool,
    /// It presses: it heals itself late, at four to six points of health,
    /// and while the opponent is hurt, or has a potion in hand or in the
    /// air, it goes straight at it, jumping to be faster, and throws its
    /// pearl at it as soon as it may. A potion thrown ahead with it that
    /// near heals both, and its hits knock the thrower away from where the
    /// potion breaks.
    pub presser: bool,
    /// It pots as the best players do: the pitch worked out from its own
    /// run for a potion that lands on it (`agent::aim_potion`), turned
    /// away and sprinting if the opponent is near, then backing into it
    /// with its view on the opponent until it breaks. `carry` is whether
    /// the server adds the thrower's movement to the throw.
    pub perfect_pot: bool,
    pub carry: bool,
    /// It pots as players do with the opponent on them
    /// (`Params::pot_turn_prob`): at 6 to 11 health, or after three hits
    /// taken in a row with 14 or less, it takes the potion in hand, turns
    /// its back and sprints away for 1 to `pot_run` ticks (a quarter of
    /// that at least) before it throws as it always does, and comes back
    /// into its potion.
    pub pot_turn: bool,
    pub pot_run: i32,
    /// It spaces its combos, as good players do: after a hit of its own,
    /// while the opponent cannot be hit again (its hurt time), it keeps
    /// out of the opponent's reach, letting go of W or backing off, and
    /// comes in again with a fresh sprint for the last `SPACE_IN` ticks of
    /// it, so that its next hit lands as early as the game allows and the
    /// opponent has had it within reach as little as it could manage. A
    /// player who sprints back at it straight away catches it backing off
    /// and can hit it first; one who waits for the next hit is comboed.
    pub spacer: bool,
}

/// What a spacer (`BotParams::spacer`) keeps between itself and the
/// opponent, feet to feet: three blocks of reach from the eye reach a
/// hitbox whose middle is this much further away, and a little for the
/// opponent's view of it being late. And the ticks of hurt time left at
/// which it comes in again.
pub const SPACE_OUT: f64 = 3.8;
pub const SPACE_IN: i32 = 4;

impl BotParams {
    pub fn level(level: u8) -> BotParams {
        match level {
            0 | 1 => BotParams {
                level,
                max_turn: 4.0,
                aim_noise: 0.0,
                click_prob: 0.0,
                strafe: false,
                wtap: false,
                jump_reset: false,
                blockhit: 0.0,
                block_react: 0.0,
                pearl_attack: 0.0,
                pearl_escape: 0.0,
                pearl_cd: 0,
                pearl_lead: 3.0,
                potion: 0.0,
                drink: 0.0,
                refill: 0.0,
                runner: false,
                circler: false,
                presser: false,
                perfect_pot: false,
                carry: false,
                pot_turn: false,
                pot_run: 0,
                spacer: false,
            },
            2 => BotParams {
                level,
                max_turn: 12.0,
                aim_noise: 2.0,
                click_prob: 0.45,
                strafe: false,
                wtap: false,
                jump_reset: false,
                blockhit: 0.0,
                block_react: 0.0,
                pearl_attack: 0.3,
                pearl_escape: 0.0,
                pearl_cd: 0,
                pearl_lead: 3.0,
                potion: 0.4,
                drink: 0.4,
                refill: 0.4,
                runner: false,
                circler: false,
                presser: false,
                perfect_pot: false,
                carry: false,
                pot_turn: false,
                pot_run: 0,
                spacer: false,
            },
            3 => BotParams {
                level,
                max_turn: 30.0,
                aim_noise: 0.8,
                click_prob: 0.7,
                strafe: true,
                wtap: true,
                jump_reset: false,
                blockhit: 0.25,
                block_react: 0.0,
                pearl_attack: 0.7,
                pearl_escape: 0.4,
                pearl_cd: 0,
                pearl_lead: 3.0,
                potion: 0.7,
                drink: 0.7,
                refill: 0.7,
                runner: false,
                circler: false,
                presser: false,
                perfect_pot: false,
                carry: false,
                pot_turn: false,
                pot_run: 0,
                spacer: false,
            },
            _ => BotParams {
                level: 4,
                max_turn: 90.0,
                aim_noise: 0.2,
                click_prob: 1.0,
                strafe: true,
                wtap: true,
                jump_reset: true,
                blockhit: 0.4,
                block_react: 0.25,
                pearl_attack: 1.0,
                pearl_escape: 0.8,
                pearl_cd: 0,
                pearl_lead: 3.0,
                potion: 1.0,
                drink: 1.0,
                refill: 1.0,
                runner: false,
                circler: false,
                presser: false,
                perfect_pot: false,
                carry: false,
                pot_turn: false,
                pot_run: 0,
                spacer: false,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BotState {
    strafe_dir: i8,
    strafe_left: i32,
    wtap_left: i32,
    walk_fwd: i8,
    walk_side: i8,
    walk_left: i32,
    walk_turn: i32,
    walk_sprint: bool,
    block_left: i32,
    saw_swing: bool,
    /// 0: no throw under way. 1: turning to the direction of the throw.
    /// 2: throw on this tick. 3: take the sword again.
    pub pearl_stage: u8,
    /// The spot the throw under way, or the last one, is meant to reach,
    /// and why it was thrown: 1 to attack, 2 to get out of a combo by
    /// landing next to the opponent, 3 to get away, 4 to get at an
    /// opponent that is healing itself (0: nothing thrown yet).
    pub pearl_to: V3,
    pub pearl_kind: u8,
    /// Ticks until the next pearl may be thrown.
    pearl_wait: i32,
    /// Hits taken since it last landed one.
    taken_run: i32,
    /// What it does about the combo it is in, chosen at its third hit: a
    /// pearl to the side or the back of the opponent, or one away from it.
    counter: bool,
    /// How the opponent has been moving per tick lately, as seen, with the
    /// jitter of single ticks smoothed out.
    pub seen_vel: [f64; 2],
    /// 0: no potion under way. 1: turning to throw it. 2: throw on this
    /// tick. 3: take the sword again.
    pub potion_stage: u8,
    /// Ticks until it may throw the next potion.
    potion_wait: i32,
    /// The health at or below which it heals itself in this duel (0: not
    /// drawn yet), and whether the throw under way is made running away.
    potion_at: f32,
    potion_away: bool,
    /// 0: neither drinking nor eating. 1: a potion of Speed in hand and
    /// the right button down. 2: the same with steak.
    pub use_stage: u8,
    /// Whether the client has been using the item since the button went
    /// down, and for how many ticks it has been down.
    use_seen: bool,
    use_ticks: i32,
    /// Ticks until it may try again, and for how many ticks it has wanted
    /// a drink or a meal without getting to it.
    use_wait: i32,
    use_need: i32,
    /// The ticks of Speed left at which it drinks and the food at or below
    /// which it eats in this duel, once drawn.
    use_drawn: bool,
    drink_at: i32,
    eat_at: i32,
    /// The inventory: whether its screen is open, the potions it still
    /// means to get down on this visit, the ticks until it may go again,
    /// for how many ticks it has wanted to, and, once drawn, the number
    /// of healing potions at hand at or below which it goes.
    pub inv_open: bool,
    inv_left: u8,
    inv_wait: i32,
    inv_need: i32,
    inv_drawn: bool,
    inv_keep: u32,
    /// A runner on the run (see `BotParams::runner`).
    pub fleeing: bool,
    /// The potion a perfect potter is throwing: the yaw and pitch it
    /// throws along, the way it runs, and the ticks it still backs into it.
    pot_yaw: f64,
    pot_pitch: f64,
    pot_dir: [f64; 2],
    pot_run: i32,
    /// Ticks of the run with its back turned still to go before the throw
    /// (`BotParams::pot_turn`), while `potion_stage` is 4.
    pot_turn_left: i32,
}

fn gauss(rng: &mut Rng) -> f64 {
    let u1 = rng.f64().max(1e-12);
    let u2 = rng.f64();
    (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
}

/// Yaw and pitch (degrees) that point from `eye` at `p`.
pub fn angles_to(eye: V3, p: V3) -> (f64, f64) {
    let dx = p[0] - eye[0];
    let dy = p[1] - eye[1];
    let dz = p[2] - eye[2];
    let h = (dx * dx + dz * dz).sqrt();
    ((-dx).atan2(dz).to_degrees(), -(dy.atan2(h)).to_degrees())
}

/// How far from the thrower a pearl thrown with this pitch (degrees, up is
/// negative) puts it on a floor `drop` blocks below the eyes, and after how
/// many ticks. The thrower ends up where the pearl was at the start of the
/// tick in which it reached the floor.
pub fn pearl_reach(pitch: f64, drop: f64) -> (f64, i32) {
    let (sp, cp) = pitch.to_radians().sin_cos();
    let (mut x, mut y) = (0.0f64, -0.1f64);
    let (mut vx, mut vy) = (1.5 * cp, -1.5 * sp);
    for t in 0..200 {
        if y + vy <= -drop {
            return (x, t);
        }
        x += vx;
        y += vy;
        vx *= 0.99;
        vy = vy * 0.99 - 0.03;
    }
    (x, 200)
}

/// The pitch that brings a pearl's thrower nearest to `dist` blocks away on
/// a floor `drop` blocks below its eyes, with the most direct throw that
/// does: from steeply down to 35 degrees up, first in steps of a degree,
/// then in steps of one mouse count around the best of those.
pub fn pearl_pitch(dist: f64, drop: f64) -> f64 {
    let mut best = (f64::MAX, 0.0);
    let scan = |from: f64, to: f64, step: f64, best: &mut (f64, f64)| {
        let mut pitch = from;
        while pitch >= to {
            let err = (pearl_reach(pitch, drop).0 - dist).abs();
            if err < best.0 - 0.02 {
                *best = (err, pitch);
            }
            pitch -= step;
        }
    };
    scan(85.0, -35.0, 1.0, &mut best);
    let mid = best.1;
    scan((mid + 0.9).min(85.0), (mid - 0.9).max(-35.0), 0.15, &mut best);
    best.1
}

/// How far ahead of its thrower a potion thrown with this pitch (degrees,
/// down is positive) is when it breaks on a floor `drop` blocks below the
/// eyes, and after how many ticks. As in the game, the potion is thrown
/// twenty degrees above the view, and its reach is measured from where it
/// was at the start of the tick in which it reached the floor.
pub fn potion_reach(pitch: f64, drop: f64) -> (f64, i32) {
    let (vx, vy) = (pitch.to_radians().cos(), -(pitch - 20.0).to_radians().sin());
    let len = vx.hypot(vy).max(1e-9);
    let (mut vx, mut vy) = (vx / len * 0.5, vy / len * 0.5);
    let (mut x, mut y) = (0.0f64, -0.1f64);
    for t in 0..200 {
        if y + vy <= -drop {
            return (x, t);
        }
        x += vx;
        y += vy;
        vx *= 0.99;
        vy = vy * 0.99 - 0.05;
    }
    (x, 200)
}

/// The pitch at which a potion breaks nearest to where a player running at
/// `speed` blocks per tick along its view will be by then.
pub fn potion_pitch(speed: f64, drop: f64) -> f64 {
    let mut best = (f64::MAX, 90.0);
    let mut pitch = 90.0;
    while pitch >= 0.0 {
        let (x, t) = potion_reach(pitch, drop);
        let err = (x - speed * t as f64).abs();
        if err < best.0 - 0.01 {
            best = (err, pitch);
        }
        pitch -= 2.0;
    }
    best.1
}

/// A spot for the bot to throw a pearl at and the kind of throw it is (see
/// `BotState::pearl_kind`), or `None`.
fn pearl_target(c: &Client, world: &World, p: &BotParams, st: &BotState, hdist: f64, rng: &mut Rng) -> Option<(V3, u8)> {
    let (me, them) = (c.lp.l.body.pos, c.rp.pos);
    let lim = world.half - 1.0;
    let inside = |q: &V3| q[0].abs() <= lim && q[2].abs() <= lim;
    let dir = if hdist > 1e-6 { [(them[0] - me[0]) / hdist, (them[2] - me[2]) / hdist] } else { [0.0, 1.0] };
    // Three hits taken in a row: time to get out of the combo.
    let comboed = st.taken_run >= 3 && rng.chance(0.5 * p.pearl_escape);
    let counter = comboed && st.counter;
    if comboed && !counter {
        // Away from the opponent, 10 to 18 blocks from here.
        for _ in 0..12 {
            let ang = rng.range(-1.2, 1.2);
            let (s, co) = ang.sin_cos();
            let d = [-(dir[0] * co - dir[1] * s), -(dir[0] * s + dir[1] * co)];
            let r = rng.range(10.0, 18.0);
            let q = [me[0] + d[0] * r, me[1], me[2] + d[1] * r];
            if inside(&q) {
                return Some((q, 3));
            }
        }
        // Cornered: anywhere far from the opponent.
        for _ in 0..12 {
            let q = [rng.range(-lim, lim), me[1], rng.range(-lim, lim)];
            if (q[0] - them[0]).hypot(q[2] - them[2]) > 9.0 && (q[0] - me[0]).hypot(q[2] - me[2]) > 6.0 {
                return Some((q, 3));
            }
        }
        return None;
    }
    let mut kind = if counter { 2 } else { 1 };
    if !counter {
        // The opponent has a potion in hand or in the air, or is hurt
        // enough to heal itself any moment: a pearl now puts the bot on it
        // while it does, and the hit that follows knocks it away from
        // where its potion breaks. A potion is in the air for a quarter of
        // a second, too short to answer, so the bot does not wait to see
        // one.
        let potting = c.rp.item == Item::Potion
            || c.thrown.iter().any(|e| e.kind == Kind::Potion && !e.mine)
            || (c.rp.using && matches!(c.rp.item, Item::Drink | Item::Food));
        let low = c.rp.health <= 10.0;
        let keen = if p.presser && p.pearl_attack > 0.0 { 1.0 } else { 0.6 * p.pearl_attack };
        if hdist > 2.8 && ((potting && rng.chance(keen)) || (low && rng.chance(0.25 * p.pearl_attack))) {
            kind = 4;
        } else {
            // Towards the opponent: often from far away, now and then in a
            // fight, and not in the half second after a hit on the opponent,
            // while it is being thrown about.
            let chance = if hdist > 7.0 {
                0.05
            } else if hdist > 3.5 {
                0.015
            } else {
                0.0
            };
            if c.rp.ticks_since_hurt <= 10 || !rng.chance(chance * p.pearl_attack) {
                return None;
            }
        }
    }
    // Where the opponent will be when the pearl gets there: after the
    // flight and the ticks of `pearl_lead`. A player on foot
    // loses about half its speed every tick and gains what its keys add,
    // so the way it moves now fades within a few ticks, and the bot takes
    // it to end up mostly running the way it faces.
    let n = (hdist / 1.4).min(20.0) + p.pearl_lead;
    let run = crate::living::Attr { sprint: true, ..c.lp.l.attr }.value() * 2.1;
    let yaw = (c.rp.yaw as f64).to_radians();
    let face = [-yaw.sin(), yaw.cos()];
    let flank = [-face[1], face[0]];
    let v = st.seen_vel;
    let goal = [v[0] + (face[0] * run - v[0]) * 0.7, v[1] + (face[1] * run - v[1]) * 0.7];
    let keep = 0.55f64;
    let fade = keep * (1.0 - keep.powf(n)) / (1.0 - keep);
    let moved = [goal[0] * n + (v[0] - goal[0]) * fade, goal[1] * n + (v[1] - goal[1]) * fade];
    let then = [them[0] + moved[0], them[2] + moved[1]];
    // An opponent that will be on top of the bot by then walks into any
    // pearl thrown past it.
    if !counter && (then[0] - me[0]).hypot(then[1] - me[2]) < 3.5 {
        return None;
    }
    // How near the straight line from here to `q` passes a point: a pearl
    // that touches the opponent on its way stops there, in front of it.
    let clear = |q: &V3, at: [f64; 2]| {
        let (ax, az) = (q[0] - me[0], q[2] - me[2]);
        let t = (((at[0] - me[0]) * ax + (at[1] - me[2]) * az) / (ax * ax + az * az).max(1e-9)).clamp(0.0, 1.0);
        (me[0] + ax * t - at[0]).hypot(me[2] + az * t - at[1])
    };
    // Next to the opponent or behind it, by the way it faces: never in
    // front, where it only has to keep looking.
    for _ in 0..16 {
        // On the side the opponent is moving away from, if it is moving
        // sideways at all: it will not walk into the pearl.
        let drift = v[0] * flank[0] + v[1] * flank[1];
        let sign = if drift.abs() > 0.05 { -drift.signum() } else if rng.chance(0.5) { 1.0 } else { -1.0 };
        // From close by there is no way around it to its back.
        let off = if hdist < 6.0 || rng.chance(0.5) {
            let (w, along) = (rng.range(1.4, 2.2) * sign, rng.range(-0.8, 0.0));
            [flank[0] * w + face[0] * along, flank[1] * w + face[1] * along]
        } else {
            let (a, w) = (rng.range(1.2, 2.0), rng.range(1.0, 1.9) * sign);
            [-face[0] * a + flank[0] * w, -face[1] * a + flank[1] * w]
        };
        let q = [(then[0] + off[0]).clamp(-lim, lim), them[1], (then[1] + off[1]).clamp(-lim, lim)];
        if (q[0] - me[0]).hypot(q[2] - me[2]) < 3.0 {
            continue;
        }
        // The opponent keeps moving while the bot turns and the pearl
        // flies: stay clear of all of the way it is expected to take.
        let mid = [(them[0] + then[0]) * 0.5, (them[2] + then[1]) * 0.5];
        if clear(&q, then) < 1.15 || clear(&q, mid) < 1.15 || clear(&q, [them[0], them[2]]) < 1.15 {
            continue;
        }
        // Still behind or beside it once the walls have moved the spot.
        let rel = [q[0] - then[0], q[2] - then[1]];
        let ahead = rel[0] * face[0] + rel[1] * face[1];
        if ahead > 0.5 || rel[0].hypot(rel[1]) < 1.2 {
            continue;
        }
        return Some((q, kind));
    }
    None
}

/// The yaw to run along away from the opponent (`dx`, `dz` and `hdist`
/// are where it is): straight away, or as near to that as the walls leave
/// room for five blocks on, the way nearest the current view first.
fn away_yaw(c: &Client, world: &World, dx: f64, dz: f64, hdist: f64) -> f64 {
    let lp = &c.lp;
    let eye = lp.eye();
    let me = lp.l.body.pos;
    let lim = world.half - 1.5;
    let away = if hdist > 1e-6 { [-dx / hdist, -dz / hdist] } else { [0.0, 1.0] };
    let yaw = (lp.l.yaw as f64).to_radians();
    let facing = [-yaw.sin(), yaw.cos()];
    let mut best: Option<([f64; 2], f64)> = None;
    'turns: for k in 0..=6 {
        for sign in [1.0, -1.0] {
            let (sn, cs) = (sign * k as f64 * 30f64.to_radians()).sin_cos();
            let d = [away[0] * cs - away[1] * sn, away[0] * sn + away[1] * cs];
            if (me[0] + d[0] * 5.0).abs() <= lim && (me[2] + d[1] * 5.0).abs() <= lim {
                let keep = d[0] * facing[0] + d[1] * facing[1];
                if best.map_or(true, |(_, b)| keep > b) {
                    best = Some((d, keep));
                }
            }
        }
        if best.is_some() {
            break 'turns;
        }
    }
    let d = best.map_or(away, |(d, _)| d);
    angles_to(eye, [me[0] + d[0] * 5.0, eye[1], me[2] + d[1] * 5.0]).0
}

/// How near the opponent has to be for a bot that pots with a turn
/// (`BotParams::pot_turn`) to turn its back first, and the health at or
/// below which it pots after three hits taken in a row.
const POT_TURN_NEAR: f64 = 8.0;
const POT_TURN_COMBO_HP: f32 = 14.0;

pub fn act(c: &Client, world: &World, cfg: &ClientCfg, p: &BotParams, st: &mut BotState, rng: &mut Rng) -> Input {
    let mut inp = act_flat(c, world, cfg, p, st, rng);
    if world.terrain().is_some() && p.level >= 2 {
        terrain_guard(c, world, &mut inp);
    }
    inp
}

/// How a bot gets about on terrain, on top of what it does on a flat
/// floor: it jumps onto a block in its way, goes around a wall it cannot
/// jump and does not walk into a drop it could not get out of (unless the
/// other player is down there).
fn terrain_guard(c: &Client, world: &World, inp: &mut Input) {
    use crate::agent::ground;
    let b = &c.lp.l.body;
    if !b.on_ground || (inp.forward == 0 && inp.strafe == 0) {
        return;
    }
    let (s, co) = (c.lp.l.yaw as f64).to_radians().sin_cos();
    // Where the keys move it: forward is (-sin, cos), left is (cos, sin).
    let dir = |fw: f64, left: f64| -> [f64; 2] {
        let v = [left * co - fw * s, fw * co + left * s];
        let n = v[0].hypot(v[1]).max(1e-9);
        [v[0] / n, v[1] / n]
    };
    let ahead = |d: [f64; 2], k: f64| -> f64 {
        ground(world, mth::floor_d(b.pos[0] + d[0] * k), mth::floor_d(b.pos[2] + d[1] * k), b.pos[1])
    };
    let worst = |d: [f64; 2]| -> (f64, f64) {
        let (a, f) = (ahead(d, 0.7), ahead(d, 1.4));
        (a.min(f), a.max(f))
    };
    let rel_y = c.rp.pos[1] - b.pos[1];
    let d = dir(inp.forward as f64, inp.strafe as f64);
    let (low, high) = worst(d);
    let drop = low <= -2.0 && !(rel_y < -1.5 && low > crate::agent::NO_GROUND);
    if high > 0.6 && high <= 1.25 && !drop {
        // A block to get up onto.
        inp.jump = true;
        return;
    }
    if drop || high > 1.25 {
        // A drop or a wall: the way to one side or the other that is
        // open, the one nearer the way it wanted to go first.
        for (fw, left) in [(inp.forward as f64, 1.0), (inp.forward as f64, -1.0), (0.0, 1.0), (0.0, -1.0), (-1.0, 0.0)] {
            let (l, h) = worst(dir(fw, left));
            if l > -2.0 && h <= 1.25 {
                inp.forward = fw as i8;
                inp.strafe = left as i8;
                inp.jump = h > 0.6;
                return;
            }
        }
        inp.forward = 0;
        inp.strafe = 0;
    }
}

fn act_flat(c: &Client, world: &World, cfg: &ClientCfg, p: &BotParams, st: &mut BotState, rng: &mut Rng) -> Input {
    let mut inp = Input::default();
    let deg_per_count = cfg.mouse_f1 as f64 * 0.15;
    if p.level == 0 {
        return inp;
    }
    if p.level == 1 {
        // Wanderer: holds random keys for random stretches, never attacks.
        if st.walk_left <= 0 {
            st.walk_left = 5 + (rng.next_u64() % 30) as i32;
            st.walk_fwd = (rng.next_u64() % 3) as i8 - 1;
            st.walk_side = (rng.next_u64() % 3) as i8 - 1;
            st.walk_turn = (rng.next_u64() % 41) as i32 - 20;
            st.walk_sprint = rng.chance(0.5);
        }
        st.walk_left -= 1;
        inp.forward = st.walk_fwd;
        inp.strafe = st.walk_side;
        inp.sprint_key = st.walk_sprint;
        inp.jump = rng.chance(0.03);
        inp.dx = st.walk_turn;
        // Keep the head roughly level.
        inp.dy = (-(c.lp.l.pitch as f64) / deg_per_count * 0.2).round() as i32;
        return inp;
    }

    let lp = &c.lp;
    let rp = &c.rp;
    let eye = lp.eye();
    // Lead the target by one tick of its visible movement.
    let lead = [rp.pos[0] - rp.prev_pos[0], rp.pos[1] - rp.prev_pos[1], rp.pos[2] - rp.prev_pos[2]];
    let ty = eye[1].clamp(rp.pos[1] + 0.3, rp.pos[1] + 1.6);
    let aim = [rp.pos[0] + lead[0], ty + lead[1], rp.pos[2] + lead[2]];
    let (yaw_to, pitch_to) = angles_to(eye, aim);
    let mut dyaw = mth::wrap_deg(yaw_to - lp.l.yaw as f64);
    let mut dpitch = pitch_to - lp.l.pitch as f64;
    dyaw = dyaw.clamp(-p.max_turn, p.max_turn) + p.aim_noise * gauss(rng);
    dpitch = dpitch.clamp(-p.max_turn, p.max_turn) + p.aim_noise * 0.5 * gauss(rng);
    inp.dx = (dyaw / deg_per_count).round() as i32;
    inp.dy = (dpitch / deg_per_count).round() as i32;

    let dx = rp.pos[0] - lp.l.body.pos[0];
    let dz = rp.pos[2] - lp.l.body.pos[2];
    let hdist = (dx * dx + dz * dz).sqrt();

    // The inventory. With the screen open the game takes no other key, so
    // a tick of it is nothing but the click or the key that shuts it. It
    // gets the potion of Speed down first if the Speed is over, then
    // healing potions, one a tick, and shuts the screen with the last.
    if st.inv_open {
        let bar = lp.bar();
        let cup = lp.drinks == 0 && lp.speed_ticks <= st.drink_at.max(0);
        let pick = |b: &crate::net::Bar, r: &crate::net::Rows| {
            refill_click(b, r, Item::Drink).filter(|_| cup).or_else(|| refill_click(b, r, Item::Potion))
        };
        let click = if st.inv_left > 0 { pick(&bar, &lp.rows) } else { None };
        let mut screen = Input::default();
        match click {
            Some(slot) => {
                if slot < WINDOW_BAR {
                    st.inv_left -= 1;
                }
                let (mut b, mut r) = (bar, lp.rows);
                shift_click(&mut b, &mut r, slot);
                if st.inv_left > 0 && refill_click(&b, &r, Item::Potion).is_some() {
                    screen.inv = slot;
                } else if slot >= WINDOW_BAR {
                    screen.inv = slot;
                } else {
                    screen.inv = slot + INV_SHUT;
                    st.inv_open = false;
                    st.inv_wait = 20;
                }
            }
            None => {
                st.inv_open = false;
                st.inv_wait = 20;
            }
        }
        return screen;
    }
    if st.inv_wait > 0 {
        st.inv_wait -= 1;
    }
    // Nothing is drawn in a duel without potions in the rows, so that the
    // others go as they always did.
    let (more_p, more_d) = (lp.rows.count(Item::Potion) > 0, lp.rows.count(Item::Drink) > 0);
    if p.refill > 0.0 && (more_p || more_d) {
        if !st.inv_drawn {
            st.inv_drawn = true;
            st.inv_keep = (rng.next_u64() % 2) as u32;
        }
        let bar = lp.bar();
        let pots = more_p && lp.potions.count_ones() <= st.inv_keep && refill_click(&bar, &lp.rows, Item::Potion).is_some();
        let cups = more_d && lp.drinks == 0 && lp.speed_ticks <= st.drink_at.max(0) && refill_click(&bar, &lp.rows, Item::Drink).is_some();
        let free = st.inv_wait == 0
            && st.pearl_stage == 0
            && st.potion_stage == 0
            && st.use_stage == 0
            && lp.held == SLOT_SWORD
            && st.block_left == 0;
        if (pots || cups) && free {
            st.inv_need += 1;
            let busy = rp.using && matches!(rp.item, Item::Drink | Item::Food);
            let room = [0.0, 0.0, 3.0, 5.0, 7.0][p.level.min(4) as usize];
            // Hurt with nothing to heal with, it cannot wait for long.
            let urgent = (st.inv_need > 60 && hdist > 0.5 * room) || (pots && lp.potions == 0 && lp.health <= 9.0 && st.inv_need > 8);
            if (hdist > room || busy || urgent) && rng.chance(0.3 * p.refill) {
                st.inv_open = true;
                st.inv_left = 1 + (rng.next_u64() % 5) as u8 + cups as u8;
                st.inv_need = 0;
                return Input { inv: 1, ..Input::default() };
            }
        } else {
            st.inv_need = 0;
        }
    }

    inp.sprint_key = true;
    inp.forward = if hdist > 1.2 { 1 } else { 0 };
    if p.wtap {
        if rp.hurt_time == 10 && lp.ticks_since_attack <= 12 {
            // A hit landed: reset the sprint, by letting go of W or by a
            // short block.
            if rng.chance(p.blockhit) {
                st.block_left = 1 + (rng.next_u64() % 2) as i32;
            } else {
                st.wtap_left = 2;
            }
        }
        if st.wtap_left > 0 {
            st.wtap_left -= 1;
            inp.forward = 0;
        }
    }
    let swing_starts = rp.swing_in_progress && !st.saw_swing;
    st.saw_swing = rp.swing_in_progress;
    if swing_starts && hdist < 3.6 && lp.hurt_time <= 1 && st.block_left == 0 && rng.chance(p.block_react) {
        st.block_left = 2;
    }
    if st.block_left > 0 {
        st.block_left -= 1;
        inp.use_item = true;
    }
    if p.circler {
        if st.strafe_left <= 0 {
            st.strafe_left = 60 + (rng.next_u64() % 140) as i32;
            st.strafe_dir = if rng.chance(0.5) { 1 } else { -1 };
        }
        st.strafe_left -= 1;
        inp.strafe = st.strafe_dir;
        if hdist <= 2.4 {
            inp.forward = 0;
        }
    } else if p.strafe && hdist < 6.0 {
        if st.strafe_left <= 0 {
            st.strafe_left = 6 + (rng.next_u64() % 18) as i32;
            st.strafe_dir = if rng.chance(0.5) { 1 } else { -1 };
        }
        st.strafe_left -= 1;
        inp.strafe = st.strafe_dir;
    }
    if p.jump_reset && lp.hurt_time == 10 && lp.l.body.on_ground {
        inp.jump = true;
    }
    // A spacer that hit last, while its hit still keeps the opponent from
    // being hit: out of the opponent's reach until its last ticks, or at
    // the end of its own reach if that is longer than three blocks.
    let keep = SPACE_OUT.max(c.edge.reach + 0.25);
    let spacing = p.spacer && rp.hurt_time > SPACE_IN && lp.hurt_time < rp.hurt_time && hdist < keep;
    if spacing {
        inp.forward = if hdist < keep - 0.3 { -1 } else { 0 };
    }
    // A presser on an opponent that is hurt or healing itself: straight
    // at it, in jumps while it is out of reach.
    if p.presser && (rp.health <= 12.0 || rp.item == Item::Potion || c.thrown.iter().any(|e| e.kind == Kind::Potion && !e.mine)) {
        inp.strafe = 0;
        if hdist > 3.2 && lp.l.body.on_ground && !spacing {
            inp.jump = true;
        }
    }

    // A runner that is hurt runs from the opponent until it is healed or
    // out of potions, which it throws ahead of itself as it goes (below).
    // It heads straight away from the opponent, or as near to that as the
    // walls leave room for five blocks on, and jumps to be faster.
    let mut flee_yaw = None;
    if p.runner {
        if !st.fleeing {
            st.fleeing = lp.potions != 0 && st.potion_at > 0.0 && lp.health <= st.potion_at && hdist < 12.0;
        } else if lp.potions == 0 || lp.health >= 16.0 || hdist > 24.0 {
            st.fleeing = false;
        }
        if st.fleeing {
            let want = away_yaw(c, world, dx, dz, hdist);
            let lim = p.max_turn.max(45.0);
            inp.dx = (mth::wrap_deg(want - lp.l.yaw as f64).clamp(-lim, lim) / deg_per_count).round() as i32;
            inp.dy = ((-(lp.l.pitch as f64)).clamp(-lim, lim) / deg_per_count).round() as i32;
            inp.forward = 1;
            inp.strafe = 0;
            inp.sprint_key = true;
            inp.use_item = false;
            inp.jump = lp.l.body.on_ground && st.potion_stage == 0;
            flee_yaw = Some(want);
        }
    }

    // Would the crosshair be on the target after this turn?
    let new_yaw = (lp.l.yaw as f64 + inp.dx as f64 * deg_per_count) as f32;
    let new_pitch = ((lp.l.pitch as f64 + inp.dy as f64 * deg_per_count) as f32).clamp(-90.0, 90.0);
    let ray_yaw = if cfg.mouse_delay_fix { new_yaw } else { lp.yaw_head };
    let pk = c.pick_ray(world, new_pitch, ray_yaw);
    if !st.fleeing && pk.entity && rng.chance(p.click_prob) {
        inp.attack = true;
    }

    // Pearls. The server throws along the view it last heard of and from
    // where it last heard the player was, so the bot first turns to the
    // direction of the throw as seen from where this tick will take it
    // (without touching the movement keys, so that is known), throws on the
    // next tick (number key 2 and a right click) and takes the sword again
    // on the one after (number key 1).
    let step = [rp.pos[0] - rp.prev_pos[0], rp.pos[2] - rp.prev_pos[2]];
    if step[0].hypot(step[1]) < 1.5 {
        st.seen_vel = [(st.seen_vel[0] + step[0]) * 0.5, (st.seen_vel[1] + step[1]) * 0.5];
    }
    if lp.hurt_time == 10 {
        st.taken_run += 1;
        if st.taken_run == 3 {
            st.counter = p.pearl_attack > 0.0 && rng.chance(0.65);
        }
    }
    if (rp.hurt_time == 10 && lp.ticks_since_attack <= 12) || lp.ticks_since_hurt > 30 {
        st.taken_run = 0;
    }
    if st.pearl_wait > 0 {
        st.pearl_wait -= 1;
    }
    match st.pearl_stage {
        0 => {
            let ready = (p.pearl_attack > 0.0 || p.pearl_escape > 0.0)
                && lp.pearls > 0
                && st.pearl_wait == 0
                && lp.held == SLOT_SWORD
                && st.potion_stage == 0
                && st.use_stage == 0
                && st.block_left == 0
                && !st.fleeing;
            if ready {
                if let Some((q, kind)) = pearl_target(c, world, p, st, hdist, rng) {
                    st.pearl_to = q;
                    st.pearl_kind = kind;
                    st.pearl_stage = 1;
                    st.taken_run = 0;
                }
            }
        }
        1 => {
            let b = &lp.l.body;
            let from = [b.pos[0] + b.motion[0], (b.pos[1] + b.motion[1]).max(world.floor_y) + EYE_HEIGHT, b.pos[2] + b.motion[2]];
            let q = st.pearl_to;
            let want_yaw = angles_to(from, q).0;
            let want_pitch = pearl_pitch((q[0] - from[0]).hypot(q[2] - from[2]), from[1] - world.floor_y);
            let lim = p.max_turn.max(45.0);
            let dyaw = mth::wrap_deg(want_yaw - lp.l.yaw as f64);
            let dpitch = want_pitch - lp.l.pitch as f64;
            inp.dx = (dyaw.clamp(-lim, lim) / deg_per_count).round() as i32;
            inp.dy = (dpitch.clamp(-lim, lim) / deg_per_count).round() as i32;
            inp.forward = 0;
            inp.strafe = 0;
            inp.jump = false;
            inp.attack = false;
            inp.use_item = false;
            if dyaw.abs() <= lim && dpitch.abs() <= lim {
                st.pearl_stage = 2;
            }
        }
        2 => {
            inp.slot_key = 2;
            inp.use_item = true;
            inp.attack = false;
            st.pearl_stage = 3;
            st.pearl_wait = p.pearl_cd + 4;
        }
        _ => {
            inp.slot_key = 1;
            inp.use_item = false;
            st.pearl_stage = 0;
        }
    }

    // Potions. Once hurt enough it looks at the ground a little ahead of
    // where it is going, throws on the next tick and takes the sword again
    // on the one after, like a pearl.
    if st.potion_wait > 0 {
        st.potion_wait -= 1;
    }
    if st.potion_at == 0.0 && p.potion > 0.0 && lp.potions != 0 {
        let (lo, hi) = if p.pot_turn { (6.0, 11.0) } else if p.presser { (4.0, 6.0) } else { (7.0, 12.0) };
        st.potion_at = rng.range(lo, hi) as f32;
    }
    match st.potion_stage {
        0 => {
            // On the run it heals itself up: another potion while six of
            // its eight points would not be wasted.
            let comboed = p.pot_turn && st.taken_run >= 3 && lp.health <= POT_TURN_COMBO_HP;
            let ready = p.potion > 0.0
                && lp.potions != 0
                && (lp.health <= st.potion_at || (st.fleeing && lp.health <= 14.0) || comboed)
                && st.potion_wait == 0
                && st.pearl_stage == 0
                && st.use_stage == 0
                && lp.held == SLOT_SWORD
                && st.block_left == 0;
            let good_moment = st.fleeing || p.level < 4 || hdist > 3.5 || rp.ticks_since_hurt <= 8 || lp.health <= 6.0;
            if ready && good_moment && rng.chance(0.35 * p.potion) {
                st.potion_stage = 1;
                st.potion_away = st.fleeing || (p.level >= 3 && hdist < 6.0);
                if p.pot_turn && !st.fleeing && hdist < POT_TURN_NEAR {
                    // The potion in hand, the back turned, a run first.
                    let most = p.pot_run.max(1);
                    st.pot_turn_left = ((most as f64 * rng.range(0.25, 1.0)).round() as i32).clamp(1, most);
                    st.potion_stage = 4;
                    st.potion_away = true;
                    st.taken_run = 0;
                } else if p.perfect_pot {
                    let yaw = if st.potion_away { flee_yaw.unwrap_or(yaw_to + 180.0) } else { lp.l.yaw as f64 };
                    let run = crate::agent::run_feet(c, world, yaw as f32);
                    let them = c.rp.pos;
                    let (pitch, ticks) = crate::agent::aim_potion(c, world, yaw as f32, &run, &[if p.carry { 1.0 } else { 0.0 }], |_| them);
                    let f = yaw.to_radians();
                    st.pot_yaw = yaw;
                    st.pot_pitch = pitch as f64;
                    st.pot_dir = [-f.sin(), f.cos()];
                    st.pot_run = ticks + 2;
                }
            }
        }
        4 => {
            // Its back to the opponent, sprinting away with the potion in
            // hand; at the end of the run the throw goes on as usual.
            let want = away_yaw(c, world, dx, dz, hdist);
            let lim = p.max_turn.max(45.0);
            inp.dx = (mth::wrap_deg(want - lp.l.yaw as f64).clamp(-lim, lim) / deg_per_count).round() as i32;
            inp.dy = ((-(lp.l.pitch as f64)).clamp(-lim, lim) / deg_per_count).round() as i32;
            inp.attack = false;
            inp.use_item = false;
            inp.forward = 1;
            inp.strafe = 0;
            inp.jump = false;
            inp.sprint_key = true;
            inp.slot_key = lp.potions.trailing_zeros() as u8 + 1;
            st.pot_turn_left -= 1;
            if st.pot_turn_left <= 0 || lp.potions == 0 {
                st.potion_stage = if lp.potions == 0 { 3 } else { 1 };
                if p.perfect_pot && lp.potions != 0 {
                    let run = crate::agent::run_feet(c, world, want as f32);
                    let them = c.rp.pos;
                    let (pitch, ticks) = crate::agent::aim_potion(c, world, want as f32, &run, &[if p.carry { 1.0 } else { 0.0 }], |_| them);
                    let f = want.to_radians();
                    st.pot_yaw = want;
                    st.pot_pitch = pitch as f64;
                    st.pot_dir = [-f.sin(), f.cos()];
                    st.pot_run = ticks + 2;
                }
            }
        }
        1 if p.perfect_pot => {
            // The turn and the pitch in one tick, sprinting along the view.
            let dyaw = mth::wrap_deg(st.pot_yaw - lp.l.yaw as f64);
            let dpitch = st.pot_pitch - lp.l.pitch as f64;
            inp.dx = (dyaw / deg_per_count).round() as i32;
            inp.dy = (dpitch / deg_per_count).round() as i32;
            inp.attack = false;
            inp.use_item = false;
            inp.forward = 1;
            inp.strafe = 0;
            inp.jump = false;
            inp.sprint_key = true;
            inp.slot_key = lp.potions.trailing_zeros() as u8 + 1;
            st.potion_stage = 2;
        }
        2 if p.perfect_pot => {
            inp.slot_key = lp.potions.trailing_zeros() as u8 + 1;
            inp.use_item = true;
            inp.attack = false;
            inp.dx = 0;
            inp.dy = 0;
            inp.forward = 1;
            inp.strafe = 0;
            inp.jump = false;
            inp.sprint_key = true;
            st.potion_stage = 3;
            st.potion_wait = 10;
        }
        1 => {
            let b = &lp.l.body;
            let speed = if st.potion_away && !st.fleeing { 0.33 } else { b.motion[0].hypot(b.motion[2]) / 0.546 };
            let want_pitch = potion_pitch(speed.min(0.6), b.pos[1] + EYE_HEIGHT - world.floor_y);
            let lim = p.max_turn.max(45.0);
            let dpitch = want_pitch - lp.l.pitch as f64;
            let dyaw = if st.potion_away { mth::wrap_deg(flee_yaw.unwrap_or(yaw_to + 180.0) - lp.l.yaw as f64) } else { 0.0 };
            inp.dx = (dyaw.clamp(-lim, lim) / deg_per_count).round() as i32;
            inp.dy = (dpitch.clamp(-lim, lim) / deg_per_count).round() as i32;
            inp.attack = false;
            inp.use_item = false;
            if st.potion_away {
                inp.forward = 1;
                inp.strafe = 0;
                inp.sprint_key = true;
            }
            if dyaw.abs() <= lim && dpitch.abs() <= lim {
                st.potion_stage = 2;
            }
        }
        2 => {
            inp.slot_key = lp.potions.trailing_zeros() as u8 + 1;
            inp.use_item = true;
            inp.attack = false;
            inp.dx = 0;
            inp.dy = 0;
            if st.potion_away {
                inp.forward = 1;
                inp.strafe = 0;
            }
            st.potion_stage = 3;
            st.potion_wait = 16 + (rng.next_u64() % 12) as i32;
        }
        _ => {
            inp.slot_key = 1;
            inp.use_item = false;
            st.potion_stage = 0;
        }
    }
    if p.perfect_pot && st.pot_run > 0 && st.potion_stage != 1 && st.potion_stage != 2 && st.potion_stage != 4 {
        // Backing into its potion, the view back on the opponent.
        let yaw = (lp.l.yaw as f64 + inp.dx as f64 * deg_per_count) as f32;
        let (fw, sd) = crate::agent::walk_along(yaw, st.pot_dir);
        inp.forward = fw;
        inp.strafe = sd;
        inp.sprint_key = false;
        inp.jump = false;
        st.pot_run -= 1;
    }

    // Drinks and meals. When its Speed is over, or nearly for the better
    // bots, and it carries a potion of it, or when it is hungry, it takes
    // the potion or the steak in hand and keeps the right button down
    // until the server says it is done. The better the bot, the more room
    // it wants for that, though not for ever; the best one backs off
    // meanwhile and gives up when the opponent gets to it too early.
    if st.use_wait > 0 {
        st.use_wait -= 1;
    }
    // Drawn only in a duel that has something to drink or eat, so that
    // the others go as they always did.
    if !st.use_drawn && p.drink > 0.0 && (lp.drinks != 0 || lp.steak > 0) {
        st.use_drawn = true;
        let (lo, hi) = [(0.0, 0.0), (0.0, 0.0), (0.0, 40.0), (20.0, 100.0), (40.0, 160.0)][p.level.min(4) as usize];
        st.drink_at = rng.range(lo, hi).round() as i32;
        let (lo, hi) = [(6.0, 6.0), (6.0, 6.0), (6.0, 8.0), (6.0, 12.0), (8.0, 17.0)][p.level.min(4) as usize];
        st.eat_at = rng.range(lo, hi).round() as i32;
    }
    match st.use_stage {
        0 => {
            let drink = lp.drinks != 0 && lp.speed_ticks <= st.drink_at;
            let eat = lp.steak > 0 && lp.food <= st.eat_at && lp.food < 20;
            let free = st.use_drawn
                && st.use_wait == 0
                && st.pearl_stage == 0
                && st.potion_stage == 0
                && lp.held == SLOT_SWORD
                && st.block_left == 0;
            if (drink || eat) && free && !st.fleeing {
                st.use_need += 1;
                let busy = rp.using && matches!(rp.item, Item::Drink | Item::Food);
                let room = [0.0, 0.0, 3.0, 5.0, 7.0][p.level.min(4) as usize];
                let urgent = st.use_need > 80 && hdist > 0.5 * room;
                if (hdist > room || busy || urgent) && rng.chance(0.3 * p.drink) {
                    // The drink first: without Speed it loses every race.
                    let slot = if drink { lp.drinks.trailing_zeros() as u8 } else { SLOT_FOOD };
                    inp.slot_key = slot + 1;
                    inp.use_item = true;
                    inp.attack = false;
                    st.use_stage = if drink { 1 } else { 2 };
                    st.use_seen = false;
                    st.use_ticks = 0;
                    st.use_need = 0;
                }
            } else if !(drink || eat) {
                st.use_need = 0;
            }
        }
        _ => {
            st.use_ticks += 1;
            let using = lp.consuming();
            let done = st.use_seen && !using;
            let never = !st.use_seen && !using && st.use_ticks > 3;
            let caught = p.level >= 4 && hdist < 3.2 && using && lp.use_count > 12;
            if done || never || caught {
                inp.slot_key = SLOT_SWORD + 1;
                inp.use_item = false;
                st.use_stage = 0;
                st.use_wait = if caught { 25 } else { 8 };
            } else {
                inp.use_item = true;
                inp.attack = false;
                if p.level >= 3 {
                    inp.forward = -1;
                    inp.sprint_key = false;
                }
            }
            st.use_seen |= using;
        }
    }
    inp
}
