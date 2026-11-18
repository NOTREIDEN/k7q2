//! Maps that are never the same: hills and mountains from layered noise,
//! lakes, walls, pillars and steps, and chests spread in clusters over it.
use crate::mth::Rng;
use crate::world::{World, CHEST, SOLID, WATER};

#[derive(Clone, Copy, Debug)]
pub struct TerrainCfg {
    pub size: i32,
    pub height: i32,
    /// Height of the flat floor the hills rise from.
    pub base: i32,
    /// Largest height of the hills above the floor.
    pub relief: f64,
    /// Share of maps with lakes, and how high their water stands.
    pub water_prob: f64,
    /// Walls, pillars and steps per 1000 columns.
    pub clutter: f64,
    /// Chance that a chest has a wall or a step beside it (a place to pin
    /// mobs against).
    pub gank_spot_prob: f64,
    pub chests_min: usize,
    pub chests_max: usize,
    /// Chests come in groups; the distance between chests of one group.
    pub cluster_gap_min: f64,
    pub cluster_gap_max: f64,
    pub cluster_max: usize,
}

impl Default for TerrainCfg {
    fn default() -> Self {
        TerrainCfg {
            size: 96,
            height: 48,
            base: 6,
            relief: 14.0,
            water_prob: 0.5,
            clutter: 6.0,
            gank_spot_prob: 0.5,
            chests_min: 3,
            chests_max: 10,
            cluster_gap_min: 4.0,
            cluster_gap_max: 22.0,
            cluster_max: 4,
        }
    }
}

/// Value noise on a lattice of `cell` blocks, smoothly interpolated.
struct Noise {
    n: usize,
    v: Vec<f64>,
    cell: f64,
}

impl Noise {
    fn new(rng: &mut Rng, size: i32, cell: f64) -> Noise {
        let n = (size as f64 / cell).ceil() as usize + 2;
        Noise { n, v: (0..n * n).map(|_| rng.f64()).collect(), cell }
    }
    fn at(&self, x: f64, z: f64) -> f64 {
        let (fx, fz) = (x / self.cell, z / self.cell);
        let (ix, iz) = (fx.floor() as usize, fz.floor() as usize);
        let (tx, tz) = (fx - ix as f64, fz - iz as f64);
        let s = |t: f64| t * t * (3.0 - 2.0 * t);
        let (sx, sz) = (s(tx), s(tz));
        let g = |a: usize, b: usize| self.v[(b.min(self.n - 1)) * self.n + a.min(self.n - 1)];
        let a = g(ix, iz) * (1.0 - sx) + g(ix + 1, iz) * sx;
        let b = g(ix, iz + 1) * (1.0 - sx) + g(ix + 1, iz + 1) * sx;
        a * (1.0 - sz) + b * sz
    }
}

pub struct Map {
    pub world: World,
    /// Block coordinates of each chest.
    pub chests: Vec<[i32; 3]>,
    /// Where the player starts (feet).
    pub spawn: [f64; 3],
}

pub fn generate(cfg: &TerrainCfg, rng: &mut Rng) -> Map {
    let (sx, sy, sz) = (cfg.size, cfg.height, cfg.size);
    let mut w = World::new(sx, sy, sz);
    // Hills: three octaves; a sharpening power gives plains and peaks.
    let relief = cfg.relief * rng.range(0.0, 1.0);
    let c_n1 = rng.range(20.0, 40.0);
    let n1 = Noise::new(rng, sx, c_n1);
    let c_n2 = rng.range(8.0, 14.0);
    let n2 = Noise::new(rng, sx, c_n2);
    let c_n3 = rng.range(3.0, 5.0);
    let n3 = Noise::new(rng, sx, c_n3);
    let sharp = rng.range(1.0, 2.5);
    let terrace = rng.chance(0.4);
    let mut hgt = vec![0i32; (sx * sz) as usize];
    for x in 0..sx {
        for z in 0..sz {
            let (fx, fz) = (x as f64, z as f64);
            let v = 0.6 * n1.at(fx, fz) + 0.3 * n2.at(fx, fz) + 0.1 * n3.at(fx, fz);
            let mut h = v.powf(sharp) * relief;
            if terrace {
                h = (h / 2.0).floor() * 2.0;
            }
            let top = (cfg.base as f64 + h).round() as i32;
            hgt[(z * sx + x) as usize] = top.clamp(1, sy - 12);
            for y in 0..top.clamp(1, sy - 12) {
                w.set(x, y, z, SOLID);
            }
        }
    }
    // Lakes: water up to a level, in every column lower than it.
    if rng.chance(cfg.water_prob) {
        let level = cfg.base + 1 + (relief * rng.range(0.1, 0.4)).round() as i32;
        for x in 0..sx {
            for z in 0..sz {
                let top = hgt[(z * sx + x) as usize];
                for y in top..level {
                    w.set(x, y, z, WATER);
                }
            }
        }
    }
    // Clutter: walls two or three high, pillars, single steps.
    let n_clutter = (cfg.clutter * (sx * sz) as f64 / 1000.0 * rng.range(0.3, 1.5)) as usize;
    for _ in 0..n_clutter {
        let x = rng.range(2.0, (sx - 2) as f64) as i32;
        let z = rng.range(2.0, (sz - 2) as f64) as i32;
        place_feature(&mut w, rng, x, z);
    }
    // Chests in clusters.
    let n = cfg.chests_min + (rng.f64() * (cfg.chests_max - cfg.chests_min + 1) as f64) as usize;
    let n = n.min(cfg.chests_max.max(cfg.chests_min));
    let mut chests: Vec<[i32; 3]> = Vec::new();
    let margin = 6.0;
    let mut tries = 0;
    while chests.len() < n && tries < 2000 {
        tries += 1;
        // Most groups sit on a hilltop: the highest of a few places tried.
        let mut cx = rng.range(margin, sx as f64 - margin);
        let mut cz = rng.range(margin, sz as f64 - margin);
        if rng.chance(0.6) {
            let mut best = w.ground(cx as i32, cz as i32);
            for _ in 0..10 {
                let (x, z) = (rng.range(margin, sx as f64 - margin), rng.range(margin, sz as f64 - margin));
                let g = w.ground(x as i32, z as i32);
                if g > best && !w.water(x as i32, g, z as i32) {
                    best = g;
                    cx = x;
                    cz = z;
                }
            }
        }
        let k = 1 + (rng.f64() * cfg.cluster_max as f64) as usize;
        let mut last = (cx, cz);
        for _ in 0..k {
            if chests.len() >= n {
                break;
            }
            let (x, z) = if chests.is_empty() || rng.chance(0.3) && last == (cx, cz) {
                (cx, cz)
            } else {
                let a = rng.range(0.0, std::f64::consts::TAU);
                let d = rng.range(cfg.cluster_gap_min, cfg.cluster_gap_max);
                (last.0 + a.cos() * d, last.1 + a.sin() * d)
            };
            if x < margin || z < margin || x > sx as f64 - margin || z > sz as f64 - margin {
                continue;
            }
            let (bx, bz) = (x as i32, z as i32);
            let gy = w.ground(bx, bz);
            if w.water(bx, gy, bz) || gy >= sy - 8 {
                continue;
            }
            if chests.iter().any(|c| (c[0] - bx).pow(2) + (c[2] - bz).pow(2) < 9) {
                continue;
            }
            // Clear the air above, so the chest and its mobs have room.
            for dx in -2..=2 {
                for dz in -2..=2 {
                    for dy in 0..4 {
                        if w.solid(bx + dx, gy + dy, bz + dz) && (dx * dx + dz * dz <= 2 || dy > 0) {
                            w.set(bx + dx, gy + dy, bz + dz, 0);
                        }
                    }
                }
            }
            w.set(bx, gy, bz, CHEST);
            if rng.chance(cfg.gank_spot_prob) {
                // A wall or a step a few blocks off one side.
                let a = rng.range(0.0, std::f64::consts::TAU);
                let d = rng.range(2.0, 4.5);
                let (fx, fz) = (bx as f64 + a.cos() * d, bz as f64 + a.sin() * d);
                let along_x = a.sin().abs() > a.cos().abs();
                let len = rng.range(3.0, 7.0) as i32;
                let tall = if rng.chance(0.5) { 1 } else { 2 + (rng.f64() * 2.0) as i32 };
                for i in -(len / 2)..=(len / 2) {
                    let (wx, wz) = if along_x { (fx as i32 + i, fz as i32) } else { (fx as i32, fz as i32 + i) };
                    let g = w.ground(wx, wz);
                    for dy in 0..tall {
                        if w.get(wx, g + dy, wz) != CHEST {
                            w.set(wx, g + dy, wz, SOLID);
                        }
                    }
                }
            }
            chests.push([bx, gy, bz]);
            last = (x, z);
        }
    }
    if chests.is_empty() {
        // A map with nowhere dry enough to put a chest: try another.
        let mut c2 = *cfg;
        c2.water_prob = 0.0;
        return generate(&c2, rng);
    }
    // The player starts on dry ground somewhere near a chest.
    let mut spawn = [sx as f64 / 2.0, sy as f64, sz as f64 / 2.0];
    for _ in 0..200 {
        let c = chests[(rng.f64() * chests.len() as f64) as usize % chests.len().max(1)];
        let a = rng.range(0.0, std::f64::consts::TAU);
        let d = rng.range(8.0, 30.0);
        let x = (c[0] as f64 + a.cos() * d).clamp(2.0, sx as f64 - 3.0);
        let z = (c[2] as f64 + a.sin() * d).clamp(2.0, sz as f64 - 3.0);
        let (bx, bz) = (x as i32, z as i32);
        let gy = w.ground(bx, bz);
        if !w.water(bx, gy, bz) && !w.solid(bx, gy + 1, bz) && !w.solid(bx, gy + 2, bz) {
            spawn = [bx as f64 + 0.5, gy as f64, bz as f64 + 0.5];
            break;
        }
    }
    Map { world: w, chests, spawn }
}

fn place_feature(w: &mut World, rng: &mut Rng, x: i32, z: i32) {
    let g = w.ground(x, z);
    if w.water(x, g, z) {
        return;
    }
    match (rng.f64() * 3.0) as i32 {
        0 => {
            let len = rng.range(3.0, 9.0) as i32;
            let tall = 2 + (rng.f64() * 2.0) as i32;
            let along_x = rng.chance(0.5);
            for i in 0..len {
                let (wx, wz) = if along_x { (x + i, z) } else { (x, z + i) };
                let gg = w.ground(wx, wz);
                for dy in 0..tall {
                    w.set(wx, gg + dy, wz, SOLID);
                }
            }
        }
        1 => {
            let tall = 2 + (rng.f64() * 4.0) as i32;
            for dy in 0..tall {
                w.set(x, g + dy, z, SOLID);
            }
        }
        _ => {
            let r = 1 + (rng.f64() * 2.0) as i32;
            for dx in -r..=r {
                for dz in -r..=r {
                    let gg = w.ground(x + dx, z + dz);
                    if gg <= g {
                        w.set(x + dx, g, z + dz, SOLID);
                    }
                }
            }
        }
    }
}
