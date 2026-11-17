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
        self.next_client[id] = t + TICK_US;
        self.client_ticks[id] += 1;
        r
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
