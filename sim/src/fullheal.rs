//! A healing potion thrown with a turn that gives all eight
//! points. On the tick of the turn the agent works out, from its own
//! client, a few ways to go on (its own keys held, no key, a brake against
//! its motion; each with a jump on one of the first ticks or none) and, for
//! each, the pitches from straight up to straight down. A way and a pitch
//! are taken when the potion gives eight points in the worst case over the
//! server's random part of the throw (2.5 and 5 deviations along each axis
//! and on the diagonals), over the server adding the thrower's movement or
//! not (unless the agent knows which), and over every way the packets can
//! be a tick late (jitter). The keys of that way are then held until the
//! potion is due, laid over the policy's; the view is the policy's.
//!
//! The potion is the server's (`EntityThrowable`): it breaks where it
//! first meets a block, or on its thrower's box (grown by 0.3) from its
//! fifth tick on; a potion that breaks on a block gives
//! `8 * (1 - d / 4)` rounded, d the distance to the feet. The server has
//! the player where the packets of client tick k put it on the potion's
//! tick k (tick 1 is the click), or of tick k - 1 or k + 1 when a packet
//! or the click came a server tick late.
use crate::aabb::{dist_sq, Aabb, V3};
use crate::client::{Client, ClientCfg, Input};
use crate::mth;
use crate::net::{Kind, C2S};
use crate::server::{splash_heal, throw_heading, POTION_HEAL, THROW_SPREAD};
use crate::world::{RayHit, World};
use std::sync::atomic::{AtomicU64, Ordering};

/// Ticks of keys a plan holds at most, from the turn's tick.
pub const PLAN_TICKS: usize = 12;
/// The latest tick of the potion a plan may heal on.
const MAX_HEAL_TICK: i32 = 9;

/// Counters for the evaluation: 0 turned throws seen, 1-3 planned by class
/// (own keys, no key, brake), 4 not planned, 5 plans dropped by a hit,
/// 6-11 planned with no jump / a jump on tick 0-4, 12-18 heal on potion
/// tick 3-9, 19-20 thrown upward / downward, 21 planned with the carry
/// known, 22-23 not planned with the carry unknown / in the air, 24-35
/// ticks since the last hurt at a turned throw (0 to 11 or more), 36-45 the
/// distance to the other player (whole blocks, 0 to 9 or more), 46 turned
/// throws left to the policy by the gate (see `gate_open`).
pub const NCOUNTS: usize = 48;
#[allow(clippy::declare_interior_mutable_const)]
const ZERO: AtomicU64 = AtomicU64::new(0);
pub static COUNTS: [AtomicU64; NCOUNTS] = [ZERO; NCOUNTS];

pub fn count(i: usize) {
    COUNTS[i].fetch_add(1, Ordering::Relaxed);
}

/// When and how agents plan their turned throws (`Agent::full_heal`, set
/// from `Params::full_heal*`).
///
/// A hit while the potion is in the air knocks the player off it (a
/// potion thrown up then comes down some twenty ticks later, away from
/// it), so a plan is only tried when no hit can come in time: the last
/// hurt so recent that the server's hurt time still covers the flight
/// (`ticks_since_hurt <= s`), or the other player too far to reach it
/// (`dist >= d` blocks, 0 for no such test). s = 4 (the hurt time of 10
/// ticks then covers the potion's ticks to its heal, 5, and one tick of
/// jitter) and no distance measured best; s >= 100 with d = 0 opens it
/// always, s < 0 shuts the hurt test.
///
/// `restore`: how the view gets its pitch back after the throw: 1 on the
/// click's tick to the pitch the policy chose for the turn plus its own
/// turn, 2 on the tick after the click to the pitch before the turn, 3 on
/// the click's tick to the policy's pitch, 4 on the click's tick to the
/// pitch before the turn. 4 measured best: the server throws with the turn
/// tick's view, so only that tick looks away (trader: 1.41 ticks away per
/// potion against 1.45 for 2 and 2.0 for 1 and 3; what is left over the
/// plain throw's 0.80 is mostly the jump lifting the eye off the line to
/// the other player).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FullHealCfg {
    pub s: i32,
    pub d: i32,
    pub restore: i32,
}

impl Default for FullHealCfg {
    fn default() -> Self {
        FullHealCfg { s: 4, d: 0, restore: 4 }
    }
}

/// Whether a plan is tried for this throw (see `FullHealCfg`).
pub fn gate_open(cfg: &FullHealCfg, ticks_since_hurt: i32, dist: f64) -> bool {
    let (smax, dmin) = (cfg.s, cfg.d);
    if smax >= 100 && dmin <= 0 {
        return true;
    }
    (smax >= 0 && smax < 100 && ticks_since_hurt <= smax) || (dmin > 0 && dist >= dmin as f64)
}

/// Movement keys of one tick, for the view the plan was made for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PKey {
    pub fw: i8,
    pub st: i8,
    pub sprint: bool,
    pub jump: bool,
}

#[derive(Clone, Debug)]
pub struct HealPlan {
    /// Mouse counts down on the turn's tick, the yaw the keys are for, the
    /// keys from the turn's tick on and how many ticks they are held.
    pub dy: i32,
    pub yaw: f32,
    pub keys: [PKey; PLAN_TICKS],
    pub len: usize,
    /// The potion's tick of the heal in the worst case, the class of the
    /// way (0 own keys, 1 no key, 2 brake), the window of pitch (degrees).
    pub ticks: i32,
    pub class: u8,
    pub window: i32,
    /// The pitch of the throw and the tick of the jump (-1 none).
    pub pitch: f32,
    pub jump: i8,
}

/// What the server has of the player after a client tick.
#[derive(Clone, Copy, Debug)]
struct Rep {
    feet: V3,
    on_ground: bool,
    moved: V3,
    yaw: f32,
}

#[derive(Clone, Copy, Debug)]
struct Up {
    pos: V3,
    end: V3,
    block: bool,
}

/// The client's own ticks with these keys, from a copy of it: the turn's
/// tick has the mouse and number key of `inp`.
fn predict(c: &Client, world: &World, inp: &Input, keys: &[PKey; PLAN_TICKS]) -> Vec<Rep> {
    let mut cc = c.clone();
    let cfg = ClientCfg::with_sensitivity(0.5);
    let mut out: Vec<C2S> = Vec::with_capacity(8);
    let mut feet = cc.lp.last_reported;
    let mut yaw = cc.lp.last_yaw;
    let mut reps = Vec::with_capacity(PLAN_TICKS + 2);
    for t in 0..PLAN_TICKS + 2 {
        let k = keys[t.min(PLAN_TICKS - 1)];
        let mut i = Input { forward: k.fw, strafe: k.st, jump: k.jump && t < PLAN_TICKS, sprint_key: k.sprint, ..Input::default() };
        if t == 0 {
            i.dx = inp.dx;
            i.slot_key = inp.slot_key;
        }
        out.clear();
        cc.tick(world, &i, &cfg, &mut out);
        let mut moved = [0.0; 3];
        let mut og = cc.lp.l.body.on_ground;
        for p in out.iter() {
            if let C2S::Player { pos, rot, on_ground } = p {
                if let Some(d) = pos {
                    moved = [d[0] - feet[0], d[1] - feet[1], d[2] - feet[2]];
                    feet = *d;
                }
                if let Some((y, _)) = rot {
                    yaw = *y;
                }
                og = *on_ground;
            }
        }
        reps.push(Rep { feet, on_ground: og, moved, yaw: yaw % 360.0f32 });
    }
    reps
}

fn throw(r0: &Rep, pitch: f32, z: [f64; 3], carry: bool) -> (V3, V3) {
    let (pos, dir) = throw_heading(Kind::Potion, r0.feet, r0.yaw, pitch % 360.0f32);
    let v = Kind::Potion.speed() as f64;
    let mut m = [0.0; 3];
    for a in 0..3 {
        m[a] = (dir[a] + z[a] * THROW_SPREAD) * v;
    }
    if carry {
        m[0] += r0.moved[0];
        m[2] += r0.moved[2];
        if !r0.on_ground {
            m[1] += r0.moved[1];
        }
    }
    (pos, m)
}

fn path(world: &World, mut pos: V3, mut m: V3, out: &mut Vec<Up>) {
    out.clear();
    let (f2, g) = (0.99f32 as f64, Kind::Potion.gravity() as f64);
    for _ in 0..(MAX_HEAL_TICK as usize + 3) {
        let mut end = [pos[0] + m[0], pos[1] + m[1], pos[2] + m[2]];
        let mut block = false;
        if !world.clear_path(pos, end) {
            if let RayHit::Block(h) = world.ray_trace_blocks(pos, end, false) {
                end = h;
                block = true;
            }
        }
        out.push(Up { pos, end, block });
        if block {
            return;
        }
        for a in 0..3 {
            pos[a] += m[a];
            m[a] *= f2;
        }
        m[1] -= g;
    }
}

fn splash(at: V3, f: V3, direct: bool) -> f32 {
    let w = (0.25f32 / 2.0f32) as f64;
    let bb = Aabb::new([at[0] - w, at[1], at[2] - w], [at[0] + w, at[1] + 0.25f32 as f64, at[2] + w]);
    if !Aabb::player(f).intersects(&bb.expand(4.0, 2.0, 4.0)) || !(dist_sq(at, f) < 16.0) {
        return 0.0;
    }
    splash_heal(at, f, direct)
}

/// The worst heal and its tick over the packets' patterns (all of them
/// with `jitter`, the plain one without).
fn worst(p: &[Up], reps: &[Rep], jitter: bool) -> (f32, i32) {
    let n = reps.len() as isize;
    let feet = |t: isize| reps[t.clamp(0, n - 1) as usize].feet;
    let f3 = 0.3f32 as f64;
    let mut w = (POTION_HEAL, 0);
    for l0 in 0..=(jitter as isize) {
        let mut res = (0.0f32, p.len() as i32);
        for (i, u) in p.iter().enumerate() {
            let k = i as isize + 1;
            let choices: &[isize] = if jitter { &[0, 1] } else { &[0] };
            let (mut all, mut low) = (true, POTION_HEAL);
            for &e in choices {
                let f = feet(k + l0 - e);
                let h = if k >= 5 && Aabb::player(f).expand(f3, f3, f3).calculate_intercept(u.pos, u.end).is_some() {
                    POTION_HEAL
                } else {
                    all = false;
                    if u.block {
                        splash(u.pos, f, false)
                    } else {
                        0.0
                    }
                };
                low = low.min(h);
            }
            if all {
                res = (POTION_HEAL, k as i32);
                break;
            }
            if u.block {
                res = (low, k as i32);
                break;
            }
        }
        if res.0 < w.0 || (res.0 == w.0 && res.1 > w.1) {
            w = res;
        }
    }
    w
}

/// The spread a plan has to stand: the middle, 2.5 and 5 deviations along
/// each axis, and the eight corners at 5.
const ZS: [[f64; 3]; 21] = {
    const D: f64 = 2.886751345948129;
    [
        [0.0, 0.0, 0.0],
        [5.0, 0.0, 0.0],
        [-5.0, 0.0, 0.0],
        [0.0, 5.0, 0.0],
        [0.0, -5.0, 0.0],
        [0.0, 0.0, 5.0],
        [0.0, 0.0, -5.0],
        [2.5, 0.0, 0.0],
        [-2.5, 0.0, 0.0],
        [0.0, 2.5, 0.0],
        [0.0, -2.5, 0.0],
        [0.0, 0.0, 2.5],
        [0.0, 0.0, -2.5],
        [D, D, D],
        [D, D, -D],
        [D, -D, D],
        [D, -D, -D],
        [-D, D, D],
        [-D, D, -D],
        [-D, -D, D],
        [-D, -D, -D],
    ]
};

/// The pitch a dy of mouse counts gives the client (`set_angles`, a count
/// 0.15 degrees).
pub fn pitch_of(pitch: f32, dy: i32) -> f32 {
    let p = (pitch as f64 - (-(dy as f32)) as f64 * 0.15) as f32;
    p.clamp(-90.0, 90.0)
}

fn counts(deg: f32) -> i32 {
    (deg / 0.15f32 + 0.5).floor() as i32
}

/// The plan for a potion thrown with a turn on the tick to come, `inp`
/// being that tick's input (the turn in its mouse), `own` the policy's
/// movement keys on it, `carry` what the agent knows of the server adding
/// its movement (-1 not yet).
pub fn plan(c: &Client, world: &World, inp: &Input, own: PKey, carry: i8) -> Option<HealPlan> {
    let lp = &c.lp;
    let yaw1 = (lp.l.yaw as f64 + inp.dx as f64 * 0.15) as f32;
    let ways: &[bool] = match carry {
        0 => &[false],
        1 => &[true],
        _ => &[false, true],
    };
    // The ways to go on. The brake: the keys most against its motion.
    let m = lp.l.body.motion;
    let sp = (m[0] * m[0] + m[2] * m[2]).sqrt();
    let brake = if sp > 0.02 {
        let (fw, st) = crate::agent::walk_along(yaw1, [-m[0] / sp, -m[2] / sp]);
        Some(PKey { fw, st, sprint: false, jump: false })
    } else {
        None
    };
    let none = PKey::default();
    let own = PKey { jump: false, ..own };
    let mut progs: Vec<(u8, [PKey; PLAN_TICKS])> = Vec::new();
    for j in [None, Some(0usize), Some(1), Some(2), Some(3), Some(4)] {
        let mut add = |class: u8, f: &dyn Fn(usize) -> PKey| {
            let mut keys = [none; PLAN_TICKS];
            for (t, k) in keys.iter_mut().enumerate() {
                *k = f(t);
                k.jump = j == Some(t);
            }
            progs.push((class, keys));
        };
        add(0, &|_| own);
        if own != none {
            add(1, &|_| none);
        }
        if let Some(b) = brake {
            for nb in [1usize, 2, 3] {
                add(2, &|t| if t < nb { b } else { none });
            }
        }
    }
    let mut best: Option<(HealPlan, (i32, u8, i32))> = None;
    let mut buf = Vec::with_capacity(16);
    // Nominal paths for each distinct place and way of the throw.
    let mut cache: Vec<(V3, V3, bool, Vec<Vec<Vec<Up>>>)> = Vec::new();
    for (class, keys) in &progs {
        let reps = predict(c, world, inp, keys);
        let r0 = reps[0];
        let at = cache.iter().position(|e| e.0 == r0.feet && e.1 == r0.moved && e.2 == r0.on_ground);
        let at = match at {
            Some(i) => i,
            None => {
                let mut all = Vec::with_capacity(181);
                for d in -90..=90 {
                    let pitch = pitch_of(lp.l.pitch, counts(d as f32 - lp.l.pitch));
                    let mut per = Vec::with_capacity(ways.len());
                    for &w in ways {
                        let (from, m0) = throw(&r0, pitch, [0.0; 3], w);
                        path(world, from, m0, &mut buf);
                        per.push(buf.clone());
                    }
                    all.push(per);
                }
                cache.push((r0.feet, r0.moved, r0.on_ground, all));
                cache.len() - 1
            }
        };
        let nominal = &cache[at].3;
        // Windows of whole degrees where the nominal throw gives eight.
        let ok: Vec<bool> = nominal.iter().map(|per| per.iter().all(|p| worst(p, &reps, false).0 >= POTION_HEAL)).collect();
        let mut i = 0;
        while i < ok.len() {
            if !ok[i] {
                i += 1;
                continue;
            }
            let mut e = i;
            while e + 1 < ok.len() && ok[e + 1] {
                e += 1;
            }
            let width = (e - i + 1) as i32;
            // The middle of the window, then a little to either side.
            let mid = (i + e) as f32 / 2.0 - 90.0;
            for off in [0.0f32, -0.25, 0.25] {
                let deg = mid + off * width as f32;
                let dy = counts(deg - lp.l.pitch);
                let pitch = pitch_of(lp.l.pitch, dy);
                let mut low = (POTION_HEAL, 0i32);
                'z: for z in ZS.iter() {
                    for &w in ways {
                        let (from, m0) = throw(&r0, pitch, *z, w);
                        path(world, from, m0, &mut buf);
                        let r = worst(&buf, &reps, true);
                        if r.0 < low.0 || (r.0 == low.0 && r.1 > low.1) {
                            low = r;
                        }
                        if low.0 < POTION_HEAL {
                            break 'z;
                        }
                    }
                }
                if low.0 >= POTION_HEAL && low.1 <= MAX_HEAL_TICK {
                    let score = (low.1, *class, -width);
                    if best.as_ref().map_or(true, |b| score < b.1) {
                        let len = ((low.1 + 2) as usize).min(PLAN_TICKS);
                        let jump = keys.iter().position(|k| k.jump).map_or(-1, |t| t as i8);
                        best = Some((HealPlan { dy, yaw: yaw1, keys: *keys, len, ticks: low.1, class: *class, window: width, pitch, jump }, score));
                    }
                    break;
                }
            }
            i = e + 1;
        }
    }
    best.map(|b| b.0)
}

/// The keys of plan tick `t` for a view now at `yaw`: the plan's, turned
/// with the view when the policy has turned it (no sprint unless that is
/// still forward).
pub fn keys_now(p: &HealPlan, t: usize, yaw: f32) -> PKey {
    let k = p.keys[t.min(PLAN_TICKS - 1)];
    if (k.fw == 0 && k.st == 0) || mth::wrap_deg(yaw as f64 - p.yaw as f64).abs() < 1.0 {
        return k;
    }
    // The direction the plan's keys move it in, for its own view.
    let f = p.yaw * mth::PI_F / 180.0f32;
    let (sn, cs) = (mth::sin(f) as f64, mth::cos(f) as f64);
    let (fw, st) = (k.fw as f64, k.st as f64);
    let (x, z) = (st * cs - fw * sn, fw * cs + st * sn);
    let l = (x * x + z * z).sqrt();
    let (nf, ns) = crate::agent::walk_along(yaw, [x / l, z / l]);
    PKey { fw: nf, st: ns, sprint: k.sprint && nf == 1, jump: k.jump }
}
