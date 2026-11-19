//! A duel: two clients and one server, each ticking every 50 ms on its own
//! clock, joined by links with latency. Times are in microseconds.
use crate::aabb::V3;
use crate::client::{Client, ClientCfg, Input, TickOut};
use crate::mth::Rng;
use crate::net::{Link, C2S, S2C};
use crate::server::{KbProfile, Outbox, Server, ServerCfg, ServerPlayer};
use crate::world::World;

pub const TICK_US: i64 = 50_000;

#[derive(Clone, Copy, Debug)]
pub struct NetCfg {
    /// One-way delay client to server, per client.
    pub up_us: [i64; 2],
    /// One-way delay server to client, per client.
    pub down_us: [i64; 2],
    /// Extra uniform random delay per packet, per client.
    pub jitter_us: [i64; 2],
}

#[derive(Clone, Copy, Debug)]
pub struct Spawn {
    pub pos: V3,
    pub yaw: f32,
    pub pitch: f32,
}

/// A client that cheats with a timer (speed hack, "timer range"): in
/// bursts its game clock runs faster than 20 ticks a second, so that it
/// moves, swings and sends its movement packets that much more often. A
/// 1.8 server without an anticheat takes every movement packet that does
/// not move the player more than ten blocks past its motion or into a
/// block, and runs the player's own update once for each (item use,
/// effects, food, hurt animation), so the extra ticks count in full; only
/// the world tick (hurt resistance, pearl cooldown) keeps its pace. A
/// burst starts while the other player is within `range` blocks of it, as
/// it sees them. A blink is a short burst of a far faster timer: the client
/// covers a second's way in a few server ticks and is suddenly in the
/// other player's face.
#[derive(Clone, Copy, Debug, Default)]
pub struct Timer {
    pub on: bool,
    /// Timer of a burst (ticks per tick of the game), its length in
    /// microseconds of the client's clock, the pause after one, and the
    /// share of bursts that are blinks.
    pub rate: [f64; 2],
    pub burst_us: [i64; 2],
    pub gap_us: [i64; 2],
    pub blink: f64,
    pub range: [f64; 2],
    /// The burst under way: its timer, until when it runs, and the
    /// earliest start of the next one.
    pub cur: f64,
    pub until: i64,
    pub next: i64,
    /// Ticks run in bursts, and bursts started.
    pub ticks: u32,
    pub bursts: u32,
}

/// Timer and length of a blink.
pub const BLINK_RATE: [f64; 2] = [3.0, 6.0];
pub const BLINK_US: [i64; 2] = [100_000, 250_000];

#[derive(Clone, Debug)]
pub struct Match {
    pub world: World,
    pub c: [Client; 2],
    pub s: Server,
    pub c2s: [Link<C2S>; 2],
    pub s2c: [Link<S2C>; 2],
    pub next_client: [i64; 2],
    pub next_server: i64,
    pub net: NetCfg,
    pub ccfg: ClientCfg,
    pub rng: Rng,
    pub client_ticks: [u32; 2],
    pub server_ticks: u32,
    /// A timer per client (`Timer`); off for the plain game.
    pub timer: [Timer; 2],
    out_c: Vec<C2S>,
    out_s: Outbox,
}

impl Match {
    /// Both players standing still on the floor, already tracked by each
    /// other. `phase` holds the first tick time of client 0, client 1 and
    /// the server.
    pub fn new(
        world: World,
        spawns: [Spawn; 2],
        speed_amp: i8,
        kb: KbProfile,
        scfg: ServerCfg,
        ccfg: ClientCfg,
        net: NetCfg,
        phase: [i64; 3],
        seed: u64,
    ) -> Match {
        let mk_client = |me: usize| {
            let o = 1 - me;
            Client::new(spawns[me].pos, spawns[me].yaw, spawns[me].pitch, speed_amp, spawns[o].pos, spawns[o].yaw, spawns[o].pitch)
        };
        let mut rng = Rng::new(seed);
        let mut sp = [
            ServerPlayer::new(spawns[0].pos, spawns[0].yaw, spawns[0].pitch, speed_amp),
            ServerPlayer::new(spawns[1].pos, spawns[1].yaw, spawns[1].pitch, speed_amp),
        ];
        // The two tracker entries are created at different times in a real
        // session, so their counters are unrelated.
        for p in sp.iter_mut() {
            p.update_counter = 1 + (rng.next_u64() % 1200) as i32;
            p.ticks_since_teleport = (rng.next_u64() % 400) as i32;
        }
        let [p0, p1] = sp;
        Match {
            world,
            c: [mk_client(0), mk_client(1)],
            s: Server::new(p0, p1, kb, scfg),
            c2s: [Link::new(), Link::new()],
            s2c: [Link::new(), Link::new()],
            next_client: [phase[0], phase[1]],
            next_server: phase[2],
            net,
            ccfg,
            rng,
            client_ticks: [0; 2],
            server_ticks: 0,
            timer: [Timer::default(); 2],
            out_c: Vec::with_capacity(8),
            out_s: Vec::with_capacity(16),
        }
    }

    #[inline]
    fn jitter(rng: &mut Rng, j: i64) -> i64 {
        if j > 0 {
            (rng.next_u64() % (j as u64 + 1)) as i64
        } else {
            0
        }
    }

    /// Run server ticks until a client is due, hand that client the packets
    /// that have reached it, and return its index. The client has not
    /// ticked yet; call `client_tick` with its input.
    pub fn advance(&mut self) -> usize {
        self.advance_seen(|_, _, _, _| {})
    }

    /// `advance`, with every packet a client is handed shown to `seen`
    /// together with that client, once before the client handles it
    /// (false) and once after (true).
    #[inline]
    pub fn advance_seen(&mut self, mut seen: impl FnMut(usize, &S2C, &Client, bool)) -> usize {
        loop {
            let id = if self.next_client[1] < self.next_client[0] { 1 } else { 0 };
            if self.next_server <= self.next_client[id] {
                self.server_tick();
                continue;
            }
            let t = self.next_client[id];
            while let Some(at) = self.s2c[id].peek_time().filter(|&a| a <= t) {
                let p = self.s2c[id].pop_ready(t).unwrap();
                self.out_c.clear();
                seen(id, &p, &self.c[id], false);
                self.c[id].handle(&self.world, p, &mut self.out_c);
                seen(id, &p, &self.c[id], true);
                // A handler's own packets leave when the packet is handled,
                // which is the frame it arrived in, not the next tick.
                for k in 0..self.out_c.len() {
                    let d = self.net.up_us[id] + Self::jitter(&mut self.rng, self.net.jitter_us[id]);
                    self.c2s[id].push(at + d, self.out_c[k]);
                }
            }
            return id;
        }
    }

    pub fn client_tick(&mut self, id: usize, inp: &Input) -> TickOut {
        let t = self.next_client[id];
        self.out_c.clear();
        let r = self.c[id].tick(&self.world, inp, &self.ccfg, &mut self.out_c);
        for k in 0..self.out_c.len() {
            let d = self.net.up_us[id] + Self::jitter(&mut self.rng, self.net.jitter_us[id]);
            self.c2s[id].push(t + d, self.out_c[k]);
        }
        let period = if self.timer[id].on { self.timer_period(id, t) } else { TICK_US };
        self.next_client[id] = t + period;
        self.client_ticks[id] += 1;
        r
    }

    /// The time from this tick of a client with a timer, at `t` on its
    /// clock, to its next: a burst is started or kept up here.
    fn timer_period(&mut self, id: usize, t: i64) -> i64 {
        let rng = &mut self.rng;
        let tm = &mut self.timer[id];
        if t >= tm.until && t >= tm.next {
            let (a, b) = (self.c[id].lp.l.body.pos, self.c[id].rp.pos);
            let d = (a[0] - b[0]).hypot(a[2] - b[2]);
            if d >= tm.range[0] && d <= tm.range[1] && rng.chance(0.25) {
                let pick = |r: &mut Rng, lo: f64, hi: f64| r.range(lo, hi.max(lo));
                let len = if rng.chance(tm.blink) {
                    tm.cur = pick(rng, BLINK_RATE[0], BLINK_RATE[1]);
                    pick(rng, BLINK_US[0] as f64, BLINK_US[1] as f64)
                } else {
                    tm.cur = pick(rng, tm.rate[0], tm.rate[1]);
                    pick(rng, tm.burst_us[0] as f64, tm.burst_us[1] as f64)
                };
                tm.until = t + len as i64;
                tm.next = tm.until + pick(rng, tm.gap_us[0] as f64, tm.gap_us[1] as f64) as i64;
                tm.bursts += 1;
            }
        }
        if t < tm.until && tm.cur > 1.0 {
            tm.ticks += 1;
            ((TICK_US as f64 / tm.cur).round() as i64).max(1)
        } else {
            TICK_US
        }
    }

    pub fn server_tick(&mut self) {
        let t = self.next_server;
        self.out_s.clear();
        // Packets from both connections are handled in arrival order.
        loop {
            let a0 = self.c2s[0].peek_time().filter(|&a| a <= t);
            let a1 = self.c2s[1].peek_time().filter(|&a| a <= t);
            let from = match (a0, a1) {
                (None, None) => break,
                (Some(_), None) => 0,
                (None, Some(_)) => 1,
                (Some(x), Some(y)) => {
                    if y < x {
                        1
                    } else {
                        0
                    }
                }
            };
            let p = self.c2s[from].pop_ready(t).unwrap();
            self.s.handle(from, p, &self.world, &mut self.rng, &mut self.out_s);
        }
        self.s.tick(&self.world, &mut self.rng, &mut self.out_s);
        for k in 0..self.out_s.len() {
            let (dest, p) = self.out_s[k];
            let d = self.net.down_us[dest] + Self::jitter(&mut self.rng, self.net.jitter_us[dest]);
            self.s2c[dest].push(t + d, p);
        }
        self.next_server = t + TICK_US;
        self.server_ticks += 1;
    }
}
