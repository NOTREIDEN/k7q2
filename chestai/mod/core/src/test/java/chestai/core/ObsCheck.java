package chestai.core;

import java.io.BufferedReader;
import java.io.FileReader;
import java.io.IOException;

/**
 * Builds the observation of every tick of a dump of the simulator
 * (chestsim/examples/dump.rs) from what the client saw in it and compares
 * it with the one the simulator built.
 *
 *     java chestai.core.ObsCheck dump.txt
 */
public final class ObsCheck {
    static final class Cells implements Grid {
        final int sx, sy, sz;
        final byte[] c;

        Cells(int sx, int sy, int sz, String hex) {
            this.sx = sx;
            this.sy = sy;
            this.sz = sz;
            c = new byte[sx * sy * sz];
            for (int i = 0; i < c.length; i++) {
                c[i] = (byte) Character.digit(hex.charAt(i), 16);
            }
        }

        int get(int x, int y, int z) {
            if (x < 0 || y < 0 || z < 0 || x >= sx || y >= sy || z >= sz) {
                return y >= sy && x >= 0 && z >= 0 && x < sx && z < sz ? 0 : 1;
            }
            return c[(y * sz + z) * sx + x];
        }

        public boolean solid(int x, int y, int z) {
            int v = get(x, y, z);
            return v == 1 || v == 3;
        }

        public boolean water(int x, int y, int z) {
            return get(x, y, z) == 2;
        }
    }

    public static void main(String[] a) throws IOException {
        BufferedReader r = new BufferedReader(new FileReader(a[0]));
        String line;
        Cells grid = null;
        Memory mem = null;
        Obs obs = new Obs();
        boolean first = true;
        int ticks = 0, bad = 0;
        double worst = 0.0;
        int worstAt = -1;
        String[] names = names();
        while ((line = r.readLine()) != null) {
            String[] t = line.split(" ");
            if (t[0].equals("W")) {
                grid = new Cells(Integer.parseInt(t[1]), Integer.parseInt(t[2]), Integer.parseInt(t[3]), t[4]);
                continue;
            }
            if (t[0].equals("C")) {
                mem = new Memory();
                int n = Integer.parseInt(t[1]);
                for (int i = 0; i < n; i++) {
                    mem.chest(Integer.parseInt(t[2 + 3 * i]), Integer.parseInt(t[3 + 3 * i]), Integer.parseInt(t[4 + 3 * i]));
                }
                first = true;
                continue;
            }
            int[] p = {1};
            Frame f = new Frame();
            f.tick = (long) d(t, p);
            f.x = d(t, p);
            f.y = d(t, p);
            f.z = d(t, p);
            f.mx = d(t, p);
            f.my = d(t, p);
            f.mz = d(t, p);
            f.yaw = (float) d(t, p);
            f.pitch = (float) d(t, p);
            f.onGround = d(t, p) != 0;
            f.collidedH = d(t, p) != 0;
            f.sprinting = d(t, p) != 0;
            f.hp = (float) d(t, p);
            f.absorption = (float) d(t, p);
            f.regenLeft = (int) d(t, p);
            f.slowLeft = (int) d(t, p);
            f.blindLeft = (int) d(t, p);
            f.gapples = (int) d(t, p);
            f.pearls = (int) d(t, p);
            f.potions = (int) d(t, p);
            f.arrows = (int) d(t, p);
            f.pearlCdLeft = (int) d(t, p);
            f.slot = (int) d(t, p);
            f.using = d(t, p) != 0;
            f.useTicks = (int) d(t, p);
            f.tokens = d(t, p);
            f.speedAmp = (int) d(t, p);
            f.pingMs = d(t, p);
            f.cps = d(t, p);
            f.punch = d(t, p);
            f.leashR = d(t, p);
            f.extreme = d(t, p) != 0;
            int sinceHurt = (int) d(t, p);
            f.pickMob = (int) d(t, p);
            f.pickChest = (int) d(t, p);
            int n = (int) d(t, p);
            for (int i = 0; i < n; i++) {
                f.rivals.add(new double[] {d(t, p), d(t, p), d(t, p)});
            }
            n = (int) d(t, p);
            for (int i = 0; i < n; i++) {
                Frame.Mob m = new Frame.Mob();
                m.id = (int) d(t, p);
                m.kind = (int) d(t, p);
                m.x = d(t, p);
                m.y = d(t, p);
                m.z = d(t, p);
                m.mx = d(t, p);
                m.my = d(t, p);
                m.mz = d(t, p);
                m.hp = (float) d(t, p);
                m.maxHp = (float) d(t, p);
                m.hurtTime = (int) d(t, p);
                m.mounted = d(t, p) != 0;
                m.hw = d(t, p);
                m.h = d(t, p);
                f.mobs.add(m);
            }
            n = (int) d(t, p);
            for (int i = 0; i < n; i++) {
                Frame.Holo h = new Frame.Holo();
                h.chest = (int) d(t, p);
                h.state = (int) d(t, p);
                h.secs = (int) d(t, p);
                f.holos.add(h);
            }
            n = (int) d(t, p);
            for (int i = 0; i < n; i++) {
                Frame.Proj q = new Frame.Proj();
                q.kind = (int) d(t, p);
                q.owner = (int) d(t, p);
                q.x = d(t, p);
                q.y = d(t, p);
                q.z = d(t, p);
                q.mx = d(t, p);
                q.my = d(t, p);
                q.mz = d(t, p);
                f.projs.add(q);
            }
            n = (int) d(t, p);
            float[] want = new float[n];
            for (int i = 0; i < n; i++) {
                want[i] = (float) d(t, p);
            }
            mem.update(f, first);
            first = false;
            if (mem.sinceHurt != sinceHurt) {
                System.out.println("tick " + ticks + ": since_hurt " + mem.sinceHurt + " here, " + sinceHurt + " there");
            }
            float[] got = obs.build(f, mem, grid);
            boolean off = false;
            for (int i = 0; i < n; i++) {
                double e = Math.abs(got[i] - want[i]);
                if (e > worst) {
                    worst = e;
                    worstAt = i;
                }
                if (e > 1e-4) {
                    if (!off && bad < 20) {
                        System.out.println("tick " + ticks + " value " + i + " (" + names[i] + "): " + got[i] + " here, " + want[i] + " there");
                    }
                    off = true;
                }
            }
            if (off) {
                bad++;
            }
            ticks++;
        }
        System.out.println(ticks + " ticks, " + bad + " with a value off by more than 1e-4; largest difference " + worst + " at " + (worstAt >= 0 ? names[worstAt] : "-"));
        if (bad > 0) {
            System.exit(1);
        }
    }

    static double d(String[] t, int[] p) {
        return Double.parseDouble(t[p[0]++]);
    }

    static String[] names() {
        String[] s = new String[Obs.DIM];
        int i = 0;
        for (int k = 0; k < Obs.SELF_DIM; k++) {
            s[i++] = "self." + k;
        }
        for (int k = 0; k < Obs.TERRAIN_DIM; k++) {
            s[i++] = "terrain." + k;
        }
        for (int m = 0; m < Obs.K_MOBS; m++) {
            for (int k = 0; k < Obs.MOB_DIM; k++) {
                s[i++] = "mob" + m + "." + k;
            }
        }
        for (int m = 0; m < Obs.K_CHESTS; m++) {
            for (int k = 0; k < Obs.CHEST_DIM; k++) {
                s[i++] = "chest" + m + "." + k;
            }
        }
        for (int m = 0; m < Obs.K_PROJ; m++) {
            for (int k = 0; k < Obs.PROJ_DIM; k++) {
                s[i++] = "proj" + m + "." + k;
            }
        }
        for (int k = 0; k < Obs.OWN_DIM; k++) {
            s[i++] = "own." + k;
        }
        return s;
    }
}
