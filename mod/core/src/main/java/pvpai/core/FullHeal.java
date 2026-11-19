package pvpai.core;

import java.util.ArrayList;
import java.util.List;

/**
 * A healing potion thrown with a turn that gives all eight points: a copy,
 * line for line, of the simulator's (sim/src/fullheal.rs). On the tick of
 * the turn the agent works out, from its own client, a few ways to go on
 * (its own keys held, no key, a brake against its motion; each with a jump
 * on one of the first ticks or none) and, for each, the pitches from
 * straight up to straight down. A way and a pitch are taken when the
 * potion gives eight points in the worst case over the server's random
 * part of the throw, over the server adding the thrower's movement or not
 * (unless the agent knows which), and over every way the packets can be a
 * tick late. The keys of that way are then held until the potion is due,
 * laid over the policy's.
 */
public final strictfp class FullHeal {
    /** Ticks of keys a plan holds at most, from the turn's tick. */
    public static final int PLAN_TICKS = 12;
    /** The latest tick of the potion a plan may heal on. */
    private static final int MAX_HEAL_TICK = 9;
    private static final float POTION_HEAL = 8.0f;
    /** The deviation of the gaussian {@code setThrowableHeading} adds to each part of a potion's direction (0.0075 as a float). */
    private static final double THROW_SPREAD = 0.007499999832361937;

    /** Movement keys of one tick, for the view the plan was made for. */
    public static final class PKey {
        public final int fw, st;
        public final boolean sprint, jump;

        public PKey(int fw, int st, boolean sprint, boolean jump) {
            this.fw = fw;
            this.st = st;
            this.sprint = sprint;
            this.jump = jump;
        }

        PKey withJump(boolean j) {
            return new PKey(fw, st, sprint, j);
        }

        boolean same(PKey o) {
            return fw == o.fw && st == o.st && sprint == o.sprint && jump == o.jump;
        }
    }

    private static final PKey NONE = new PKey(0, 0, false, false);

    /** A plan: see fullheal.rs {@code HealPlan}. */
    public static final class Plan {
        /** Mouse counts down on the turn's tick, the yaw the keys are for, the keys from the turn's tick on and how many ticks they are held. */
        public int dy;
        public float yaw;
        public PKey[] keys;
        public int len;
        /** The potion's tick of the heal in the worst case, the class of the way (0 own keys, 1 no key, 2 brake), the window of pitch (degrees). */
        public int ticks, cls, window;
        /** The pitch of the throw and the tick of the jump (-1 none). */
        public float pitch;
        public int jump;
    }

    /** What the server has of the player after a client tick, and its box there grown by 0.3 (the potion's thrower check). */
    private static final class Rep {
        double[] feet;
        boolean onGround;
        double[] moved;
        float yaw;
        double[] bmin, bmax;
    }

    private static final double GROW = (double) 0.3f;

    /** One tick of a potion's path: where it starts, where it ends, and whether it ends on a block. */
    private static final class Up {
        final double[] pos, end;
        final boolean block;

        Up(double[] pos, double[] end, boolean block) {
            this.pos = pos;
            this.end = end;
            this.block = block;
        }
    }

    /**
     * Whether a plan is tried for this throw (fullheal.rs {@code gate_open}):
     * the last hurt at most smax ticks ago, or the other player at least
     * dmin blocks away. smax of 100 or more with dmin 0 opens it always,
     * smax below 0 shuts that part.
     */
    public static boolean gateOpen(int smax, int dmin, int ticksSinceHurt, double dist) {
        if (smax >= 100 && dmin <= 0) {
            return true;
        }
        return (smax >= 0 && smax < 100 && ticksSinceHurt <= smax) || (dmin > 0 && dist >= (double) dmin);
    }

    /** The client's own ticks with these keys, from a copy of it: the turn's tick has the mouse of the turn. */
    private static Rep[] predict(Walk w, Blocks blocks, int dx, PKey[] keys) {
        Walk cc = w.copy();
        double[] feet = cc.said.clone();
        float yaw = cc.saidYaw;
        Rep[] reps = new Rep[PLAN_TICKS + 2];
        for (int t = 0; t < PLAN_TICKS + 2; t++) {
            PKey k = keys[Math.min(t, PLAN_TICKS - 1)];
            cc.tick(blocks, k.fw, k.st, k.jump && t < PLAN_TICKS, k.sprint, t == 0 ? dx : 0, 0, 1.0f);
            double[] moved = {0.0, 0.0, 0.0};
            if (cc.sentPos) {
                moved = new double[] {cc.said[0] - feet[0], cc.said[1] - feet[1], cc.said[2] - feet[2]};
                feet = cc.said.clone();
            }
            if (cc.sentRot) {
                yaw = cc.saidYaw;
            }
            Rep r = new Rep();
            r.feet = feet;
            r.onGround = cc.sentOnGround;
            r.moved = moved;
            r.yaw = yaw % 360.0f;
            r.bmin = playerMin(feet, GROW);
            r.bmax = playerMax(feet, GROW);
            reps[t] = r;
        }
        return reps;
    }

    /** Where a potion thrown from r0 at this pitch starts and how it moves, z the deviations of the random part. */
    private static double[][] throwOf(Rep r0, float pitch, double[] z, boolean carry) {
        double[] h = Geo.throwHeading(Inv.KIND_POTION, r0.feet, r0.yaw, pitch % 360.0f);
        double v = (double) 0.5f;
        double[] m = new double[3];
        for (int a = 0; a < 3; a++) {
            m[a] = (h[3 + a] + z[a] * THROW_SPREAD) * v;
        }
        if (carry) {
            m[0] += r0.moved[0];
            m[2] += r0.moved[2];
            if (!r0.onGround) {
                m[1] += r0.moved[1];
            }
        }
        return new double[][] {{h[0], h[1], h[2]}, m};
    }

    private static List<Up> path(Blocks blocks, double[] pos, double[] m) {
        List<Up> out = new ArrayList<Up>(MAX_HEAL_TICK + 3);
        pos = pos.clone();
        m = m.clone();
        double f2 = (double) 0.99f, g = (double) 0.05f;
        for (int n = 0; n < MAX_HEAL_TICK + 3; n++) {
            double[] end = {pos[0] + m[0], pos[1] + m[1], pos[2] + m[2]};
            boolean block = false;
            double[] hit = blocks.rayTrace(pos, end);
            if (hit != null) {
                end = hit;
                block = true;
            }
            out.add(new Up(pos.clone(), end, block));
            if (block) {
                return out;
            }
            for (int a = 0; a < 3; a++) {
                pos[a] += m[a];
                m[a] *= f2;
            }
            m[1] -= g;
        }
        return out;
    }

    private static double[] playerMin(double[] f, double grow) {
        return new double[] {f[0] - Walk.HALF_WIDTH - grow, f[1] - grow, f[2] - Walk.HALF_WIDTH - grow};
    }

    private static double[] playerMax(double[] f, double grow) {
        return new double[] {f[0] + Walk.HALF_WIDTH + grow, f[1] + Walk.HEIGHT + grow, f[2] + Walk.HALF_WIDTH + grow};
    }

    private static float splash(double[] at, double[] f) {
        double w = (double) (0.25f / 2.0f);
        double[] lo = {at[0] - w - 4.0, at[1] - 2.0, at[2] - w - 4.0};
        double[] hi = {at[0] + w + 4.0, at[1] + (double) 0.25f + 2.0, at[2] + w + 4.0};
        double[] pmin = {f[0] - Walk.HALF_WIDTH, f[1], f[2] - Walk.HALF_WIDTH};
        double[] pmax = {f[0] + Walk.HALF_WIDTH, f[1] + Walk.HEIGHT, f[2] + Walk.HALF_WIDTH};
        boolean touches = hi[0] > pmin[0] && lo[0] < pmax[0] && hi[1] > pmin[1] && lo[1] < pmax[1] && hi[2] > pmin[2] && lo[2] < pmax[2];
        if (!touches || !(Geo.distSq(at, f) < 16.0)) {
            return 0.0f;
        }
        return Geo.splashHeal(at, f, false);
    }

    /**
     * Whether the line from a to b meets the box: {@code calculateIntercept}
     * is not null ({@link Geo#intercept}, the same arithmetic without
     * keeping the point).
     */
    private static boolean meets(double[] min, double[] max, double[] a, double[] b) {
        double[] d = {b[0] - a[0], b[1] - a[1], b[2] - a[2]};
        for (int axis = 0; axis < 3; axis++) {
            double da = d[axis];
            if (da * da < 1.0000000116860974E-7) {
                continue;
            }
            int u = axis == 0 ? 1 : 0, w = axis == 2 ? 1 : 2;
            for (int side = 0; side < 2; side++) {
                double t = ((side == 0 ? min[axis] : max[axis]) - a[axis]) / da;
                if (t >= 0.0 && t <= 1.0) {
                    double pu = a[u] + d[u] * t, pw = a[w] + d[w] * t;
                    if (pu >= min[u] && pu <= max[u] && pw >= min[w] && pw <= max[w]) {
                        return true;
                    }
                }
            }
        }
        return false;
    }

    /** The worst heal and its tick over the packets' patterns (all of them with jitter, the plain one without). */
    private static float[] worst(List<Up> p, Rep[] reps, boolean jitter) {
        int n = reps.length;
        float wHeal = POTION_HEAL;
        int wTick = 0;
        for (int l0 = 0; l0 <= (jitter ? 1 : 0); l0++) {
            float rHeal = 0.0f;
            int rTick = p.size();
            for (int i = 0; i < p.size(); i++) {
                Up u = p.get(i);
                int k = i + 1;
                boolean all = true;
                float low = POTION_HEAL;
                for (int e = 0; e <= (jitter ? 1 : 0); e++) {
                    Rep r = reps[Math.min(Math.max(k + l0 - e, 0), n - 1)];
                    float h;
                    if (k >= 5 && meets(r.bmin, r.bmax, u.pos, u.end)) {
                        h = POTION_HEAL;
                    } else {
                        all = false;
                        h = u.block ? splash(u.pos, r.feet) : 0.0f;
                    }
                    low = Math.min(low, h);
                }
                if (all) {
                    rHeal = POTION_HEAL;
                    rTick = k;
                    break;
                }
                if (u.block) {
                    rHeal = low;
                    rTick = k;
                    break;
                }
            }
            if (rHeal < wHeal || (rHeal == wHeal && rTick > wTick)) {
                wHeal = rHeal;
                wTick = rTick;
            }
        }
        return new float[] {wHeal, (float) wTick};
    }

    /** The spread a plan has to stand: the middle, 2.5 and 5 deviations along each axis, and the eight corners at 5. */
    private static final double[][] ZS;

    static {
        double d = 2.886751345948129;
        ZS = new double[][] {
            {0.0, 0.0, 0.0},
            {5.0, 0.0, 0.0},
            {-5.0, 0.0, 0.0},
            {0.0, 5.0, 0.0},
            {0.0, -5.0, 0.0},
            {0.0, 0.0, 5.0},
            {0.0, 0.0, -5.0},
            {2.5, 0.0, 0.0},
            {-2.5, 0.0, 0.0},
            {0.0, 2.5, 0.0},
            {0.0, -2.5, 0.0},
            {0.0, 0.0, 2.5},
            {0.0, 0.0, -2.5},
            {d, d, d},
            {d, d, -d},
            {d, -d, d},
            {d, -d, -d},
            {-d, d, d},
            {-d, d, -d},
            {-d, -d, d},
            {-d, -d, -d},
        };
    }

    /** The pitch a dy of mouse counts gives the client (a count 0.15 degrees). */
    static float pitchOf(float pitch, int dy) {
        float p = (float) ((double) pitch - (double) (-((float) dy)) * 0.15);
        return p < -90.0f ? -90.0f : p > 90.0f ? 90.0f : p;
    }

    static int counts(float deg) {
        return (int) Math.floor(deg / 0.15f + 0.5f);
    }

    /** Pieces of a key table. */
    private interface KeyAt {
        PKey at(int t);
    }

    private static void add(List<Integer> classes, List<PKey[]> progs, int cls, Integer j, KeyAt f) {
        PKey[] keys = new PKey[PLAN_TICKS];
        for (int t = 0; t < PLAN_TICKS; t++) {
            keys[t] = f.at(t).withJump(j != null && j.intValue() == t);
        }
        classes.add(cls);
        progs.add(keys);
    }

    private static final class Cached {
        final double[] feet, moved;
        final boolean onGround;
        /** Per whole degree of pitch from -90, per way of the server: the nominal path. */
        final List<List<List<Up>>> all;

        Cached(double[] feet, double[] moved, boolean onGround, List<List<List<Up>>> all) {
            this.feet = feet;
            this.moved = moved;
            this.onGround = onGround;
            this.all = all;
        }
    }

    private static boolean eq3(double[] a, double[] b) {
        return a[0] == b[0] && a[1] == b[1] && a[2] == b[2];
    }

    /** {@code (a, b, c) < (d, e, f)} as tuples. */
    private static boolean less(int[] a, int[] b) {
        for (int i = 0; i < 3; i++) {
            if (a[i] != b[i]) {
                return a[i] < b[i];
            }
        }
        return false;
    }

    /**
     * The plan for a potion thrown with a turn on the tick to come: w the
     * client's player now, dx the tick's mouse counts across (the turn in
     * them), own the policy's movement keys on it, carry what the agent
     * knows of the server adding its movement (-1 not yet). Null for none.
     */
    public static Plan plan(Walk w, Blocks blocks, int dx, PKey own, int carry) {
        float yaw1 = (float) ((double) w.yaw + (double) dx * 0.15);
        boolean[] ways = carry == 0 ? new boolean[] {false} : carry == 1 ? new boolean[] {true} : new boolean[] {false, true};
        double[] m = w.motion;
        double sp = Math.sqrt(m[0] * m[0] + m[2] * m[2]);
        PKey brake = null;
        if (sp > 0.02) {
            int[] fs = Agent.walkAlong(yaw1, new double[] {-m[0] / sp, -m[2] / sp});
            brake = new PKey(fs[0], fs[1], false, false);
        }
        final PKey ownK = own.withJump(false);
        final PKey brakeK = brake;
        List<Integer> classes = new ArrayList<Integer>();
        List<PKey[]> progs = new ArrayList<PKey[]>();
        Integer[] js = {null, 0, 1, 2, 3, 4};
        for (Integer j : js) {
            add(classes, progs, 0, j, new KeyAt() {
                public PKey at(int t) {
                    return ownK;
                }
            });
            if (!ownK.same(NONE)) {
                add(classes, progs, 1, j, new KeyAt() {
                    public PKey at(int t) {
                        return NONE;
                    }
                });
            }
            if (brakeK != null) {
                for (final int nb : new int[] {1, 2, 3}) {
                    add(classes, progs, 2, j, new KeyAt() {
                        public PKey at(int t) {
                            return t < nb ? brakeK : NONE;
                        }
                    });
                }
            }
        }
        Plan best = null;
        int[] bestScore = null;
        List<Cached> cache = new ArrayList<Cached>();
        for (int pi = 0; pi < progs.size(); pi++) {
            int cls = classes.get(pi);
            PKey[] keys = progs.get(pi);
            Rep[] reps = predict(w, blocks, dx, keys);
            Rep r0 = reps[0];
            Cached at = null;
            for (Cached e : cache) {
                if (eq3(e.feet, r0.feet) && eq3(e.moved, r0.moved) && e.onGround == r0.onGround) {
                    at = e;
                    break;
                }
            }
            if (at == null) {
                List<List<List<Up>>> all = new ArrayList<List<List<Up>>>(181);
                for (int d = -90; d <= 90; d++) {
                    float pitch = pitchOf(w.pitch, counts((float) d - w.pitch));
                    List<List<Up>> per = new ArrayList<List<Up>>(ways.length);
                    for (boolean way : ways) {
                        double[][] t = throwOf(r0, pitch, new double[] {0.0, 0.0, 0.0}, way);
                        per.add(path(blocks, t[0], t[1]));
                    }
                    all.add(per);
                }
                at = new Cached(r0.feet, r0.moved, r0.onGround, all);
                cache.add(at);
            }
            List<List<List<Up>>> nominal = at.all;
            boolean[] ok = new boolean[nominal.size()];
            for (int i = 0; i < ok.length; i++) {
                boolean good = true;
                for (List<Up> p : nominal.get(i)) {
                    if (!(worst(p, reps, false)[0] >= POTION_HEAL)) {
                        good = false;
                        break;
                    }
                }
                ok[i] = good;
            }
            int i = 0;
            while (i < ok.length) {
                if (!ok[i]) {
                    i++;
                    continue;
                }
                int e = i;
                while (e + 1 < ok.length && ok[e + 1]) {
                    e++;
                }
                int width = e - i + 1;
                float mid = (float) (i + e) / 2.0f - 90.0f;
                for (float off : new float[] {0.0f, -0.25f, 0.25f}) {
                    float deg = mid + off * (float) width;
                    int dy = counts(deg - w.pitch);
                    float pitch = pitchOf(w.pitch, dy);
                    float lowHeal = POTION_HEAL;
                    int lowTick = 0;
                    zs:
                    for (double[] z : ZS) {
                        for (boolean way : ways) {
                            double[][] t = throwOf(r0, pitch, z, way);
                            float[] r = worst(path(blocks, t[0], t[1]), reps, true);
                            int rt = (int) r[1];
                            if (r[0] < lowHeal || (r[0] == lowHeal && rt > lowTick)) {
                                lowHeal = r[0];
                                lowTick = rt;
                            }
                            if (lowHeal < POTION_HEAL) {
                                break zs;
                            }
                        }
                    }
                    if (lowHeal >= POTION_HEAL && lowTick <= MAX_HEAL_TICK) {
                        int[] score = {lowTick, cls, -width};
                        if (bestScore == null || less(score, bestScore)) {
                            Plan p = new Plan();
                            p.dy = dy;
                            p.yaw = yaw1;
                            p.keys = keys;
                            p.len = Math.min(lowTick + 2, PLAN_TICKS);
                            p.ticks = lowTick;
                            p.cls = cls;
                            p.window = width;
                            p.pitch = pitch;
                            p.jump = -1;
                            for (int t = 0; t < PLAN_TICKS; t++) {
                                if (keys[t].jump) {
                                    p.jump = t;
                                    break;
                                }
                            }
                            best = p;
                            bestScore = score;
                        }
                        break;
                    }
                }
                i = e + 1;
            }
        }
        return best;
    }

    /**
     * The keys of plan tick t for a view now at yaw: the plan's, turned
     * with the view when the policy has turned it (no sprint unless that is
     * still forward).
     */
    public static PKey keysNow(Plan p, int t, float yaw) {
        PKey k = p.keys[Math.min(t, PLAN_TICKS - 1)];
        if ((k.fw == 0 && k.st == 0) || Math.abs(Geo.wrapDeg((double) yaw - (double) p.yaw)) < 1.0) {
            return k;
        }
        float f = p.yaw * Geo.PI_F / 180.0f;
        double sn = (double) Geo.sin(f), cs = (double) Geo.cos(f);
        double fw = (double) k.fw, st = (double) k.st;
        double x = st * cs - fw * sn, z = fw * cs + st * sn;
        double l = Math.sqrt(x * x + z * z);
        int[] n = Agent.walkAlong(yaw, new double[] {x / l, z / l});
        return new PKey(n[0], n[1], k.sprint && n[0] == 1, k.jump);
    }

    /**
     * Works out a few plans for a player standing at feet in these blocks
     * and throws them away, so that the first real one, in the middle of a
     * duel, does not take the time of code not compiled yet: to be called
     * once when the agent starts. Changes nothing.
     */
    public static void warmUp(Blocks blocks, double[] feet, int times) {
        for (int n = 0; n < times; n++) {
            Walk w = new Walk();
            System.arraycopy(feet, 0, w.pos, 0, 3);
            System.arraycopy(feet, 0, w.said, 0, 3);
            w.min[0] = feet[0] - Walk.HALF_WIDTH;
            w.min[1] = feet[1];
            w.min[2] = feet[2] - Walk.HALF_WIDTH;
            w.max[0] = feet[0] + Walk.HALF_WIDTH;
            w.max[1] = feet[1] + Walk.HEIGHT;
            w.max[2] = feet[2] + Walk.HALF_WIDTH;
            w.onGround = true;
            w.motion[0] = n % 2 == 0 ? 0.1 : 0.0;
            w.motion[1] = -0.0784000015258789;
            w.motion[2] = n % 3 == 0 ? -0.1 : 0.05;
            w.yaw = 37.0f * n;
            w.saidYaw = w.yaw;
            w.sprinting = n % 2 == 0;
            w.attrSprint = w.sprinting;
            int dx = Agent.BAR_TURNS[n % Agent.BAR_TURNS.length];
            PKey own = n % 2 == 0 ? new PKey(1, 0, true, false) : NONE;
            plan(w, blocks, dx, own, n % 3 - 1);
        }
    }

    private FullHeal() {}
}
