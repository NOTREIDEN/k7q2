package chestai.client;

import java.util.ArrayList;
import java.util.List;
import java.util.Random;
import chestai.core.Frame;
import chestai.core.Memory;
import chestai.core.Pilot;

/**
 * What the player does, tick by tick, on top of the network's choices
 * (which it overrides):
 *
 * <ul>
 * <li>Health first: a splash potion at the feet when health is low, a
 * golden apple only when hurt and with no apple's effect left, never one
 * after the other for nothing.</li>
 * <li>Mobs: with none in reach the view turns to the middle of the biggest
 * group and the player sprints straight at it without swinging; with one
 * in reach it is aimed at and hit while the player keeps pushing into the
 * group.</li>
 * <li>Chests: one goal is picked (a claim first, then a wave that can be
 * started, then the one that comes soonest) and kept unless another is
 * clearly better; with a second chest close by the player stands between
 * them so that both waves come. The way there is planned through the
 * blocks (round walls, houses, trees, up and down hills, through water)
 * and walked facing forward at a sprint; far over open ground a Punch
 * arrow shot up carries the player along.</li>
 * </ul>
 *
 * The view always turns smoothly, at most {@link Pilot#MAX_DYAW} degrees a
 * tick sideways and {@link Pilot#MAX_DPITCH} up or down (faster only to
 * look down for a potion or up for the bow).
 */
final class Coach {
    static final int NONE = 0, FIGHT = 1, WAIT = 2, CLAIM = 3, GUARD = 4, TRAVEL = 5, APPROACH = 6, EXPLORE = 7, ESCAPE = 8;
    static final int HEAL_NONE = 0, HEAL_POTION = 1, HEAL_APPLE = 2;
    private static final String[] NAMES = {"quieto", "pelea", "espera", "reclamo", "guardia", "viaje", "acercarse", "explorar", "escapar"};

    /** A mob this close is fought; with a wave going on by the player, any of its mobs. */
    private static final double FIGHT_R = 12.0, WAVE_R = 24.0;
    /** A chest whose wave is on, this close, is not left until its mobs are dead. */
    private static final double LOCK_R = 16.0;
    /** Reach of the sword, from the eyes to the mob's box. */
    private static final double REACH = 3.0;
    /** Two chests this close: stand between them, so that both waves come. */
    private static final double PAIR_R = 22.0;
    /** Ticks between two potions. */
    private static final int POT_GAP = 22;
    /** The Punch boost: from this far, the view this high, the bow drawn this many ticks. */
    private static final double BOOST_D = Double.parseDouble(System.getProperty("chestai.boost.d", "30")),
        BOOST_PITCH = Double.parseDouble(System.getProperty("chestai.boost.pitch", "-45"));
    /** Fastest the player went in the ticks after the last boost, misses in a row, and no boosts until. */
    private double boostTop;
    private int boostMisses;
    private long boostOffUntil = -1;
    private boolean boostJudged = true;
    private static final int BOOST_DRAW = Integer.getInteger("chestai.boost.draw", 5);
    /** In a fight with a mob in reach: jump (blows on the way down are critical), and walk (no sprint knockback). */
    private static final boolean FIGHT_JUMP = Boolean.getBoolean("chestai.fight.jump"), FIGHT_WALK = Boolean.getBoolean("chestai.fight.walk");
    private boolean fightJump, fightWalk;

    private final Random rnd = new Random();
    private final Settings s;
    private final Planner planner;

    int mode = NONE, heal = HEAL_NONE;
    boolean boosting;
    private int target = -1;
    private double tx, ty, tz;
    private boolean aimSet, move, jumpUp, swim, surface;
    private double pitchAbs = Double.NaN;
    private double gain = 0.55;

    private long lastPot = -1000, lastApple = -10000, lastBoost = -1000, boostFrom = -1;
    /** Plain food being eaten (through the apple's slot), since when; the most absorption seen. */
    private boolean foodBite;
    private long foodAt = -1000;
    private float absTop;

    /** The chest gone for (its block), where to stand for it, and when it was picked. */
    private int goalX, goalY, goalZ;
    private boolean hasGoal;
    private double standX, standZ;
    private long goalAt = -1000;
    private String goalWhy = "";

    /** The way being walked. */
    private List<int[]> path;
    private int pi;
    private double pgx, pgy, pgz;
    private long pathAt = -1000;
    private double lastX, lastZ;
    private long movedAt;
    /** Where and when the player got stuck last, and until when it steps back to the middle of its block. */
    private double stuckX = 1e9, stuckZ = 1e9;
    private long stuckAt = -10000, unstickUntil = -1;
    /** Out of water, a wall, lava or harm from nowhere: where to, and until when. */
    private int[] dry;
    private long escapeUntil = -1, claimSince = -1;
    private int harmNoMob;
    /** Ways that end short of their goal, the player stuck at their end: out by an ender pearl. */
    private boolean partial;
    private int deadEnds;
    private long deadAt = -10000, pearlAt = -10000, pearlUntil = -1;
    private double pearlYaw, pearlPitch;
    private boolean pearlThrown;
    /** Never quite still: a short tap of a side key now and then (an anti-AFK), out exploring with nothing to do. */
    private int lastMode = NONE, tapDir = 1;
    private long tapNext, tapUntil = -1, wanderAt = -10000;
    private double wanderX, wanderZ;

    Coach(Settings s, Planner planner) {
        this.s = s;
        this.planner = planner;
    }

    private static double wrap(double a) {
        return ((a % 360.0) + 540.0) % 360.0 - 180.0;
    }

    private static double eyeY(Frame f) {
        return f.y + 1.62;
    }

    private static double yawTo(Frame f, double x, double z) {
        return wrap(Math.toDegrees(Math.atan2(-(x - f.x), z - f.z)) - f.yaw);
    }

    private static double pitchTo(Frame f, double x, double y, double z) {
        double dx = x - f.x, dz = z - f.z;
        return -Math.toDegrees(Math.atan2(y - eyeY(f), Math.sqrt(dx * dx + dz * dz))) - f.pitch;
    }

    private static double hd(double ax, double az, double bx, double bz) {
        return Math.sqrt((ax - bx) * (ax - bx) + (az - bz) * (az - bz));
    }

    private static double clamp(double v, double lo, double hi) {
        return v < lo ? lo : v > hi ? hi : v;
    }

    /** From the eyes to the nearest point of the mob's box. */
    private static double reach(Frame f, Frame.Mob m) {
        double ex = f.x, ey = eyeY(f), ez = f.z;
        double qx = clamp(ex, m.x - m.hw, m.x + m.hw);
        double qy = clamp(ey, m.y, m.y + m.h);
        double qz = clamp(ez, m.z - m.hw, m.z + m.hw);
        return Math.sqrt((qx - ex) * (qx - ex) + (qy - ey) * (qy - ey) + (qz - ez) * (qz - ez));
    }

    private void aim(double x, double y, double z) {
        tx = x;
        ty = y;
        tz = z;
        aimSet = true;
    }

    /** What the coach is doing, for the play log. */
    String describe() {
        StringBuilder b = new StringBuilder(NAMES[mode]);
        if (heal == HEAL_POTION) {
            b.append("+pocion");
        } else if (heal == HEAL_APPLE) {
            b.append(foodBite ? "+comida" : "+manzana");
        }
        if (boosting) {
            b.append("+arco");
        }
        if (hasGoal) {
            b.append(String.format(java.util.Locale.ROOT, " meta=(%d,%d,%d %s)", goalX, goalY, goalZ, goalWhy));
        }
        if (path != null) {
            b.append(" camino=").append(pi).append('/').append(path.size());
        }
        return b.toString();
    }

    /** The middle of the biggest group of these mobs, the near ones counting a little more. */
    private static double[] groupCenter(Frame f, List<Frame.Mob> c) {
        Frame.Mob best = null;
        double bs = -1e9;
        for (Frame.Mob m : c) {
            int n = 0;
            for (Frame.Mob o : c) {
                if (hd(m.x, m.z, o.x, o.z) < 4.5) {
                    n++;
                }
            }
            double sc = n - hd(m.x, m.z, f.x, f.z) / 8.0;
            if (sc > bs) {
                bs = sc;
                best = m;
            }
        }
        double x = 0, y = 0, z = 0;
        int n = 0;
        for (Frame.Mob o : c) {
            if (hd(best.x, best.z, o.x, o.z) < 4.5) {
                x += o.x + o.mx * 2.0;
                y += o.y;
                z += o.z + o.mz * 2.0;
                n++;
            }
        }
        return new double[] {x / n, y / n, z / n};
    }

    /** Seconds to a chest's next wave, by memory. */
    private static double est(Memory mem, int ci, long now) {
        return mem.estSecs(ci, now);
    }

    /** How good a chest is to go to now: lower is better. */
    private double score(Frame f, Memory mem, int ci) {
        Memory.Known k = mem.chests.get(ci);
        for (double[] r : f.rivals) {
            if (hd(r[0], r[2], k.cx(), k.cz()) <= 14.0) {
                // Someone else's to farm.
                return 1e9;
            }
        }
        double d = hd(k.cx(), k.cz(), f.x, f.z);
        double eta = d / 5.6;
        if (!k.seen) {
            return 150.0 + d * 0.5;
        }
        switch (k.state) {
            case Frame.CLAIMABLE:
                return d * 0.1;
            case Frame.READY:
                return 40.0 + d * 0.3;
            case Frame.ACTIVE:
                return d < 40.0 ? 30.0 + d * 0.3 : 1e5;
            default:
                return 80.0 + Math.max(est(mem, ci, f.tick) - eta, 0.0) + d * 0.2;
        }
    }

    private static String why(Memory.Known k) {
        if (!k.seen) {
            return "sin ver";
        }
        switch (k.state) {
            case Frame.CLAIMABLE:
                return "reclamar";
            case Frame.READY:
                return "lista";
            case Frame.ACTIVE:
                return "oleada";
            default:
                return k.secs + "s";
        }
    }

    /** Picks the chest to go for, keeping the one picked unless another is clearly better. */
    private int chooseGoal(Frame f, Memory mem) {
        int cur = hasGoal ? mem.near(goalX, goalY, goalZ, 0.5) : -1;
        if (cur >= 0 && f.tick - goalAt < 10) {
            return cur;
        }
        int best = -1;
        double bs = 1e8;
        for (int i = 0; i < mem.chests.size(); i++) {
            double sc = score(f, mem, i);
            if (sc < bs) {
                bs = sc;
                best = i;
            }
        }
        if (cur >= 0) {
            double cs = score(f, mem, cur);
            if (cs < 1e5 && cs <= bs * 1.2 + 8.0) {
                best = cur;
                bs = cs;
            }
        }
        goalAt = f.tick;
        if (best < 0) {
            hasGoal = false;
            return -1;
        }
        Memory.Known k = mem.chests.get(best);
        hasGoal = true;
        goalX = k.x;
        goalY = k.y;
        goalZ = k.z;
        goalWhy = why(k);
        standX = k.cx();
        standZ = k.cz();
        if (k.seen && k.state != Frame.CLAIMABLE) {
            // A second chest close by whose wave comes about as soon: stand between the two.
            double bestPair = 1e9;
            for (int j = 0; j < mem.chests.size(); j++) {
                Memory.Known o = mem.chests.get(j);
                if (j == best || !o.seen) {
                    continue;
                }
                double dd = hd(o.cx(), o.cz(), k.cx(), k.cz());
                boolean along = o.state == Frame.READY || o.state == Frame.ACTIVE
                    || (o.state == Frame.COOLDOWN && Math.abs(est(mem, j, f.tick) - est(mem, best, f.tick)) < 8.0);
                if (dd > 2.0 && dd < PAIR_R && along && dd < bestPair) {
                    bestPair = dd;
                    standX = (k.cx() + o.cx()) / 2.0;
                    standZ = (k.cz() + o.cz()) / 2.0;
                    goalWhy = why(k) + "+par";
                }
            }
        }
        return best;
    }

    /** The chest with a wave on close by, which is not left until its mobs are dead, or -1. */
    private static int waveLock(Frame f, Memory mem) {
        int lock = -1;
        double ld = LOCK_R;
        for (int i = 0; i < mem.chests.size(); i++) {
            Memory.Known k = mem.chests.get(i);
            double d = hd(k.cx(), k.cz(), f.x, f.z);
            if (k.seen && k.state == Frame.ACTIVE && f.tick - k.tick < 200 && d < ld) {
                ld = d;
                lock = i;
            }
        }
        return lock;
    }

    private boolean inWater(Frame f) {
        int x = (int) Math.floor(f.x), y = (int) Math.floor(f.y + 0.1), z = (int) Math.floor(f.z);
        return planner.water(x, y, z) || planner.water(x, y + 1, z);
    }

    /**
     * Walks to (gx, gy, gz), to within {@code tol} blocks: straight when
     * the ground allows it, else along a planned way, always facing where
     * it goes.
     */
    private void go(Frame f, double gx, double gy, double gz, double tol) {
        move = true;
        objective(gx, gy, gz, f.tick);
        swim = inWater(f);
        double d = hd(gx, gz, f.x, f.z);
        // Stuck: the way is planned again from here.
        if (hd(lastX, lastZ, f.x, f.z) > 1.0) {
            lastX = f.x;
            lastZ = f.z;
            movedAt = f.tick;
        }
        boolean stuck = f.tick - movedAt > 30;
        planner.now(f.tick);
        if (stuck) {
            movedAt = f.tick;
            if (partial) {
                // Stuck where the way ends short of the goal: a pit, a ravine, a pond with high banks.
                deadEnds = f.tick - deadAt < 600 ? deadEnds + 1 : 1;
                deadAt = f.tick;
            }
            if (path != null && f.tick - stuckAt < 300 && hd(stuckX, stuckZ, f.x, f.z) < 2.0) {
                // Stuck again at the same place: the next spot of the way is left out for a while.
                int[] n = path.get(Math.min(path.size() - 1, pi + 1));
                planner.ban(n[0], n[1], n[2], f.tick + 300);
            }
            stuckX = f.x;
            stuckZ = f.z;
            stuckAt = f.tick;
            path = null;
            unstickUntil = f.tick + 10;
        }
        if (f.tick < unstickUntil) {
            // Off the wall it is pressed against: to the middle of its block first.
            double cx = Math.floor(f.x) + 0.5, cz = Math.floor(f.z) + 0.5;
            if (hd(cx, cz, f.x, f.z) > 0.12) {
                aim(cx, eyeY(f) - 0.4, cz);
                jumpUp = swim;
                return;
            }
            unstickUntil = -1;
        }
        if (d < 7.0 && planner.straight(f.x, f.y, f.z, gx, gz)) {
            path = null;
            aim(gx, eyeY(f) - 0.4, gz);
            jumpUp = (f.collidedH && f.onGround) || swim;
            return;
        }
        boolean far = path == null || hd(gx, gz, pgx, pgz) > 2.0 || Math.abs(gy - pgy) > 3.0;
        if (far || f.tick - pathAt > 40) {
            planner.clear();
            path = planner.plan(f.x, f.y, f.z, gx, gy, gz, tol);
            partial = path == null || !planner.reached;
            pi = 0;
            pathAt = f.tick;
            pgx = gx;
            pgy = gy;
            pgz = gz;
        }
        if (path == null) {
            aim(gx, eyeY(f) - 0.4, gz);
            jumpUp = (f.collidedH && f.onGround) || swim;
            return;
        }
        // The node of the way the player is at.
        int last = path.size() - 1;
        int bestI = pi;
        double bd = 1e9;
        for (int i = pi; i <= Math.min(last, pi + 8); i++) {
            int[] n = path.get(i);
            double dd = hd(n[0] + 0.5, n[2] + 0.5, f.x, f.z) + Math.abs(n[1] - f.y) * 0.5;
            if (dd < bd) {
                bd = dd;
                bestI = i;
            }
        }
        pi = bestI;
        if (bd > 3.0) {
            // Off the way: plan again next tick.
            pathAt = -1000;
        }
        // Look as far along it as the ground lets the player walk straight.
        int j = Math.min(last, pi + 1);
        for (int k = Math.min(last, pi + 6); k > pi + 1; k--) {
            int[] n = path.get(k);
            if (Math.abs(n[1] - Math.floor(f.y + 0.01)) <= 1 && planner.straight(f.x, f.y, f.z, n[0] + 0.5, n[2] + 0.5)) {
                j = k;
                break;
            }
        }
        int[] to = path.get(j);
        aim(to[0] + 0.5, Math.max(to[1] + 1.2, eyeY(f) - 0.6), to[2] + 0.5);
        int[] next = path.get(Math.min(last, pi + 1));
        jumpUp = next[1] > Math.floor(f.y + 0.01) || (f.collidedH && f.onGround) || (swim && next[1] >= Math.floor(f.y + 0.01));
    }

    /**
     * Where an ender pearl thrown with this yaw and pitch comes down: the
     * spot it lands in front of, or null if it flies on or hits nothing to
     * stand on. As the game moves it: 1.5 blocks a tick, 1% drag, 0.03 down.
     */
    private int[] pearlLands(Frame f, double yaw, double pitch) {
        double yr = Math.toRadians(yaw), pr = Math.toRadians(pitch);
        double x = f.x, y = eyeY(f) - 0.1, z = f.z;
        double mx = -Math.sin(yr) * Math.cos(pr) * 1.5, my = -Math.sin(pr) * 1.5, mz = Math.cos(yr) * Math.cos(pr) * 1.5;
        int[] air = null;
        for (int i = 0; i < 100; i++) {
            for (int s = 1; s <= 6; s++) {
                double px = x + mx * s / 6.0, py = y + my * s / 6.0, pz = z + mz * s / 6.0;
                int bx = (int) Math.floor(px), by = (int) Math.floor(py), bz = (int) Math.floor(pz);
                if (by < 0 || by > 255) {
                    return null;
                }
                if (planner.open(bx, by, bz)) {
                    air = new int[] {bx, by, bz};
                    continue;
                }
                if (air == null) {
                    return null;
                }
                // Down on top of something, or against its side with ground under: a spot to stand.
                for (int dy = 0; dy >= -2; dy--) {
                    if (planner.stand(air[0], air[1] + dy, air[2])) {
                        return new int[] {air[0], air[1] + dy, air[2]};
                    }
                }
                return null;
            }
            x += mx;
            y += my;
            z += mz;
            mx *= 0.99;
            my = my * 0.99 - 0.03;
            mz *= 0.99;
        }
        return null;
    }

    /** {yaw, pitch} of a pearl that lands clearly nearer the goal of the way, out of the hole the player is in, or null. */
    private double[] pearlShot(Frame f) {
        planner.clear();
        double toGoal = Math.toDegrees(Math.atan2(-(pgx - f.x), pgz - f.z));
        double best = hd(pgx, pgz, f.x, f.z) + 0.3 * Math.abs(pgy - f.y) - 4.0;
        double[] out = null;
        int y0 = (int) Math.floor(f.y + 0.01);
        for (int dyaw = -90; dyaw <= 90; dyaw += 15) {
            for (int pitch = -75; pitch <= 25; pitch += 2) {
                int[] l = pearlLands(f, toGoal + dyaw, pitch);
                if (l == null || hd(l[0] + 0.5, l[2] + 0.5, f.x, f.z) < 3.0 || l[1] < y0 - 1 || planner.water(l[0], l[1], l[2])) {
                    continue;
                }
                double d = hd(l[0] + 0.5, l[2] + 0.5, pgx, pgz) + 0.3 * Math.abs(l[1] - pgy);
                if (d < best) {
                    best = d;
                    out = new double[] {toGoal + dyaw, pitch};
                }
            }
        }
        return out;
    }

    /** Points of a chest's box a click can be aimed at: its middle, its lid, and in from its corners. */
    private static final double[][] LID = {{0, 0.8, 0}, {0, 0.45, 0}, {0.28, 0.8, 0.28}, {-0.28, 0.8, -0.28}, {0.28, 0.8, -0.28},
        {-0.28, 0.8, 0.28}, {0.3, 0.4, 0}, {-0.3, 0.4, 0}, {0, 0.4, 0.3}, {0, 0.4, -0.3}};

    /** Whether a click from the eyes at (ex, ey, ez) reaches point i of the chest. */
    private boolean seesLid(double ex, double ey, double ez, Memory.Known k, int i) {
        double px = k.cx() + LID[i][0], py = k.y + LID[i][1], pz = k.cz() + LID[i][2];
        double dx = px - ex, dy = py - ey, dz = pz - ez;
        return dx * dx + dy * dy + dz * dz <= 4.3 * 4.3 && planner.see(ex, ey, ez, px, py, pz, k.x, k.y, k.z);
    }

    /** A point of the chest a click reaches from where the player is, the one nearest the view (another after a while), or null. */
    private double[] lidPoint(Frame f, Memory.Known k, long tried) {
        double ex = f.x, ey = eyeY(f), ez = f.z;
        List<double[]> ok = new ArrayList<double[]>();
        for (int i = 0; i < LID.length; i++) {
            if (seesLid(ex, ey, ez, k, i)) {
                double px = k.cx() + LID[i][0], py = k.y + LID[i][1], pz = k.cz() + LID[i][2];
                ok.add(new double[] {px, py, pz, Math.abs(yawTo(f, px, pz)) + Math.abs(pitchTo(f, px, py, pz))});
            }
        }
        if (ok.isEmpty()) {
            return null;
        }
        java.util.Collections.sort(ok, new java.util.Comparator<double[]>() {
            public int compare(double[] p, double[] q) {
                return Double.compare(p[3], q[3]);
            }
        });
        return ok.get((int) ((tried / 20) % ok.size()));
    }

    private int[] spot;
    private long spotAt = -1000;
    private int spotFor = Integer.MIN_VALUE;

    /** A spot within four blocks of the chest to stand at, from where a click reaches it, the nearest to the player; or null. */
    private int[] claimSpot(Frame f, Memory.Known k, long t) {
        int key = k.x * 31 + k.z * 17 + k.y;
        if (key == spotFor && t - spotAt < 20) {
            return spot;
        }
        spotFor = key;
        spotAt = t;
        spot = null;
        double best = 1e9;
        for (int dx = -4; dx <= 4; dx++) {
            for (int dz = -4; dz <= 4; dz++) {
                if (dx * dx + dz * dz > 16 || (dx == 0 && dz == 0)) {
                    continue;
                }
                for (int dy = -2; dy <= 1; dy++) {
                    int x = k.x + dx, y = k.y + dy, z = k.z + dz;
                    double d = hd(x + 0.5, z + 0.5, f.x, f.z) + 0.6 * Math.abs(y - f.y);
                    if (d >= best || !planner.stand(x, y, z)) {
                        continue;
                    }
                    for (int i = 0; i < LID.length; i++) {
                        if (seesLid(x + 0.5, y + 1.62, z + 0.5, k, i)) {
                            best = d;
                            spot = new int[] {x, y, z};
                            break;
                        }
                    }
                }
            }
        }
        return spot;
    }

    /** Whether a step sideways (left for +1) keeps the player on safe ground: no drop of more than one, no water, no harm. */
    private boolean sideSafe(Frame f, int dir) {
        double yr = Math.toRadians(f.yaw);
        double sx = Math.cos(yr) * dir, sz = Math.sin(yr) * dir;
        int x = (int) Math.floor(f.x + sx * 0.9), z = (int) Math.floor(f.z + sz * 0.9), y = (int) Math.floor(f.y + 0.01);
        return (planner.stand(x, y, z) || planner.stand(x, y - 1, z)) && !planner.water(x, y, z) && !planner.water(x, y - 1, z);
    }

    /** The view clear up and ahead for an arrow shot up, and open ground ahead for the boost. */
    private boolean boostRoom(Frame f) {
        if (path == null) {
            return false;
        }
        int[] end = path.get(path.size() - 1);
        int[] ahead = path.get(Math.min(path.size() - 1, pi + 8));
        if (hd(end[0] + 0.5, end[2] + 0.5, f.x, f.z) < 25.0 || hd(ahead[0] + 0.5, ahead[2] + 0.5, f.x, f.z) < 5.0
                || !planner.straight(f.x, f.y, f.z, ahead[0] + 0.5, ahead[2] + 0.5)) {
            return false;
        }
        double yr = Math.toRadians(f.yaw);
        double dx = -Math.sin(yr), dz = Math.cos(yr);
        int y0 = (int) Math.floor(f.y + 0.01);
        for (int i = 0; i <= 4; i++) {
            int bx = (int) Math.floor(f.x + dx * i), bz = (int) Math.floor(f.z + dz * i);
            if (!planner.open(bx, y0 + 2, bz) || !planner.open(bx, y0 + 3, bz)) {
                return false;
            }
        }
        return true;
    }

    /** Changes the network's choices {@code a} of this tick, before the pilot turns them into keys. */
    void before(Frame f, Memory mem, Pilot pilot, int[] a, Layout lay) {
        long t = f.tick;
        if (!boostJudged) {
            // Did the arrow carry the player? Two that did not: no boosts for a minute.
            boostTop = Math.max(boostTop, Math.sqrt(f.mx * f.mx + f.mz * f.mz));
            if (t - lastBoost >= 30) {
                boostJudged = true;
                if (boostTop > 0.6) {
                    boostMisses = 0;
                } else if (++boostMisses >= 2) {
                    boostMisses = 0;
                    boostOffUntil = t + 1200;
                }
            }
        }
        // The coach drives: from keys at rest.
        a[0] = 1;
        a[1] = 1;
        a[2] = 0;
        a[3] = 0;
        a[4] = 7;
        a[5] = 5;
        a[6] = 0;
        a[7] = 0;
        a[8] = 0;
        if (mode != CLAIM) {
            claimSince = -1;
        }
        lastMode = mode;
        mode = NONE;
        heal = HEAL_NONE;
        aimSet = false;
        move = false;
        jumpUp = false;
        swim = false;
        pitchAbs = Double.NaN;
        fightJump = false;
        fightWalk = false;
        double hp = f.hp;

        int lock = waveLock(f, mem);
        double engage = lock >= 0 ? WAVE_R : FIGHT_R;
        List<Frame.Mob> cand = new ArrayList<Frame.Mob>();
        int close = 0;
        for (Frame.Mob m : f.mobs) {
            double d = hd(m.x, m.z, f.x, f.z);
            if (d < 8.0) {
                close++;
            }
            if (d <= engage && Math.abs(m.y - f.y) <= 8.0) {
                cand.add(m);
            }
        }
        boolean eating = pilot.using && pilot.slot == Pilot.GAPPLE;
        boolean drawing = pilot.using && pilot.slot == Pilot.BOW;

        // 1. Not dying comes first.
        boolean havePot = lay.real[Pilot.POTION] >= 0 && lay.potions > 0;
        int apples = lay.real[Pilot.GAPPLE] >= 0 ? lay.gapples : 0;
        boolean haveApple = apples > 0, haveFood = lay.food >= 0 && lay.foodCount > 0;
        absTop = Math.max(absTop, f.absorption);
        double nearest = 1e9;
        for (Frame.Mob m : f.mobs) {
            nearest = Math.min(nearest, hd(m.x, m.z, f.x, f.z));
        }
        boolean claimNear = false;
        for (Memory.Known k : mem.chests) {
            claimNear |= k.seen && k.state == Frame.CLAIMABLE && hd(k.cx(), k.cz(), f.x, f.z) < 5.0;
        }
        boolean potNeed = havePot && !eating && t - lastPot > POT_GAP
            && ((hp <= s.potHp && (mem.sinceHurt < 100 || close > 0)) || hp <= s.potHp * 0.6);
        boolean appleGap = t - lastApple > Math.round(s.appleWaitS * 20.0);
        // An apple when hurt: health or absorption down (the absorption kept full), while there are
        // apples to spare; with few left, only when health is low.
        boolean spare = apples > s.appleKeep;
        boolean absDown = f.absorption < Math.max(0.5, absTop - 0.4);
        // (Its own Punch arrow, with no mob about, is no reason.)
        boolean ownArrow = t - lastBoost < 60 && nearest > 12.0;
        boolean appleNeed = !potNeed && !eating && haveApple && appleGap
            && ((mem.sinceHurt < 60 && (s.appleOnHit && spare && !ownArrow ? hp < 19.9 || absDown : hp <= s.appleHp && f.absorption < 0.5f && f.regenLeft == 0))
                || (hp <= s.potHp && !havePot));
        // Food as soon as the bar is short: plain food first, else an apple; not with a claim at hand,
        // nor with a mob at the sword while the bar still lets the player sprint for long.
        boolean foodNeed = s.foodFull && !potNeed && !appleNeed && !eating && f.food < 20 && !claimNear
            && (nearest > 3.5 || f.food <= 14)
            && (haveFood || (haveApple && (f.food <= 6 || (spare && appleGap))));
        if (drawing && (potNeed || appleNeed || foodNeed || !cand.isEmpty())) {
            // The bow is let go: something matters more.
            boosting = false;
            boostFrom = -1;
        }
        if (foodBite && !eating && t - foodAt > 3) {
            foodBite = false;
        }
        if (eating) {
            heal = HEAL_APPLE;
            a[7] = 1;
        } else if (potNeed) {
            heal = HEAL_POTION;
            pitchAbs = 88.0;
            if (f.pitch >= 76.0f && !pilot.using) {
                a[8] = 1 + Pilot.POTION;
            }
        } else if ((appleNeed || foodNeed) && !pilot.using) {
            heal = HEAL_APPLE;
            a[8] = 1 + Pilot.GAPPLE;
            a[7] = 1;
            if (appleNeed || !haveFood) {
                lastApple = t;
            } else {
                foodBite = true;
                foodAt = t;
            }
        }
        if (foodBite && haveFood) {
            // Plain food goes through the apple's slot: the hotbar key is the food's.
            lay.real[Pilot.GAPPLE] = lay.food;
            lay.gapples = lay.foodCount;
        }

        // 2. Out of what hurts with no mob doing it: water over the head, a wall, lava, fire.
        if (mem.sinceHurt == 0 && t - lastBoost > 40) {
            // (Its own Punch arrow is no harm from nowhere.)
            harmNoMob = nearest > 12.0 ? harmNoMob + 1 : 0;
        } else if (mem.sinceHurt > 100) {
            harmNoMob = 0;
        }
        surface = f.headInWater;
        boolean hazard = (f.headInWater && f.air < 150) || f.inWall || f.inLava || (f.burning && !f.headInWater) || harmNoMob >= 3;
        if (hazard && t >= escapeUntil) {
            planner.clear();
            dry = planner.nearestDry(f.x, f.y, f.z, 8);
            escapeUntil = t + 60;
            harmNoMob = 0;
        }
        boolean pearlOk = lay.real[Pilot.PEARL] >= 0 && lay.pearls > 0 && f.pearlCdLeft == 0 && heal == HEAL_NONE
            && hp + f.absorption >= 12.0f && !pilot.using;
        if (pearlUntil < t && deadEnds >= 2 && pearlOk && t - pearlAt > 300) {
            double[] shot = pearlShot(f);
            deadEnds = 0;
            if (shot != null) {
                pearlYaw = shot[0];
                pearlPitch = shot[1];
                pearlUntil = t + 50;
                pearlThrown = false;
            }
        }
        if (pearlUntil >= t && heal == HEAL_NONE) {
            // No way out on foot: an ender pearl to where the goal can be walked to.
            mode = ESCAPE;
            boosting = false;
            if (!pearlThrown) {
                double yr = Math.toRadians(pearlYaw);
                aim(f.x - Math.sin(yr) * 30.0, eyeY(f), f.z + Math.cos(yr) * 30.0);
                pitchAbs = pearlPitch;
                if (Math.abs(yawTo(f, tx, tz)) < 1.5 && Math.abs(f.pitch - pearlPitch) < 1.5) {
                    a[8] = 1 + Pilot.PEARL;
                }
            }
        } else if (t < escapeUntil && dry != null && heal != HEAL_POTION) {
            mode = ESCAPE;
            boosting = false;
            if (hd(dry[0] + 0.5, dry[2] + 0.5, f.x, f.z) > 0.8 || Math.abs(dry[1] - f.y) > 1.2) {
                go(f, dry[0] + 0.5, dry[1], dry[2] + 0.5, 0.6);
            } else {
                escapeUntil = -1;
            }
        } else if (!cand.isEmpty()) {
            // 3. Mobs.
            boosting = false;
            boostFrom = -1;
            Frame.Mob tgt = null;
            double bs = 1e9, tgtY = Double.NaN;
            for (Frame.Mob m : cand) {
                double r = reach(f, m);
                if (r > REACH) {
                    continue;
                }
                // One just hit takes no harm for half a second: another one meanwhile. One behind a
                // block or tall grass, where a click would stop, only if there is no other.
                double vy = seenAt(f, m);
                double sc = r + Math.abs(yawTo(f, m.x, m.z)) / 30.0 + (m.hurtTime > 2 ? 2.0 : 0.0) + (m.id == target && m.hurtTime <= 2 ? -0.8 : 0.0)
                    + (vy != vy ? 2.5 : 0.0);
                if (sc < bs) {
                    bs = sc;
                    tgt = m;
                    tgtY = vy;
                }
            }
            double[] gc = groupCenter(f, cand);
            objective(gc[0], gc[1], gc[2], t);
            if (tgt != null) {
                mode = FIGHT;
                target = tgt.id;
                // Low on the body (the name a server keeps over a mob's head can be in the way higher up),
                // higher where grass or a block is in the way. On a horse: the rider, not the horse.
                double ax = tgt.x + tgt.mx * 2.0, az = tgt.z + tgt.mz * 2.0,
                    ay = tgtY == tgtY ? tgtY : tgt.kind == Frame.HORSEMAN ? tgt.y + 2.0 : tgt.y + Math.min(tgt.h, 2.6) * 0.45;
                aim(ax, ay, az);
                if (s.hit && heal == HEAL_NONE && !pilot.using) {
                    // A click only with a mob under the crosshair that can take it (none spent on air).
                    boolean ok = false;
                    for (Frame.Mob m : f.mobs) {
                        ok |= m.id == f.pickMob && m.hurtTime <= 2;
                    }
                    if (f.pickMob < 0 && tgtY == tgtY && Math.abs(yawTo(f, ax, az)) < 4.0 && Math.abs(pitchTo(f, ax, ay, az)) < 6.0) {
                        ok = true;
                    }
                    if (ok) {
                        a[6] = 1;
                    }
                }
                fightJump = FIGHT_JUMP && f.onGround && !inWater(f);
                fightWalk = FIGHT_WALK;
                if (hd(gc[0], gc[2], f.x, f.z) > 1.8 && hd(tgt.x, tgt.z, f.x, f.z) > 1.2) {
                    move = true;
                    jumpUp = (f.collidedH && f.onGround) || inWater(f);
                }
            } else {
                // None in reach: straight at the group, without swinging.
                mode = APPROACH;
                target = -1;
                go(f, gc[0], gc[1], gc[2], 1.5);
                aim(tx, gc[1] + 1.0, tz);
            }
        } else if (lock >= 0) {
            // The wave's mobs are out of sight for now: back to its chest and wait there.
            mode = GUARD;
            Memory.Known k = mem.chests.get(lock);
            if (hd(k.cx(), k.cz(), f.x, f.z) > (lastMode == GUARD ? 4.5 : 3.0)) {
                go(f, k.cx(), k.y, k.cz(), 2.0);
            } else {
                aim(k.cx(), eyeY(f), k.cz());
            }
        } else {
            // 4. Chests.
            int g = chooseGoal(f, mem);
            if (g >= 0) {
                Memory.Known k = mem.chests.get(g);
                double dc = hd(k.cx(), k.cz(), f.x, f.z);
                double ds = hd(standX, standZ, f.x, f.z);
                if (k.seen && k.state == Frame.CLAIMABLE && dc < 4.0) {
                    mode = CLAIM;
                    if (claimSince < 0) {
                        claimSince = t;
                    }
                    long tried = t - claimSince;
                    // A point of the chest a click reaches from here (nothing in between, not even tall
                    // grass); with none, to a spot near it from where one is.
                    double[] lid = lidPoint(f, k, tried);
                    if (lid != null) {
                        aim(lid[0], lid[1], lid[2]);
                    } else {
                        int[] at = claimSpot(f, k, t);
                        if (at != null) {
                            go(f, at[0] + 0.5, at[1], at[2] + 0.5, 0.4);
                        } else {
                            go(f, k.cx(), k.y, k.cz(), 1.0);
                        }
                    }
                    a[7] = heal == HEAL_NONE && !pilot.using ? 1 : a[7];
                    if (tried > 240) {
                        claimSince = -1;
                    }
                } else if (k.seen && k.state == Frame.CLAIMABLE) {
                    mode = TRAVEL;
                    go(f, k.cx(), k.y, k.cz(), 2.0);
                } else if (ds < (lastMode == WAIT ? 3.0 : 2.0)) {
                    mode = WAIT;
                    aim(k.cx(), k.cy() + 0.5, k.cz());
                } else {
                    mode = k.seen ? TRAVEL : EXPLORE;
                    go(f, standX, k.y, standZ, 1.5);
                    boost(f, pilot, a, lay, dc);
                }
            }
        }
        if (mode == NONE && heal != HEAL_POTION) {
            // Nothing known to go to: out looking, 30 blocks at a time.
            if (t - wanderAt > 600 || hd(wanderX, wanderZ, f.x, f.z) < 3.0) {
                double ang = rnd.nextDouble() * Math.PI * 2.0;
                wanderX = f.x + Math.cos(ang) * 30.0;
                wanderZ = f.z + Math.sin(ang) * 30.0;
                wanderAt = t;
            }
            mode = EXPLORE;
            go(f, wanderX, f.y, wanderZ, 2.0);
        }
        if (!boosting && heal == HEAL_NONE && !pilot.using && pilot.slot != Pilot.SWORD && lay.real[Pilot.SWORD] >= 0) {
            a[8] = 1 + Pilot.SWORD;
        }
        trapped(f);
        dodge(f);
    }

    /** Where the player is trying to get to (a way's goal, a group of mobs), and when that was last said. */
    private double objX, objY, objZ;
    private long objAt = -1000;
    /** Where the player has stayed (within four blocks) since when. */
    private double trapX = 1e9, trapZ = 1e9;
    private long trapAt;

    private void objective(double x, double y, double z, long t) {
        objX = x;
        objY = y;
        objZ = z;
        objAt = t;
    }

    /**
     * Twelve seconds within four blocks while trying to get somewhere farther
     * or well above (a pit, a ravine, a hole by a bank, mobs up on the edge):
     * out by an ender pearl, as with a way that ends short.
     */
    private void trapped(Frame f) {
        long t = f.tick;
        boolean busy = mode == WAIT || mode == CLAIM || mode == GUARD || mode == ESCAPE || heal != HEAL_NONE || t - objAt > 2;
        if (busy || hd(trapX, trapZ, f.x, f.z) > 4.0) {
            trapX = f.x;
            trapZ = f.z;
            trapAt = t;
            return;
        }
        if (t - trapAt > 240 && (hd(objX, objZ, f.x, f.z) > 5.0 || objY - f.y > 2.5)) {
            trapAt = t;
            deadEnds = 2;
            pgx = objX;
            pgy = objY;
            pgz = objZ;
        }
    }

    /** A mob's arrow (or snowball, or fireball) on its way to the player: a step aside, to the side it passes. */
    private int dodgeDir;
    private long dodgeUntil = -1;

    private void dodge(Frame f) {
        for (Frame.Proj p : f.projs) {
            if (p.owner < 0 || (p.kind != Frame.ARROW && p.kind != Frame.SNOWBALL)) {
                continue;
            }
            double vx = p.mx, vy = p.my, vz = p.mz;
            if (vx * vx + vy * vy + vz * vz < 0.09) {
                continue;
            }
            double x = p.x, y = p.y, z = p.z, cy = f.y + 0.9, near = 1e9;
            int when = -1;
            for (int i = 1; i <= 25; i++) {
                x += vx;
                y += vy;
                z += vz;
                vx *= 0.99;
                vy = vy * 0.99 - 0.05;
                vz *= 0.99;
                double dx = x - f.x, dy = (y - cy) * 0.6, dz = z - f.z, d = Math.sqrt(dx * dx + dy * dy + dz * dz);
                if (d < near) {
                    near = d;
                    when = i;
                }
            }
            if (near > 1.4 || when < 2 || when > 20) {
                continue;
            }
            double hl = Math.sqrt(p.mx * p.mx + p.mz * p.mz);
            if (hl < 1e-3) {
                continue;
            }
            double hx = p.mx / hl, hz = p.mz / hl;
            double side = hx * (f.z - p.z) - hz * (f.x - p.x);
            double sg = side >= 0 ? 1.0 : -1.0;
            double nx = -hz * sg, nz = hx * sg;
            double yr = Math.toRadians(f.yaw);
            double dot = nx * Math.cos(yr) + nz * Math.sin(yr);
            if (Math.abs(dot) > 0.25) {
                dodgeDir = dot > 0 ? 1 : -1;
                dodgeUntil = f.tick + 6;
            }
            return;
        }
    }

    /** The height on a mob to aim at that a click from the eyes reaches (no block or tall grass between), or NaN. */
    private double seenAt(Frame f, Frame.Mob m) {
        double ex = f.x, ey = eyeY(f), ez = f.z;
        double base = m.kind == Frame.HORSEMAN ? m.y + 1.4 : m.y, h = m.kind == Frame.HORSEMAN ? 1.0 : Math.min(m.h, 2.6);
        double[] at = {0.45, 0.7, 0.25, 0.88};
        for (double q : at) {
            double y = base + h * q;
            if (planner.see(ex, ey, ez, m.x, y, m.z, Integer.MIN_VALUE, 0, 0)) {
                return y;
            }
        }
        return Double.NaN;
    }

    /** Far to go on open ground: a Punch arrow shot up that carries the player along. */
    private void boost(Frame f, Pilot pilot, int[] a, Layout lay, double dist) {
        boolean can = s.bow && heal == HEAL_NONE && lay.real[Pilot.BOW] >= 0 && lay.arrows > 0 && f.punch >= 1 && !swim && f.tick >= boostOffUntil;
        if (boosting) {
            if (!can || f.tick - boostFrom > 40) {
                boosting = false;
                boostFrom = -1;
                return;
            }
        } else {
            if (!can || dist < BOOST_D || f.tick - lastBoost < 80 || !boostJudged || !f.onGround || !aimSet || Math.abs(yawTo(f, tx, tz)) > 15.0 || !boostRoom(f)) {
                return;
            }
            boosting = true;
            boostFrom = f.tick;
        }
        if (pilot.slot != Pilot.BOW) {
            a[8] = 1 + Pilot.BOW;
        } else if (!pilot.using) {
            if (f.pitch > BOOST_PITCH + 5.0) {
                pitchAbs = BOOST_PITCH;
            } else {
                a[7] = 1;
            }
            if (pitchAbs != pitchAbs) {
                pitchAbs = BOOST_PITCH;
            }
        } else {
            pitchAbs = BOOST_PITCH;
            if (pilot.useTicks < BOOST_DRAW) {
                a[7] = 1;
            } else {
                // Let go: the arrow is off.
                a[7] = 0;
                lastBoost = f.tick;
                boosting = false;
                boostFrom = -1;
                boostTop = 0.0;
                boostJudged = false;
            }
        }
    }

    /** Changes the keys of this tick, after the pilot. */
    void after(Frame f, Pilot.Keys k) {
        if (k.throwIt && k.slot == Pilot.POTION) {
            lastPot = f.tick;
        }
        if (k.throwIt && k.slot == Pilot.PEARL && mode == ESCAPE) {
            // Thrown: still until it lands, then a new way from where the player is.
            pearlThrown = true;
            pearlAt = f.tick;
            pearlUntil = f.tick + 30;
            path = null;
        }
        k.strafe = 0;
        if (mode == NONE && heal == HEAL_NONE && !boosting) {
            k.forward = 0;
            k.jump = false;
            k.sprint = false;
            k.dyaw = 0f;
            k.dpitch = 0f;
            return;
        }
        if (rnd.nextInt(20) == 0) {
            // A hand is not the same every moment.
            gain = 0.45 + rnd.nextDouble() * 0.25;
        }
        double ey = aimSet ? yawTo(f, tx, tz) : 0.0, ep = aimSet ? pitchTo(f, tx, ty, tz) : 0.0;
        double g = gain, capY = Pilot.MAX_DYAW, capP = Pilot.MAX_DPITCH;
        if (mode == WAIT && heal == HEAL_NONE) {
            // Waiting: a slow look over to the chest, then the view is left alone.
            g = 0.15;
            capY = 3.0;
            capP = 2.0;
            if (Math.abs(ey) < 20.0) {
                ey = 0.0;
            }
            if (Math.abs(ep) < 15.0) {
                ep = 0.0;
            }
        }
        k.dyaw = (float) clamp(ey * g, -capY, capY);
        k.dpitch = (float) clamp(ep * g, -capP, capP);
        if (pitchAbs == pitchAbs) {
            // Down for a potion, up for the bow: quicker, still smooth.
            k.dpitch = (float) clamp((pitchAbs - f.pitch) * 0.6, -20.0, 20.0);
        }
        if (move) {
            k.forward = 1;
            k.sprint = true;
            k.jump = jumpUp || swim;
        } else {
            k.forward = 0;
            k.jump = mode == WAIT || mode == CLAIM ? false : f.collidedH && f.onGround && k.forward > 0;
            k.sprint = true;
        }
        if (surface) {
            // The head under water: up for air, whatever else goes on.
            k.jump = true;
        }
        if (k.forward == 0 && !(mode == ESCAPE && pearlThrown) && !boosting && heal != HEAL_POTION) {
            // Never quite still (an anti-AFK): every second or two a side key is tapped for a few ticks,
            // to a side where the ground is safe; the view is not turned for it.
            if (f.tick >= tapNext) {
                tapUntil = f.tick + 2 + rnd.nextInt(3);
                tapNext = tapUntil + 15 + rnd.nextInt(25);
                tapDir = rnd.nextBoolean() ? 1 : -1;
                if (!sideSafe(f, tapDir)) {
                    tapDir = -tapDir;
                }
            }
            if (f.tick < tapUntil && sideSafe(f, tapDir)) {
                k.strafe = tapDir;
            }
        }
        if (f.tick < dodgeUntil && heal != HEAL_POTION && !(mode == ESCAPE && pearlThrown) && sideSafe(f, dodgeDir)) {
            // Out of the way of an arrow.
            k.strafe = dodgeDir;
        }
        if (fightJump && !surface) {
            // Up and down in the fight: blows that land on the way down are critical.
            k.jump = true;
        }
        if (fightWalk) {
            k.sprint = false;
        }
    }
}
