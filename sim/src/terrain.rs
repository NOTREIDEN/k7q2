//! Terrain drawn for a duel: what the maps of duel servers have on and in
//! their floors, made of full blocks and slabs. Steps and slab stairs to
//! walk or jump onto, pillars and walls to get around, platforms, houses
//! with doors, pits to fall into and drops into the void. And cobwebs,
//! as players place them in a fight.
use crate::aabb::V3;
use crate::mth::{floor_d, Rng};
use crate::world::{Terrain, World, AIR, FULL, SLAB, WEB};

/// A whole number from `lo` to `hi`, both included.
fn int(rng: &mut Rng, lo: i32, hi: i32) -> i32 {
    lo + (rng.next_u64() % (hi.max(lo) - lo + 1) as u64) as i32
}

/// Fills a box of blocks, `[x0, x1) × [y0, y1) × [z0, z1)`.
fn fill(t: &mut Terrain, x: [i32; 2], y: [i32; 2], z: [i32; 2], b: u8) {
    for i in x[0]..x[1] {
        for k in z[0]..z[1] {
            for j in y[0]..y[1] {
                t.set(i, j, k, b);
            }
        }
    }
}

/// Puts the floor back in a column: solid under it, air over it.
fn restore(t: &mut Terrain, x: i32, z: i32, floor: i32) {
    t.fill_floor(x, z, floor);
}

/// Draws terrain for the arena of `world` and puts it in. `amount` from 0
/// to 1 says how much of it there is; the ground around each point of
/// `keep` (where the players start) is left flat and open. With `gap`, a
/// strip open to the void also lies across the way between the first two
/// points, so that the straight way from one player to the other drops
/// into it and the way round is a few blocks off.
pub fn build(world: &mut World, rng: &mut Rng, keep: &[V3], amount: f64, gap: bool) {
    let mut t = world.blank_terrain();
    let f = world.floor_y.round() as i32;
    let (lo_x, hi_x, lo_z, hi_z) = (t.x0, t.x0 + t.nx, t.z0, t.z0 + t.nz);
    let n = 3 + (amount.clamp(0.0, 1.0) * rng.range(4.0, 22.0)) as i32;
    for _ in 0..n {
        let cx = int(rng, lo_x, hi_x - 1);
        let cz = int(rng, lo_z, hi_z - 1);
        let along_x = rng.chance(0.5);
        let r = rng.f64();
        if r < 0.16 {
            // A step of one block: a single block or a short row.
            let len = int(rng, 1, 3);
            let (w, d) = if along_x { (len, 1) } else { (1, len) };
            fill(&mut t, [cx, cx + w], [f, f + 1], [cz, cz + d], FULL);
        } else if r < 0.30 {
            // Slabs: a patch to walk onto, sometimes the first step of a
            // stair up onto a block.
            let (w, d) = (int(rng, 1, 3), int(rng, 1, 3));
            fill(&mut t, [cx, cx + w], [f, f + 1], [cz, cz + d], SLAB);
            if rng.chance(0.4) {
                let (bx, bz) = if along_x { (cx + w, cz) } else { (cx, cz + d) };
                let (bw, bd) = if along_x { (int(rng, 1, 3), d) } else { (w, int(rng, 1, 3)) };
                fill(&mut t, [bx, bx + bw], [f, f + 1], [bz, bz + bd], FULL);
                if rng.chance(0.5) {
                    // ...and on up to a second level.
                    let (sx, sz) = if along_x { (bx + bw, bz) } else { (bx, bz + bd) };
                    fill(&mut t, [sx, sx + bw.max(1)], [f, f + 1], [sz, sz + bd.max(1)], FULL);
                    fill(&mut t, [sx, sx + bw.max(1)], [f + 1, f + 2], [sz, sz + bd.max(1)], SLAB);
                }
            }
        } else if r < 0.44 {
            // A pillar.
            let w = int(rng, 1, 2);
            let h = int(rng, 2, 5);
            fill(&mut t, [cx, cx + w], [f, f + h], [cz, cz + w], FULL);
        } else if r < 0.58 {
            // A wall: low enough to jump or too high to.
            let len = int(rng, 3, 9);
            let h = int(rng, 1, 3);
            let (w, d) = if along_x { (len, 1) } else { (1, len) };
            fill(&mut t, [cx, cx + w], [f, f + h], [cz, cz + d], FULL);
        } else if r < 0.68 {
            // A platform one block high, with slabs along one side now and
            // then.
            let (w, d) = (int(rng, 2, 6), int(rng, 2, 6));
            fill(&mut t, [cx, cx + w], [f, f + 1], [cz, cz + d], FULL);
            if rng.chance(0.4) {
                if along_x {
                    fill(&mut t, [cx - 1, cx], [f, f + 1], [cz, cz + d], SLAB);
                } else {
                    fill(&mut t, [cx, cx + w], [f, f + 1], [cz - 1, cz], SLAB);
                }
            }
            if rng.chance(0.25) {
                // A second level on top.
                let (w2, d2) = ((w - 2).max(1), (d - 2).max(1));
                fill(&mut t, [cx + 1, cx + 1 + w2], [f + 1, f + 2], [cz + 1, cz + 1 + d2], FULL);
            }
        } else if r < 0.76 {
            // A house: walls three high around a room, with doors.
            let (w, d) = (int(rng, 4, 7), int(rng, 4, 7));
            let h = int(rng, 2, 3);
            fill(&mut t, [cx, cx + w], [f, f + h], [cz, cz + d], FULL);
            fill(&mut t, [cx + 1, cx + w - 1], [f, f + h], [cz + 1, cz + d - 1], AIR);
            for _ in 0..int(rng, 1, 3) {
                let gap = int(rng, 1, 2);
                match int(rng, 0, 3) {
                    0 => {
                        let at = int(rng, cx + 1, cx + w - 1 - gap);
                        fill(&mut t, [at, at + gap], [f, f + h], [cz, cz + 1], AIR);
                    }
                    1 => {
                        let at = int(rng, cx + 1, cx + w - 1 - gap);
                        fill(&mut t, [at, at + gap], [f, f + h], [cz + d - 1, cz + d], AIR);
                    }
                    2 => {
                        let at = int(rng, cz + 1, cz + d - 1 - gap);
                        fill(&mut t, [cx, cx + 1], [f, f + h], [at, at + gap], AIR);
                    }
                    _ => {
                        let at = int(rng, cz + 1, cz + d - 1 - gap);
                        fill(&mut t, [cx + w - 1, cx + w], [f, f + h], [at, at + gap], AIR);
                    }
                }
            }
        } else if r < 0.90 {
            // A pit: one deep to step out of, deeper to be stuck in.
            let (w, d) = (int(rng, 1, 4), int(rng, 1, 4));
            let depth = if rng.chance(0.4) { 1 } else { int(rng, 2, 4) };
            fill(&mut t, [cx, cx + w], [f - depth, f], [cz, cz + d], AIR);
        } else {
            // A drop into the void: a strip or a hole with no bottom.
            let len = int(rng, 2, 8);
            let wid = int(rng, 1, 3);
            let (w, d) = if along_x { (len, wid) } else { (wid, len) };
            for i in cx..cx + w {
                for k in cz..cz + d {
                    t.dig_void(i, k);
                }
            }
        }
    }
    if gap && keep.len() >= 2 {
        // Across the middle of the way, square to it, two to six blocks
        // either side of it and one to three wide.
        let (a, b) = (keep[0], keep[1]);
        let (dx, dz) = (b[0] - a[0], b[2] - a[2]);
        let len = (dx * dx + dz * dz).sqrt().max(1e-6);
        let (ux, uz) = (dx / len, dz / len);
        let at = rng.range(0.35, 0.65);
        let (mx, mz) = (a[0] + dx * at, a[2] + dz * at);
        let (left, right) = (rng.range(2.0, 6.0), rng.range(2.0, 6.0));
        let wide = rng.range(1.0, 3.0);
        let mut s = -left;
        while s <= right {
            let mut w = -wide / 2.0;
            while w <= wide / 2.0 {
                // Along the strip is square to the way: (-uz, ux).
                let x = mx - uz * s + ux * w;
                let z = mz + ux * s + uz * w;
                t.dig_void(floor_d(x), floor_d(z));
                w += 0.5;
            }
            s += 0.5;
        }
    }
    for p in keep {
        let (px, pz) = (floor_d(p[0]), floor_d(p[2]));
        for i in px - 1..=px + 1 {
            for k in pz - 1..=pz + 1 {
                restore(&mut t, i, k, f);
            }
        }
    }
    world.set_terrain(t);
}

/// Puts cobwebs into the arena of `world`, on whatever terrain it has:
/// `amount` from 0 to 1 says how many patches (3 to 12 at the most), each
/// one to three blocks across on the ground, two high in some. With
/// `trap`, one patch is round the feet of the first point of `keep` (where
/// the first player starts), as one a player is caught in.
pub fn add_webs(world: &mut World, rng: &mut Rng, keep: &[V3], amount: f64, trap: bool) {
    let mut t = world.terrain().cloned().unwrap_or_else(|| world.blank_terrain());
    let (lo_x, hi_x, lo_z, hi_z) = (t.x0, t.x0 + t.nx, t.z0, t.z0 + t.nz);
    // The first block over the ground in a column, from the top of the box down.
    let ground = |t: &Terrain, x: i32, z: i32| -> Option<i32> {
        let top = t.y0 + t.ny - 1;
        let mut y = top;
        while y > t.y0 {
            if t.get(x, y, z) == Some(AIR) && matches!(t.get(x, y - 1, z), Some(FULL) | Some(SLAB)) {
                return Some(y);
            }
            y -= 1;
        }
        None
    };
    let patch = |t: &mut Terrain, cx: i32, cz: i32, w: i32, d: i32, two: bool| {
        for x in cx..cx + w {
            for z in cz..cz + d {
                if let Some(y) = ground(t, x, z) {
                    t.set(x, y, z, WEB);
                    if two && t.get(x, y + 1, z) == Some(AIR) {
                        t.set(x, y + 1, z, WEB);
                    }
                }
            }
        }
    };
    let n = (amount.clamp(0.0, 1.0) * rng.range(3.0, 12.0)).round() as i32;
    for _ in 0..n {
        let (w, d) = (int(rng, 1, 3), int(rng, 1, 3));
        let cx = int(rng, lo_x, hi_x - w);
        let cz = int(rng, lo_z, hi_z - d);
        let two = rng.chance(0.4);
        patch(&mut t, cx, cz, w, d, two);
    }
    if trap && !keep.is_empty() {
        // Round the feet: the block it stands in, and some of those next to it.
        let (px, pz) = (floor_d(keep[0][0]), floor_d(keep[0][2]));
        let two = rng.chance(0.5);
        patch(&mut t, px, pz, 1, 1, two);
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            if rng.chance(0.5) {
                patch(&mut t, px + dx, pz + dz, 1, 1, two);
            }
        }
    }
    world.set_terrain(t);
}
