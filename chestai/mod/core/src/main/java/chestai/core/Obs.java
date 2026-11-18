package chestai.core;

import java.util.ArrayList;
import java.util.Collections;
import java.util.Comparator;
import java.util.List;

/**
 * The observation of one tick, value for value as {@code Env.write_obs}
 * of the simulator builds it (chestsim/src/env.rs). Any change there
 * has to be made here too; ObsCheck compares the two.
 */
public final class Obs {
    public static final int K_MOBS = 8, K_CHESTS = 8, K_PROJ = 4, GRID = 4;
    public static final int SELF_DIM = 46, MOB_DIM = 25, CHEST_DIM = 28, PROJ_DIM = 9, OWN_DIM = 8;
    public static final int TERRAIN_DIM = 2 * (2 * GRID + 1) * (2 * GRID + 1);
    public static final int DIM = SELF_DIM + TERRAIN_DIM + K_MOBS * MOB_DIM + K_CHESTS * CHEST_DIM + K_PROJ * PROJ_DIM + OWN_DIM;
    public static final double EYE = 1.62f;
    public static final int WAVE_TICKS = 200;
    public static final double SPAWN_RADIUS = 12.0;
    public static final int MOB_KINDS = 4;

    private final float[] o = new float[DIM];
    private int w;

    private void put(double x) {
        o[w++] = (float) x;
    }

    /** Right, up, forward of a vector in the player's frame. */
    static double[] local(float yaw, double dx, double dy, double dz) {
        double y = (double) yaw * Math.PI / 180.0;
        double s = Math.sin(y), c = Math.cos(y);
        return new double[] {-dx * c - dz * s, dy, -dx * s + dz * c};
    }

    static double[] angleErr(float yaw, float pitch, double ex, double ey, double ez, double ax, double ay, double az) {
        double dx = ax - ex, dy = ay - ey, dz = az - ez;
        double wantYaw = Math.atan2(dz, dx) * 180.0 / Math.PI - 90.0;
        double e = (wantYaw - (double) yaw) % 360.0;
        if (e > 180.0) {
            e -= 360.0;
        }
        if (e < -180.0) {
            e += 360.0;
        }
        double h = Math.sqrt(dx * dx + dz * dz);
        double wantPitch = -(Math.atan2(dy, h) * 180.0 / Math.PI);
        return new double[] {e, wantPitch - (double) pitch};
    }

    private static double clamp(double v, double lo, double hi) {
        return v < lo ? lo : v > hi ? hi : v;
    }

    private static double b(boolean v) {
        return v ? 1.0 : 0.0;
    }

    private static double sq(double v) {
        return v * v;
    }

    /** Wave time left of a chest by memory: 1 for one not running. */
    private static double left(Memory.Known k, long now) {
        return k.seen && k.state == Frame.ACTIVE ? (double) (WAVE_TICKS - (now - k.activeSeenAt)) / WAVE_TICKS : 1.0;
    }

    /** The chests nearest first, at most K_CHESTS. */
    static List<Integer> chestOrder(Memory m, final double px, final double pz) {
        List<Integer> v = new ArrayList<Integer>();
        final double[] d = new double[m.chests.size()];
        for (int i = 0; i < m.chests.size(); i++) {
            Memory.Known k = m.chests.get(i);
            d[i] = Math.sqrt(sq(k.cx() - px) + sq(k.cz() - pz));
            v.add(i);
        }
        // A stable sort, as Rust's sort_by.
        Collections.sort(v, new Comparator<Integer>() {
            public int compare(Integer a, Integer b) {
                return Double.compare(d[a], d[b]);
            }
        });
        return v.size() > K_CHESTS ? v.subList(0, K_CHESTS) : v;
    }

    public float[] build(Frame f, Memory m, Grid g) {
        w = 0;
        final double px = f.x, py = f.y, pz = f.z;
        double ex = px, ey = py + EYE, ez = pz;
        float yaw = f.yaw;
        long now = f.tick;
        // Self.
        put(f.hp / 20.0);
        put(f.absorption / 4.0);
        put(Math.min(f.regenLeft / 100.0, 1.0));
        put(Math.min(f.slowLeft / 100.0, 1.5));
        put(Math.min(f.blindLeft / 100.0, 1.5));
        put(Math.min(f.gapples / 32.0, 2.0));
        put(Math.min(f.pearls / 16.0, 1.0));
        put(Math.min(f.potions / 8.0, 1.0));
        put(Math.min(f.arrows / 64.0, 1.0));
        put(Math.min(f.pearlCdLeft / 300.0, 1.0));
        double[] mv = local(yaw, f.mx, f.my, f.mz);
        put(mv[0] * 3.0);
        put(mv[1] * 3.0);
        put(mv[2] * 3.0);
        put(b(f.onGround));
        put(b(Ray.inWater(g, px, py, pz)));
        put(b(f.collidedH));
        put(f.pitch / 90.0);
        put(b(f.sprinting));
        for (int s = 0; s < 5; s++) {
            put(b(f.slot == s));
        }
        put(b(f.using));
        put(Math.min(f.useTicks / 32.0, 1.0));
        double fr = f.useTicks / 20.0;
        put(f.slot == 1 && f.using ? Math.min((fr * fr + 2.0 * fr) / 3.0, 1.0) : 0.0);
        put(f.tokens);
        put(f.speedAmp / 4.0);
        put(f.pingMs / 250.0);
        put(f.cps / 9.0);
        put(Math.min(m.sinceHurt / 20.0, 2.0));
        int mounted = 0;
        for (Frame.Mob mob : f.mobs) {
            if (mob.mounted) {
                mounted++;
            }
        }
        put(mounted);
        put(0.0);
        put(f.punch / 2.0);
        put(f.leashR / 6.0);
        put(b(f.extreme));
        double rd = 64.0;
        for (double[] r : f.rivals) {
            rd = Math.min(rd, Math.sqrt(sq(r[0] - px) + sq(r[2] - pz)));
        }
        put(rd / 64.0);
        for (int r = 0; r < 8; r++) {
            double a = (double) yaw + r * 45.0;
            double ar = a * Math.PI / 180.0;
            double dx = -Math.sin(ar), dz = Math.cos(ar);
            double[] from = {px, py + 0.6, pz};
            double[] to = {from[0] + dx * 8.0, from[1], from[2] + dz * 8.0};
            double[] h = Ray.hit(g, from, to);
            double d = h != null ? Math.sqrt(sq(h[0] - from[0]) + sq(h[2] - from[2])) : 8.0;
            put(d / 8.0);
        }
        put(1.0);

        // Terrain around the feet, turned with the view.
        int fy = Ray.floor(py);
        double yr = (double) yaw * Math.PI / 180.0;
        double s = Math.sin(yr), c = Math.cos(yr);
        for (int i = -GRID; i <= GRID; i++) {
            for (int j = -GRID; j <= GRID; j++) {
                double rx = i, fz = j;
                double x = px + (-rx * c - fz * s);
                double z = pz + (-rx * s + fz * c);
                int bx = Ray.floor(x), bz = Ray.floor(z);
                int h = -5;
                for (int y = fy + 4; y >= fy - 5; y--) {
                    if (g.solid(bx, y, bz)) {
                        h = y + 1 - fy;
                        break;
                    }
                }
                put(h / 5.0);
                put(b(g.water(bx, Math.max(fy, fy + h - 1), bz)) + b(g.water(bx, fy, bz)) * 0.5);
            }
        }

        // Mobs, nearest first.
        final int n = f.mobs.size();
        final double[] md = new double[n];
        List<Integer> mo = new ArrayList<Integer>();
        for (int i = 0; i < n; i++) {
            Frame.Mob mob = f.mobs.get(i);
            md[i] = Math.sqrt(sq(mob.x - px) + sq(mob.y - py) + sq(mob.z - pz));
            mo.add(i);
        }
        Collections.sort(mo, new Comparator<Integer>() {
            public int compare(Integer a, Integer b) {
                return Double.compare(md[a], md[b]);
            }
        });
        double oldest = 1.0;
        for (Memory.Known k : m.chests) {
            oldest = Math.min(oldest, left(k, now));
        }
        for (int k = 0; k < K_MOBS; k++) {
            if (k >= n) {
                for (int q = 0; q < MOB_DIM; q++) {
                    put(0.0);
                }
                continue;
            }
            int i = mo.get(k);
            Frame.Mob mob = f.mobs.get(i);
            double[] rel = local(yaw, mob.x - px, mob.y - py, mob.z - pz);
            put(1.0);
            put(rel[0] / 16.0);
            put(rel[1] / 16.0);
            put(rel[2] / 16.0);
            put(md[i] / 16.0);
            double[] lv = local(yaw, mob.mx, mob.my, mob.mz);
            put(lv[0] * 4.0);
            put(lv[1] * 4.0);
            put(lv[2] * 4.0);
            put((double) (mob.hp / Math.max(mob.maxHp, 1.0f)));
            put(mob.hp / 40.0);
            for (int kk = 0; kk < MOB_KINDS; kk++) {
                put(b(mob.kind == kk));
            }
            put(mob.hurtTime / 10.0);
            put(b(mob.mounted));
            double cxm = mob.x, cym = mob.y + mob.h * 0.5, czm = mob.z;
            double qx = clamp(ex, mob.x - mob.hw, mob.x + mob.hw);
            double qy = clamp(ey, mob.y, mob.y + mob.h);
            double qz = clamp(ez, mob.z - mob.hw, mob.z + mob.hw);
            double reach = Math.sqrt(sq(qx - ex) + sq(qy - ey) + sq(qz - ez));
            put(b(reach <= 3.0));
            double[] ae = angleErr(yaw, f.pitch, ex, ey, ez, cxm, cym, czm);
            put(ae[0] / 180.0);
            put(ae[1] / 90.0);
            put(b(f.pickMob == mob.id));
            double cd = 99.0;
            int cidx = 0;
            for (int ci = 0; ci < m.chests.size(); ci++) {
                Memory.Known ch = m.chests.get(ci);
                double dd = Math.sqrt(sq(ch.cx() - mob.x) + sq(ch.cz() - mob.z));
                if (dd < cd) {
                    cd = dd;
                    cidx = ci;
                }
            }
            put(cd / f.leashR);
            put(clamp((f.leashR - cd) / 3.0, -1.0, 2.0));
            double bx = mob.x - px, bz = mob.z - pz;
            double bl = Math.max(Math.sqrt(bx * bx + bz * bz), 1e-3);
            double[] back = {mob.x + bx / bl * (mob.hw + 0.6), mob.y + 0.5, mob.z + bz / bl * (mob.hw + 0.6)};
            put(b(g.solid(Ray.floor(back[0]), Ray.floor(back[1]), Ray.floor(back[2]))));
            if (m.chests.isEmpty()) {
                put(1.0);
                put(0.0);
            } else {
                Memory.Known kn = m.chests.get(cidx);
                double mine = left(kn, now);
                put(mine);
                put(b(kn.state == Frame.ACTIVE && mine <= oldest + 1e-9));
            }
        }

        // Chests, nearest first, from memory.
        List<Integer> order = chestOrder(m, px, pz);
        for (int k = 0; k < K_CHESTS; k++) {
            if (k >= order.size()) {
                for (int q = 0; q < CHEST_DIM; q++) {
                    put(0.0);
                }
                continue;
            }
            int ci = order.get(k);
            Memory.Known kn = m.chests.get(ci);
            double ccx = kn.cx(), ccy = kn.cy(), ccz = kn.cz();
            double[] rel = local(yaw, ccx - px, ccy - py, ccz - pz);
            double d = Math.sqrt(sq(ccx - px) + sq(ccy - py - 0.9) + sq(ccz - pz));
            put(1.0);
            put(rel[0] / 32.0);
            put(rel[1] / 32.0);
            put(rel[2] / 32.0);
            put(d / 32.0);
            for (int st = 0; st < 4; st++) {
                put(b(kn.seen && kn.state == st));
            }
            put(b(!kn.seen));
            put(kn.seen ? kn.secs / 180.0 : 0.0);
            double est = m.estSecs(ci, now);
            put(kn.seen ? est / 180.0 : 1.0);
            put(kn.seen ? Math.log(1.0 + est) / 5.2 : 1.0);
            put(kn.seen && kn.state == Frame.ACTIVE ? (double) (WAVE_TICKS - (now - kn.activeSeenAt)) / WAVE_TICKS : 0.0);
            int near = 0;
            for (Frame.Mob mob : f.mobs) {
                if (Math.sqrt(sq(mob.x - ccx) + sq(mob.z - ccz)) < f.leashR + 2.0) {
                    near++;
                }
            }
            put(near / 5.0);
            put(kn.seen ? Math.min((now - kn.tick) / 100.0, 3.0) : 3.0);
            put(b(d <= SPAWN_RADIUS));
            put(clamp((SPAWN_RADIUS - d) / SPAWN_RADIUS, -2.0, 1.0));
            double od = 999.0;
            int oi = ci;
            for (int cj = 0; cj < m.chests.size(); cj++) {
                if (cj == ci) {
                    continue;
                }
                Memory.Known c2 = m.chests.get(cj);
                double dd = Math.sqrt(sq(c2.cx() - ccx) + sq(c2.cy() - ccy) + sq(c2.cz() - ccz));
                if (dd < od) {
                    od = dd;
                    oi = cj;
                }
            }
            put(Math.min(od / 24.0, 2.0));
            if (oi != ci) {
                Memory.Known c2 = m.chests.get(oi);
                double[] rm = local(yaw, (ccx + c2.cx()) / 2.0 - px, (ccy + c2.cy()) / 2.0 - py, (ccz + c2.cz()) / 2.0 - pz);
                put(rm[0] / 32.0);
                put(rm[1] / 32.0);
                put(rm[2] / 32.0);
                put(b(od <= 2.0 * SPAWN_RADIUS - 1.0));
                put(c2.seen ? m.estSecs(oi, now) / 180.0 : 1.0);
            } else {
                for (int q = 0; q < 5; q++) {
                    put(0.0);
                }
            }
            put(b(f.pickChest == ci));
            int rivals = 0;
            for (double[] r : f.rivals) {
                if (Math.sqrt(sq(r[0] - ccx) + sq(r[2] - ccz)) <= SPAWN_RADIUS) {
                    rivals++;
                }
            }
            put(rivals / 2.0);
            double[] ae = angleErr(yaw, f.pitch, ex, ey, ez, ccx, ccy, ccz);
            put(ae[0] / 180.0);
            put(ae[1] / 90.0);
        }

        // Mob projectiles coming in, nearest first.
        final List<Frame.Proj> inc = new ArrayList<Frame.Proj>();
        for (Frame.Proj p : f.projs) {
            if (p.owner >= 0) {
                inc.add(p);
            }
        }
        final double[] pd = new double[inc.size()];
        List<Integer> po = new ArrayList<Integer>();
        for (int i = 0; i < inc.size(); i++) {
            Frame.Proj p = inc.get(i);
            pd[i] = Math.sqrt(sq(p.x - px) + sq(p.y - py) + sq(p.z - pz));
            po.add(i);
        }
        Collections.sort(po, new Comparator<Integer>() {
            public int compare(Integer a, Integer b) {
                return Double.compare(pd[a], pd[b]);
            }
        });
        for (int k = 0; k < K_PROJ; k++) {
            if (k >= po.size()) {
                for (int q = 0; q < PROJ_DIM; q++) {
                    put(0.0);
                }
                continue;
            }
            Frame.Proj p = inc.get(po.get(k));
            double rx = p.x - px, ry = p.y - (py + 0.9), rz = p.z - pz;
            double[] lr = local(yaw, rx, ry, rz);
            double[] lv = local(yaw, p.mx, p.my, p.mz);
            put(1.0);
            put(lr[0] / 16.0);
            put(lr[1] / 16.0);
            put(lr[2] / 16.0);
            put(lv[0]);
            put(lv[1]);
            put(lv[2]);
            double vv = p.mx * p.mx + p.my * p.my + p.mz * p.mz;
            double t = vv > 1e-6 ? Math.max(-(rx * p.mx + ry * p.my + rz * p.mz) / vv, 0.0) : 0.0;
            double qx = rx + p.mx * t, qy = ry + p.my * t, qz = rz + p.mz * t;
            put(Math.min(t / 20.0, 2.0));
            put(Math.min(Math.sqrt(qx * qx + qy * qy + qz * qz) / 4.0, 2.0));
        }
        // The player's own arrow in flight.
        Frame.Proj own = null;
        double best = 0.0;
        for (Frame.Proj p : f.projs) {
            if (p.owner == -1 && p.kind == Frame.ARROW) {
                double dd = sq(p.x - px) + sq(p.z - pz);
                if (own == null || dd < best) {
                    own = p;
                    best = dd;
                }
            }
        }
        if (own != null) {
            double[] lr = local(yaw, own.x - px, own.y - (py + 0.9), own.z - pz);
            double[] lv = local(yaw, own.mx, own.my, own.mz);
            put(1.0);
            put(lr[0] / 8.0);
            put(lr[1] / 8.0);
            put(lr[2] / 8.0);
            put(lv[0]);
            put(lv[1]);
            put(lv[2]);
            put(0.0);
        } else {
            for (int q = 0; q < OWN_DIM; q++) {
                put(0.0);
            }
        }
        if (w != DIM) {
            throw new IllegalStateException("observation of " + w + " values, not " + DIM);
        }
        return o.clone();
    }
}
