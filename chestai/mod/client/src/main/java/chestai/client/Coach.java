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
    static final int NONE = 0, FIGHT = 1, WAIT = 2, CLAIM = 3, GUARD = 4, TRAVEL = 5, APPROACH = 6, EXPLORE = 7, ESCAPE = 8, RETREAT = 9;
    static final int HEAL_NONE = 0, HEAL_POTION = 1, HEAL_APPLE = 2;
    private static final String[] NAMES = {"quieto", "pelea", "espera", "reclamo", "guardia", "viaje", "acercarse", "explorar", "escapar", "retirada"};

    /** A mob this close is fought; with a wave going on by the player, any of its mobs. */
    private static final double FIGHT_R = 12.0, WAVE_R = 24.0;
    /** A chest whose wave is on, this close, is not left until its mobs are dead. */
    private static final double LOCK_R = 40.0;
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
    private boolean fightJump, fightWalk;
    /**
     * When the last hits came (not its own arrow), and whether they come often (three in three
     * seconds: an area blow, arrows one after another): then on the ground and sprinting, as
     * knockback carries a player in the air far and one on the ground stops soon, and back into
     * the group at once.
     */
    private final long[] hits = {-1000, -1000, -1000, -1000, -1000, -1000, -1000, -1000};
    private int hitN;
    private boolean pressed;
    private static final boolean ADAPT = !"false".equals(System.getProperty("chestai.fight.adapt"));
    /**
     * Out of the fight to live: health low with nothing left to heal it (no potion, no apple), very
     * low with or without, or going fast. Away from the mobs to a spot twenty blocks off, eating
     * there, until health is back (or for a while, when nothing can bring it back), then on again.
     */
    private static final boolean RETREAT_ON = !"false".equals(System.getProperty("chestai.retreat"));
    /** As up to the last round: back at 14 (17) with the absorption counted, apples eaten while still chased. */
    private static final boolean RETREAT_OLD = Boolean.getBoolean("chestai.retreat.old");
    /**
     * Mobs gone for that the player got no nearer to in eight seconds (up on a cliff, down a pit,
     * across water it cannot get out of): left alone for half a minute unless they come in reach.
     */
    private final java.util.Map<Integer, Long> unreach = new java.util.HashMap<Integer, Long>();
    /**
     * Mobs in reach that no blow reaches (under leaves, behind a block, in a hole below): seven
     * seconds the target without a hurt (clicking) or out of a click's sight (not clicking, another
     * module hitting): left alone for half a minute, reach or not.
     */
    private final java.util.Map<Integer, Long> unhit = new java.util.HashMap<Integer, Long>();
    private int hitTgt = -1;
    private long hitSeen, hitVis;
    /**
     * In a fight on the rim of a drop of four or more (a pit, a cliff): a step to the nearest spot
     * off it, keeping the view on the mob, so a knock does not throw the player in. NaN when not.
     */
    private double rimX = Double.NaN, rimZ;
    /** Clicks on a chest to claim it (with nothing to say it was claimed), and since when waiting at one. */
    private int claimClicks;
    private long waitSince = -1;
    private double apprBest = 1e9;
    private long apprAt = -1000, apprLast = -1000;
    /** Mobs whose arrows (or fireballs, snowballs) came at the player, and when last. */
    private final java.util.Map<Integer, Long> shotAt = new java.util.HashMap<Integer, Long>();
    private static final boolean SHOOTERS = !"false".equals(System.getProperty("chestai.shooters"));
    /** A mob shooting at the player now (the nearest), and whether it is fought; strafing meanwhile. */
    private Frame.Mob shooter;
    private boolean evade, coverOn;
    private int evadeDir = 1;
    private long evadeNext, coverAt = -1000;
    private int[] cover;
    /** Drawing the bow at a shooter that cannot be got to, since when, and when the last arrow went. */
    private long shootFrom = -1, shootTick = -1000, lastShot = -1000;
    /** A potion, an apple or food wanted this tick, and the nearest mob: no bow then (it would hold the hand). */
    private boolean healWanted;
    private double nearestMob = 1e9;
    private final double[] lifeAgo = new double[40];
    private boolean lifeSet;
    private long retreatUntil = -1, retreatSpotAt = -10000;
    private double retreatExit;
    private int[] retreatSpot;

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
    /** Plain food being eaten (through the apple's slot), and since when. */
    private boolean foodBite;
    private long foodAt = -1000;

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
    /**
     * At the end of a way that stops short (a wall two or three high, a cliff) since when: a second
     * there and a wider search for a way up one block at a time; none, a pearl up there at once;
     * none of those either, the goal is left for another.
     */
    private long endSince = -1, widePlanAt = -10000, pearlTryAt = -10000, noPearlAt = -10000;
    private boolean deadEndNow;
    /** Guarding a wave none of whose mobs can be fought, since when: after twenty seconds that wave is left for a minute. */
    private long guardSince = -1;
    private static final boolean DEAD_END = !"false".equals(System.getProperty("chestai.deadend"));
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
        if (pressed && (mode == FIGHT || mode == APPROACH)) {
            b.append("+presion");
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
        if (f.tick < k.skipUntil) {
            // Tried long and not claimed (no chest block to click, walled in): another for now.
            return 1e9;
        }
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

    /**
     * The chest with a wave on, which is not left until its mobs are dead, or -1. Of several (one
     * set off on the way to another), the one whose wave came first: its time runs out first, so
     * its mobs are finished first, then the next one's. Two that came within a second: the nearer.
     */
    private static int waveLock(Frame f, Memory mem) {
        int lock = -1;
        long first = Long.MAX_VALUE;
        double ld = 1e9;
        for (int i = 0; i < mem.chests.size(); i++) {
            Memory.Known k = mem.chests.get(i);
            double d = hd(k.cx(), k.cz(), f.x, f.z);
            if (!k.seen || k.state != Frame.ACTIVE || f.tick - k.tick >= 200 || d >= LOCK_R || f.tick < k.skipUntil) {
                continue;
            }
            boolean sooner = lock < 0 || k.activeSeenAt < first - 20;
            boolean same = lock >= 0 && Math.abs(k.activeSeenAt - first) <= 20 && d < ld;
            if (sooner || same) {
                first = Math.min(first, k.activeSeenAt);
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
            if (partial && mode != APPROACH && mode != FIGHT) {
                // Stuck where the way ends short of the goal: a pit, a ravine, a pond with high banks.
                // (Not after mobs: those it cannot get to are left alone instead, no pearl after them.)
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
        int[] endN = path == null ? null : path.get(path.size() - 1);
        boolean atEnd = partial && (endN == null || hd(endN[0] + 0.5, endN[2] + 0.5, f.x, f.z) < 1.6);
        boolean away = hd(gx, gz, f.x, f.z) > tol + 2.5 || gy - f.y > 2.0;
        if (atEnd && away) {
            if (endSince < 0) {
                endSince = f.tick;
            }
        } else {
            endSince = -1;
        }
        deadEndNow = DEAD_END && endSince >= 0 && f.tick - endSince >= 20;
        if (deadEndNow && mode != FIGHT) {
            if (f.tick - widePlanAt > 200) {
                // Once in a while a search three times as wide: a way round or up a step at a time.
                widePlanAt = f.tick;
                planner.clear();
                List<int[]> wide = planner.plan(f.x, f.y, f.z, gx, gy, gz, tol, Planner.MAX_NODES * 3);
                if (wide != null && planner.reached) {
                    path = wide;
                    partial = false;
                    pi = 0;
                    pathAt = f.tick;
                    endSince = -1;
                    deadEndNow = false;
                }
            }
            if (deadEndNow && mode != APPROACH) {
                // No way up on foot: a pearl (after mobs, none: those are left alone instead).
                deadEnds = 2;
                deadAt = f.tick;
            }
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

    private double pearlScore(double x, double y, double z) {
        return hd(x, z, pgx, pgz) + 1.2 * Math.max(0.0, pgy - y) + 0.3 * Math.max(0.0, y - pgy);
    }

    /** {yaw, pitch} of a pearl that lands clearly nearer the goal of the way, out of the hole the player is in, or null. */
    private double[] pearlShot(Frame f) {
        planner.clear();
        double toGoal = Math.toDegrees(Math.atan2(-(pgx - f.x), pgz - f.z));
        // Below the goal counts more than across from it: out of a pit straight up is a way out.
        double best = pearlScore(f.x, f.y, f.z) - 3.0;
        double[] out = null;
        int y0 = (int) Math.floor(f.y + 0.01);
        for (int dyaw = -90; dyaw <= 90; dyaw += 15) {
            for (int pitch = -75; pitch <= 25; pitch += 2) {
                int[] l = pearlLands(f, toGoal + dyaw, pitch);
                if (l == null || hd(l[0] + 0.5, l[2] + 0.5, f.x, f.z) < 3.0 || l[1] < y0 - 1 || planner.water(l[0], l[1], l[2])) {
                    continue;
                }
                double d = pearlScore(l[0] + 0.5, l[1], l[2] + 0.5);
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

    /** Spots where a click did not reach the chest though it looked free, for the chest gone for; misses in a row; a hop. */
    private final java.util.Set<Integer> badCells = new java.util.HashSet<Integer>();
    private int claimMiss;
    private boolean claimHop;

    private static int cellKey(int x, int y, int z) {
        return (x * 73856093) ^ (y * 19349663) ^ (z * 83492791);
    }

    private static int cellKey(Frame f) {
        return cellKey((int) Math.floor(f.x), (int) Math.floor(f.y + 0.01), (int) Math.floor(f.z));
    }

    /** A spot within four blocks of the chest to stand at, from where a click reaches it, the nearest to the player; or null. */
    private int[] claimSpot(Frame f, Memory.Known k, long t) {
        int key = k.x * 31 + k.z * 17 + k.y;
        if (key == spotFor && t - spotAt < 20) {
            return spot;
        }
        if (key != spotFor) {
            badCells.clear();
            claimMiss = 0;
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
                    if (d >= best || badCells.contains(cellKey(x, y, z)) || !planner.stand(x, y, z)) {
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

    /** A drop of more than two blocks (or water) just ahead, where the player looks: no walking on blindly into it. */
    /**
     * The mobs to fight: with a wave on, those by its chest (twelve blocks: its archers too), or none
     * while they are out of sight and the chest is far (back to it first); else all close by.
     */
    private List<Frame.Mob> focus(Frame f, Memory mem, int lock, List<Frame.Mob> cand, long t) {
        if (lock < 0) {
            return cand;
        }
        Memory.Known k = mem.chests.get(lock);
        List<Frame.Mob> own = new ArrayList<Frame.Mob>();
        for (Frame.Mob m : f.mobs) {
            Long gone = unreach.get(m.id), nohit = unhit.get(m.id);
            // By its chest, or one that came out of it wherever it is now (knocked away, after the player).
            boolean of = (hd(m.x, m.z, k.cx(), k.cz()) < 12.0 && Math.abs(m.y - k.y) <= 8.0)
                || (mem.waveMobOf(m.id, k) && hd(m.x, m.z, f.x, f.z) < 30.0 && Math.abs(m.y - f.y) <= 10.0);
            if (of && (gone == null || gone <= t || reach(f, m) <= REACH) && (nohit == null || nohit <= t)) {
                own.add(m);
            }
        }
        if (!own.isEmpty()) {
            if (shooter != null && cand.contains(shooter) && !own.contains(shooter) && hd(shooter.x, shooter.z, f.x, f.z) < 8.0) {
                own.add(shooter);
            }
            return own;
        }
        return hd(k.cx(), k.cz(), f.x, f.z) > 12.0 ? own : cand;
    }

    /**
     * A spot within six blocks of (cx, cz) where the one shooting cannot see the player (a block
     * between its eyes and the body), the nearest; kept two seconds. Or null.
     */
    private int[] cover(Frame f, double cx, double cz, long t) {
        if (cover != null && t - coverAt < 40 && hd(cover[0] + 0.5, cover[2] + 0.5, cx, cz) < 7.0) {
            return cover;
        }
        coverAt = t;
        cover = null;
        Frame.Mob sh = shooter;
        double ex = sh.x, ey = sh.y + Math.max(sh.h, 1.0) * 0.85, ez = sh.z;
        int y0 = (int) Math.floor(f.y + 0.01), x0 = (int) Math.floor(cx), z0 = (int) Math.floor(cz);
        double bd = 1e9;
        for (int dx = -6; dx <= 6; dx++) {
            for (int dz = -6; dz <= 6; dz++) {
                if (dx * dx + dz * dz > 36) {
                    continue;
                }
                int x = x0 + dx, z = z0 + dz;
                int y = standY(x, z, y0);
                if (y == Integer.MIN_VALUE || Math.abs(y - y0) > 2 || planner.water(x, y, z)) {
                    continue;
                }
                double d = hd(x + 0.5, z + 0.5, f.x, f.z);
                if (d >= bd) {
                    continue;
                }
                // Hidden at the head and at the chest both.
                boolean hidden = !planner.see(ex, ey, ez, x + 0.5, y + 1.5, z + 0.5, Integer.MIN_VALUE, 0, 0)
                    && !planner.see(ex, ey, ez, x + 0.5, y + 0.9, z + 0.5, Integer.MIN_VALUE, 0, 0);
                if (hidden && mobNear(f, new int[] {x, y, z}) > 3.0) {
                    bd = d;
                    cover = new int[] {x, y, z};
                }
            }
        }
        return cover;
    }

    /** How near a projectile comes to the player's body in the next 25 ticks, and in how many: {near, when}. */
    private static double[] closest(Frame f, Frame.Proj p) {
        double vx = p.mx, vy = p.my, vz = p.mz;
        if (vx * vx + vy * vy + vz * vz < 0.09) {
            return new double[] {1e9, -1};
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
        return new double[] {near, when};
    }

    /** The nearest spot two blocks or less off a rim the player stands on, into rimX and rimZ (NaN if not on one). */
    private void rim(Frame f) {
        rimX = Double.NaN;
        if (!f.onGround || inWater(f)) {
            return;
        }
        int fx = (int) Math.floor(f.x), fy = (int) Math.floor(f.y + 0.01), fz = (int) Math.floor(f.z);
        if (!planner.brink(fx, fy, fz)) {
            return;
        }
        double best = 1e9;
        for (int dx = -2; dx <= 2; dx++) {
            for (int dz = -2; dz <= 2; dz++) {
                if (dx == 0 && dz == 0) {
                    continue;
                }
                for (int dy : new int[] {0, 1, -1}) {
                    int x = fx + dx, y = fy + dy, z = fz + dz;
                    if (planner.stand(x, y, z) && !planner.brink(x, y, z)) {
                        double d = dx * dx + dz * dz + Math.abs(dy);
                        if (d < best) {
                            best = d;
                            rimX = x + 0.5;
                            rimZ = z + 0.5;
                        }
                        break;
                    }
                }
            }
        }
    }

    /**
     * The pitch for an arrow from the eyes, fully drawn (3 blocks a tick, 1% drag, 0.05 down), to
     * come down on (hx across, dy up) from here: the flattest that does, or NaN.
     */
    private static double arrowPitch(double hx, double dy) {
        // Over this range the height where the arrow gets to hx only rises as the bow points up:
        // the nearest is the one.
        double best = Double.NaN, bm = 0.6;
        for (double p = -40.0; p <= 30.0; p += 0.25) {
            double pr = Math.toRadians(p), vx = Math.cos(pr) * 3.0, vy = -Math.sin(pr) * 3.0, x = 0.0, y = -0.1;
            for (int i = 0; i < 60 && x < hx; i++) {
                double nx = x + vx, ny = y + vy;
                if (nx >= hx) {
                    double yAt = y + (ny - y) * (hx - x) / (nx - x);
                    double miss = Math.abs(yAt - dy);
                    if (miss < bm) {
                        bm = miss;
                        best = p;
                    }
                }
                x = nx;
                y = ny;
                vx *= 0.99;
                vy = vy * 0.99 - 0.05;
            }
        }
        return best;
    }

    /**
     * An arrow at the one shooting, when it cannot be got to on foot and is in sight: the bow out,
     * drawn a second, aimed for the drop and let go. True while at it.
     */
    private boolean shootBack(Frame f, Pilot pilot, int[] a, Layout lay) {
        Frame.Mob m = shooter;
        // Never with healing wanted (a drawn bow keeps the hand from the apple and the potion), low,
        // or with a mob close by.
        boolean can = s.bowMobs && m != null && heal == HEAL_NONE && !healWanted && f.hp + f.absorption > 12.0f && nearestMob > 5.0
            && lay.real[Pilot.BOW] >= 0 && lay.arrows > 0 && !swim && !inWater(f) && f.tick - lastShot > 10;
        double hx = can ? hd(m.x, m.z, f.x, f.z) : 0.0;
        double ty = can ? m.y + Math.max(m.h, 1.0) * 0.6 : 0.0;
        can = can && hx > 4.0 && hx < 30.0 && Math.abs(ty - eyeY(f)) < 12.0
            && planner.see(f.x, eyeY(f), f.z, m.x, ty, m.z, Integer.MIN_VALUE, 0, 0);
        if (!can) {
            return false;
        }
        if (f.tick - shootTick > 1) {
            shootFrom = f.tick;
        }
        if (f.tick - shootFrom > 80) {
            // Four seconds and never on it: a rest before the next try.
            lastShot = f.tick;
            return false;
        }
        shootTick = f.tick;
        // Where it will be when the arrow gets there (about a tick for every three blocks).
        double lead = hx / 3.0;
        double ax = m.x + m.mx * lead, az = m.z + m.mz * lead;
        double p = arrowPitch(hd(ax, az, f.x, f.z), ty - eyeY(f));
        if (p != p) {
            shootTick = -1000;
            return false;
        }
        aim(ax, ty, az);
        pitchAbs = p;
        if (pilot.slot != Pilot.BOW) {
            a[8] = 1 + Pilot.BOW;
        } else if (!pilot.using || pilot.useTicks < 20) {
            a[7] = 1;
        } else if (Math.abs(yawTo(f, ax, az)) < 1.5 && Math.abs(f.pitch - p) < 1.5) {
            // Full draw and on it: let go.
            a[7] = 0;
            lastShot = f.tick;
        } else {
            a[7] = 1;
        }
        return true;
    }

    /** At a dead end on the way to chest k with no pearl to throw: another chest for a minute. */
    private void giveUp(Memory.Known k, long t) {
        if (deadEndNow && t - noPearlAt < 40) {
            k.skipUntil = t + 1200;
            hasGoal = false;
            goalAt = -1000;
            endSince = -1;
            deadEndNow = false;
        }
    }

    /** How far the nearest mob is from this spot, across. */
    private static double mobNear(Frame f, int[] at) {
        double d = 1e9;
        for (Frame.Mob m : f.mobs) {
            d = Math.min(d, hd(m.x, m.z, at[0] + 0.5, at[2] + 0.5));
        }
        return d;
    }

    /** A height a player can stand at in column (x, z), the nearest to y0 within eight, or MIN_VALUE. */
    private int standY(int x, int z, int y0) {
        for (int d = 0; d <= 8; d++) {
            if (planner.stand(x, y0 + d, z)) {
                return y0 + d;
            }
            if (d > 0 && planner.stand(x, y0 - d, z)) {
                return y0 - d;
            }
        }
        return Integer.MIN_VALUE;
    }

    /**
     * Where to get away to: of spots 9, 14 and 20 blocks off all round, the one
     * farthest from every mob, away from where they are, not much up or down,
     * dry. Or null.
     */
    private int[] retreatSpot(Frame f) {
        double wx = 0.0, wz = 0.0;
        for (Frame.Mob m : f.mobs) {
            double d = Math.max(hd(m.x, m.z, f.x, f.z), 1.0);
            if (d < 30.0) {
                wx += (m.x - f.x) / (d * d);
                wz += (m.z - f.z) / (d * d);
            }
        }
        boolean side = wx * wx + wz * wz > 1e-9;
        double away = Math.atan2(-wz, -wx);
        int y0 = (int) Math.floor(f.y + 0.01);
        planner.now(f.tick);
        int[] best = null;
        double bs = -1e9;
        for (int i = 0; i < 16; i++) {
            double a = i * Math.PI / 8.0;
            for (double r : new double[] {20.0, 14.0, 9.0}) {
                int x = (int) Math.floor(f.x + Math.cos(a) * r), z = (int) Math.floor(f.z + Math.sin(a) * r);
                int y = standY(x, z, y0);
                if (y == Integer.MIN_VALUE) {
                    continue;
                }
                int[] at = {x, y, z};
                double sc = Math.min(mobNear(f, at), 30.0) + r * 0.3 - Math.abs(y - y0) * 0.6
                    + (side ? Math.cos(a - away) * 4.0 : 0.0) - (planner.water(x, y, z) || planner.water(x, y - 1, z) ? 6.0 : 0.0);
                if (sc > bs) {
                    bs = sc;
                    best = at;
                }
            }
        }
        return best;
    }

    private boolean ledge(Frame f) {
        double yr = Math.toRadians(f.yaw);
        int x = (int) Math.floor(f.x - Math.sin(yr) * 0.9), z = (int) Math.floor(f.z + Math.cos(yr) * 0.9), y = (int) Math.floor(f.y + 0.01);
        if (inWater(f)) {
            return false;
        }
        return !(planner.stand(x, y, z) || planner.stand(x, y + 1, z) || planner.stand(x, y - 1, z) || planner.stand(x, y - 2, z));
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
            claimClicks = 0;
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
        claimHop = false;
        rimX = Double.NaN;
        double hp = f.hp;

        int lock = waveLock(f, mem);
        List<Frame.Mob> focus = null;
        double engage = lock >= 0 ? WAVE_R : FIGHT_R;
        List<Frame.Mob> cand = new ArrayList<Frame.Mob>();
        int close = 0;
        for (Frame.Mob m : f.mobs) {
            double d = hd(m.x, m.z, f.x, f.z);
            if (d < 8.0) {
                close++;
            }
            Long gone = unreach.get(m.id), nohit = unhit.get(m.id);
            if ((gone != null && gone > t && reach(f, m) > REACH) || (nohit != null && nohit > t)) {
                continue;
            }
            if (d <= engage && Math.abs(m.y - f.y) <= 8.0) {
                cand.add(m);
            }
        }
        // Who is shooting at the player: a projectile of a mob's that comes within two blocks.
        for (Frame.Proj p : f.projs) {
            if (SHOOTERS && p.owner >= 0 && (p.kind == Frame.ARROW || p.kind == Frame.SNOWBALL)) {
                double[] c = closest(f, p);
                if (c[0] < 2.0 && c[1] >= 0 && c[1] <= 25) {
                    shotAt.put(p.owner, t);
                }
            }
        }
        if (shotAt.size() > 64) {
            shotAt.values().removeIf(u -> t - u > 200);
        }
        shooter = null;
        evade = false;
        coverOn = false;
        double sd = 1e9;
        for (Frame.Mob m : f.mobs) {
            Long st = shotAt.get(m.id);
            double d = hd(m.x, m.z, f.x, f.z);
            if (st != null && t - st <= 80 && d < 30.0 && d < sd) {
                sd = d;
                shooter = m;
            }
        }
        if (shooter != null && sd <= 20.0 && Math.abs(shooter.y - f.y) <= 8.0 && !cand.contains(shooter)) {
            Long gone = unreach.get(shooter.id), nohit = unhit.get(shooter.id);
            if ((gone == null || gone <= t) && (nohit == null || nohit <= t)) {
                // One shooting from out there is gone for too, not only the ones close by.
                cand.add(shooter);
            }
        }
        boolean eating = pilot.using && pilot.slot == Pilot.GAPPLE;
        boolean drawing = pilot.using && pilot.slot == Pilot.BOW;

        // 1. Not dying comes first.
        boolean havePot = lay.real[Pilot.POTION] >= 0 && lay.potions > 0;
        int apples = lay.real[Pilot.GAPPLE] >= 0 ? lay.gapples : 0;
        boolean haveApple = apples > 0, haveFood = lay.food >= 0 && lay.foodCount > 0;
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
        boolean spare = apples > s.appleKeep;
        // (Its own Punch arrow, with no mob about, is no reason.)
        boolean ownArrow = t - lastBoost < 60 && nearest > 12.0;
        // With apples to spare: one as soon as a hit takes health down to appleHitHp (the absorption
        // full again). With few: only low, with no absorption left. Never all of them on arrows: then
        // there would be none for hunger and the player would starve.
        if (mem.sinceHurt == 0 && !ownArrow) {
            hits[hitN++ % hits.length] = t;
        }
        int recent = 0;
        for (long h : hits) {
            recent += t - h <= 60 ? 1 : 0;
        }
        pressed = ADAPT && recent >= 3;
        // Getting away with a mob right behind: away first (eating slows the walk to a crawl), the
        // apple when there is a step between, or at once when very low.
        boolean fleeing = !RETREAT_OLD && t < retreatUntil && nearest < 4.5 && hp + f.absorption > 5.0;
        boolean appleNeed = !potNeed && !eating && haveApple && appleGap && !fleeing
            && ((mem.sinceHurt < 60 && (s.appleOnHit && spare && !ownArrow ? hp <= s.appleHitHp : hp <= s.appleHp && f.absorption < 0.5f && f.regenLeft == 0))
                || (hp <= s.potHp && !havePot));
        // Food as soon as the bar is short: plain food first; an apple (4 of food) when 4 are missing,
        // or when the bar is so low the player can no longer sprint. Not with a claim at hand, nor with
        // a mob at the sword while the bar still lets the player sprint for long.
        boolean foodNeed = s.foodFull && !potNeed && !appleNeed && !eating && f.food < 20 && !claimNear && !fleeing
            && (nearest > 3.5 || f.food <= 14)
            && (haveFood || (haveApple && (f.food <= 6 || (f.food <= 16 && appleGap))));
        healWanted = potNeed || appleNeed || foodNeed;
        nearestMob = nearest;
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

        // 1b. Out of the fight to live: low with nothing left to heal (no potion, no apple), very low
        // anyway, or going fast with no potion. Until health is back (food at 18 or more brings it
        // back on its own), or for twenty seconds when nothing can.
        double life = hp + f.absorption;
        int li = (int) (t % lifeAgo.length);
        if (!lifeSet) {
            java.util.Arrays.fill(lifeAgo, life);
            lifeSet = true;
        }
        double lifeBefore = lifeAgo[li];
        lifeAgo[li] = life;
        boolean threat = nearest < 16.0 || mem.sinceHurt < 40;
        boolean regen = f.food >= 18 || haveFood || haveApple;
        if (RETREAT_ON && t >= retreatUntil && threat) {
            double exit = 0.0;
            if (RETREAT_OLD) {
                if (!havePot && !haveApple && life <= 11.0) {
                    exit = 17.0;
                } else if (life <= 6.0 || (!havePot && life <= 12.0 && lifeBefore - life >= 6.0)) {
                    exit = 14.0;
                }
            } else if (!havePot && !haveApple && life <= 14.0) {
                // Nothing left to heal with: out sooner, back only nearly full.
                exit = 18.0;
            } else if (life <= 7.0 || (life <= 13.0 && lifeBefore - life >= 6.0)) {
                // Very low, or six gone in two seconds (an area blow, arrows, all at once).
                exit = 16.0;
            }
            if (exit > 0.0) {
                retreatExit = exit;
                retreatUntil = t + (regen ? 1800 : 400);
                retreatSpot = null;
            }
        }
        if (t < retreatUntil && (RETREAT_OLD ? life : hp) >= retreatExit) {
            // Back by health, not by an apple's absorption: that is gone at the first blows.
            retreatUntil = -1;
        }
        boolean retreating = t < retreatUntil;
        for (Memory.Known k : mem.chests) {
            if (k.state != Frame.CLAIMABLE) {
                k.claimTicks = 0;
            }
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
        if (pearlUntil < t && deadEnds >= 2 && t - pearlTryAt >= 40) {
            pearlTryAt = t;
            double[] shot = pearlOk && t - pearlAt > 160 ? pearlShot(f) : null;
            deadEnds = 0;
            if (shot != null) {
                pearlYaw = shot[0];
                pearlPitch = shot[1];
                pearlUntil = t + 50;
                pearlThrown = false;
            } else {
                // No pearl, or none that lands nearer: the goal is given up for now (below).
                noPearlAt = t;
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
        } else if (retreating) {
            mode = RETREAT;
            evade = shooter != null;
            boosting = false;
            boostFrom = -1;
            target = -1;
            boolean there = retreatSpot != null && hd(retreatSpot[0] + 0.5, retreatSpot[2] + 0.5, f.x, f.z) < 2.0;
            if (retreatSpot == null || mobNear(f, retreatSpot) < 8.0 || (there && nearest < 12.0) || (!there && t - retreatSpotAt > 300)) {
                retreatSpot = retreatSpot(f);
                retreatSpotAt = t;
                there = false;
            }
            if (retreatSpot != null && !there) {
                go(f, retreatSpot[0] + 0.5, retreatSpot[1], retreatSpot[2] + 0.5, 1.5);
            } else if (retreatSpot == null) {
                // No spot known to stand at: straight away from the nearest mob.
                Frame.Mob nm = null;
                for (Frame.Mob m : f.mobs) {
                    if (nm == null || hd(m.x, m.z, f.x, f.z) < hd(nm.x, nm.z, f.x, f.z)) {
                        nm = m;
                    }
                }
                if (nm != null) {
                    double d = Math.max(hd(nm.x, nm.z, f.x, f.z), 0.5);
                    go(f, f.x + (f.x - nm.x) / d * 12.0, f.y, f.z + (f.z - nm.z) / d * 12.0, 2.0);
                }
            }
        } else if (!(focus = focus(f, mem, lock, cand, t)).isEmpty()) {
            // 3. Mobs: those of the wave that came first, or any about.
            boosting = false;
            boostFrom = -1;
            Frame.Mob tgt = null;
            double bs = 1e9, tgtY = Double.NaN;
            int melee = 0;
            for (Frame.Mob m : cand) {
                melee += m != shooter && hd(m.x, m.z, f.x, f.z) < 4.0 ? 1 : 0;
            }
            for (Frame.Mob m : cand) {
                double r = reach(f, m);
                if (r > REACH) {
                    continue;
                }
                // One just hit takes no harm for half a second: another one meanwhile. One behind a
                // block or tall grass, where a click would stop, only if there is no other.
                double vy = seenAt(f, m);
                // The first wave's own before the others; one shooting before one that is not.
                double sc = r + Math.abs(yawTo(f, m.x, m.z)) / 30.0 + (m.hurtTime > 2 ? 2.0 : 0.0) + (m.id == target && m.hurtTime <= 2 ? -0.8 : 0.0)
                    + (vy != vy ? 2.5 : 0.0) + (focus.contains(m) ? 0.0 : 2.0) + (m == shooter ? -1.0 : 0.0);
                if (sc < bs) {
                    bs = sc;
                    tgt = m;
                    tgtY = vy;
                }
            }
            double[] gc = groupCenter(f, focus);
            if (shooter != null && focus.contains(shooter) && melee == 0) {
                // Only arrows coming, nobody at the sword: straight at the one shooting, strafing.
                gc = new double[] {shooter.x, shooter.y, shooter.z};
            }
            evade = shooter != null;
            objective(gc[0], gc[1], gc[2], t);
            if (tgt != null) {
                mode = FIGHT;
                target = tgt.id;
                if (tgt.id != hitTgt) {
                    hitTgt = tgt.id;
                    hitSeen = t;
                    hitVis = t;
                }
                if (tgt.hurtTime > 0 || (!s.hit && tgtY == tgtY)) {
                    hitSeen = t;
                }
                if (tgtY == tgtY) {
                    hitVis = t;
                }
                // Seven seconds without a hurt; four when not once in sight of a click either.
                if (t - hitSeen > 140 || (t - hitSeen > 80 && t - hitVis > 80)) {
                    if (unhit.size() > 64) {
                        unhit.values().removeIf(u -> u <= t);
                    }
                    unhit.put(tgt.id, t + 600);
                    hitTgt = -1;
                }
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
                    if (f.pickMob < 0 && tgtY == tgtY && Math.abs(yawTo(f, ax, az)) < 8.0 && Math.abs(pitchTo(f, ax, ay, az)) < 10.0) {
                        ok = true;
                    }
                    if (ok) {
                        a[6] = 1;
                    }
                }
                rim(f);
                // Shot at: sprinting and strafing, not hopping on the spot.
                fightJump = s.fightJump && !pressed && shooter == null && f.onGround && !inWater(f);
                fightWalk = s.fightJump && !pressed && shooter == null && !Boolean.getBoolean("chestai.fight.sprint");
                if (hd(gc[0], gc[2], f.x, f.z) > 1.8 && hd(tgt.x, tgt.z, f.x, f.z) > 1.2 && !ledge(f)) {
                    move = true;
                    jumpUp = (f.collidedH && f.onGround) || inWater(f);
                }
            } else {
                // None in reach: straight at the group, without swinging.
                mode = APPROACH;
                target = -1;
                double dg = hd(gc[0], gc[2], f.x, f.z) + Math.abs(gc[1] - f.y);
                if (t - apprLast > 20 || dg < apprBest - 1.0) {
                    apprBest = dg;
                    apprAt = t;
                }
                apprLast = t;
                if (t - apprAt > 160) {
                    if (unreach.size() > 64) {
                        unreach.values().removeIf(u -> u <= t);
                    }
                    for (Frame.Mob m : focus) {
                        if (hd(m.x, m.z, gc[0], gc[2]) < 5.0) {
                            unreach.put(m.id, t + 600);
                        }
                    }
                    apprBest = 1e9;
                    apprAt = t;
                }
                go(f, gc[0], gc[1], gc[2], 1.5);
                aim(tx, gc[1] + 1.0, tz);
                if (deadEndNow) {
                    // The way to them stops short (up a cliff, down a pit): left alone now, not in eight seconds.
                    for (Frame.Mob m : focus) {
                        if (hd(m.x, m.z, gc[0], gc[2]) < 5.0) {
                            unreach.put(m.id, t + 600);
                        }
                    }
                    endSince = -1;
                    deadEndNow = false;
                }
            }
        } else if (lock >= 0) {
            // The wave's mobs are out of sight for now: back to its chest and wait there.
            mode = GUARD;
            Memory.Known k = mem.chests.get(lock);
            if (lastMode != GUARD || guardSince < 0 || t - shootTick <= 2) {
                // (Not while its archer is being shot at.)
                guardSince = t;
            }
            if (t - guardSince > 400 || (deadEndNow && t - noPearlAt < 40)) {
                // Its mobs out of reach or out of sight all this while (or its chest cannot be got to):
                // other chests for a minute, then back.
                k.skipUntil = t + 1200;
                guardSince = -1;
            }
            boolean shot = shooter != null && shootBack(f, pilot, a, lay);
            int[] cv = shooter != null && !shot ? cover(f, k.cx(), k.cz(), t) : null;
            if (shot) {
                coverOn = false;
            } else if (cv != null) {
                coverOn = true;
                if (hd(cv[0] + 0.5, cv[2] + 0.5, f.x, f.z) > 0.6) {
                    go(f, cv[0] + 0.5, cv[1], cv[2] + 0.5, 0.4);
                } else {
                    aim(k.cx(), eyeY(f), k.cz());
                }
            } else if (hd(k.cx(), k.cz(), f.x, f.z) > (lastMode == GUARD ? 4.5 : 3.0)) {
                go(f, k.cx(), k.y, k.cz(), 2.0);
            } else {
                aim(k.cx(), eyeY(f), k.cz());
            }
            evade = shooter != null && cv == null;
            if (shot) {
                evade = true;
            }
        } else {
            // 4. Chests.
            int g = chooseGoal(f, mem);
            int[] cv;
            if (g >= 0) {
                Memory.Known k = mem.chests.get(g);
                double dc = hd(k.cx(), k.cz(), f.x, f.z);
                double ds = hd(standX, standZ, f.x, f.z);
                if (k.seen && k.state == Frame.CLAIMABLE && dc < 4.0) {
                    mode = CLAIM;
                    if (claimSince < 0) {
                        claimSince = t;
                    }
                    if (k.guessed && k.claimTicks > 60 && claimClicks > 20) {
                        // No hologram to say it was claimed: clicked on a while, taken as done (the next
                        // wave as far off as its hologram said last time).
                        mem.claimedGuess(k, t);
                        claimClicks = 0;
                        hasGoal = false;
                        goalAt = -1000;
                    }
                    if (++k.claimTicks > 600) {
                        // Thirty seconds at it and no wave: left aside for a minute.
                        k.claimTicks = 0;
                        k.skipUntil = t + 1200;
                        hasGoal = false;
                        goalAt = -1000;
                    }
                    long tried = t - claimSince;
                    // A point of the chest a click reaches from here (nothing in between, not even tall
                    // grass); with none, to a spot near it from where one is.
                    int here = cellKey(f);
                    double[] lid = badCells.contains(here) ? null : lidPoint(f, k, tried);
                    if (lid != null) {
                        aim(lid[0], lid[1], lid[2]);
                        // Aimed, and still not on the chest (a corner, a step, something the map does not
                        // have): a hop for another height, then another spot.
                        boolean aimed = Math.abs(yawTo(f, lid[0], lid[2])) < 2.5 && Math.abs(pitchTo(f, lid[0], lid[1], lid[2])) < 2.5;
                        claimMiss = f.pickChest >= 0 ? 0 : aimed ? claimMiss + 1 : claimMiss;
                        claimHop = claimMiss > 15 && f.onGround;
                        if (claimMiss > 40) {
                            badCells.add(here);
                            claimMiss = 0;
                            spotAt = -1000;
                        }
                    } else {
                        int[] at = claimSpot(f, k, t);
                        if (at != null) {
                            go(f, at[0] + 0.5, at[1], at[2] + 0.5, 0.4);
                        } else {
                            go(f, k.cx(), k.y, k.cz(), 1.0);
                        }
                    }
                    a[7] = heal == HEAL_NONE && !pilot.using ? 1 : a[7];
                    if (a[7] == 1 && f.pickChest >= 0) {
                        claimClicks++;
                    }
                    if (tried > 240) {
                        claimSince = -1;
                    }
                } else if (k.seen && k.state == Frame.CLAIMABLE) {
                    mode = TRAVEL;
                    go(f, k.cx(), k.y, k.cz(), 2.0);
                    giveUp(k, t);
                } else if (shooter != null && ds < 8.0 && shootBack(f, pilot, a, lay)) {
                    // An arrow back at one shooting from where it cannot be got to.
                    mode = WAIT;
                    evade = true;
                } else if (shooter != null && ds < 8.0 && (cv = cover(f, standX, standZ, t)) != null) {
                    // Arrows from one that cannot be got at: the wait out of its sight, behind something.
                    mode = WAIT;
                    coverOn = true;
                    if (hd(cv[0] + 0.5, cv[2] + 0.5, f.x, f.z) > 0.6) {
                        go(f, cv[0] + 0.5, cv[1], cv[2] + 0.5, 0.4);
                    } else {
                        aim(k.cx(), k.cy() + 0.5, k.cz());
                    }
                } else if (ds < (lastMode == WAIT ? 3.0 : 2.0)) {
                    mode = WAIT;
                    evade = shooter != null;
                    aim(k.cx(), k.cy() + 0.5, k.cz());
                    if (lastMode != WAIT || waitSince < 0) {
                        waitSince = t;
                    }
                    if (k.guessed && t - waitSince > 300) {
                        // Waited for a wave on a guess (no hologram): not yet, other chests meanwhile.
                        mem.claimedGuess(k, t);
                        waitSince = -1;
                        hasGoal = false;
                        goalAt = -1000;
                    }
                } else {
                    mode = k.seen ? TRAVEL : EXPLORE;
                    go(f, standX, k.y, standZ, 1.5);
                    giveUp(k, t);
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
            if (deadEndNow && t - noPearlAt < 40) {
                wanderAt = -10000;
            }
        }
        if (!boosting && t != shootTick && heal == HEAL_NONE && !pilot.using && pilot.slot != Pilot.SWORD && lay.real[Pilot.SWORD] >= 0) {
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
        boolean busy = mode == WAIT || mode == CLAIM || mode == GUARD || mode == ESCAPE || mode == RETREAT || mode == FIGHT || mode == APPROACH
            || heal != HEAL_NONE || t - objAt > 2;
        if (busy || hd(trapX, trapZ, f.x, f.z) > 4.0) {
            trapX = f.x;
            trapZ = f.z;
            trapAt = t;
            return;
        }
        if (t - trapAt > (DEAD_END ? 100 : 240) && (hd(objX, objZ, f.x, f.z) > 5.0 || objY - f.y > 2.5)) {
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
            double[] c = closest(f, p);
            double near = c[0];
            int when = (int) c[1];
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
        double g = mode == FIGHT ? 0.85 : gain, capY = Pilot.MAX_DYAW, capP = Pilot.MAX_DPITCH;
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
        boolean strafing = false;
        if (evade && heal != HEAL_POTION && !(mode == ESCAPE && pearlThrown) && !boosting && mode != CLAIM) {
            // Shot at: never a still mark. Side to side (with the forward key, sprinting, when it
            // goes somewhere), the side changed every half second or so, never off safe ground.
            if (f.tick >= evadeNext) {
                evadeDir = -evadeDir;
                evadeNext = f.tick + 8 + rnd.nextInt(10);
            }
            if (!sideSafe(f, evadeDir)) {
                evadeDir = -evadeDir;
            }
            if (sideSafe(f, evadeDir)) {
                k.strafe = evadeDir;
                strafing = true;
            }
        }
        if (!strafing && k.forward == 0 && !(mode == ESCAPE && pearlThrown) && !boosting && heal != HEAL_POTION) {
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
        if (mode == FIGHT && rimX == rimX && heal != HEAL_POTION) {
            // Off the rim, the view left on the mob: the keys toward the spot as seen from where it looks.
            double yr = Math.toRadians(f.yaw);
            double dx = rimX - f.x, dz = rimZ - f.z, len = Math.max(Math.sqrt(dx * dx + dz * dz), 1e-6);
            double ahead = (-Math.sin(yr) * dx + Math.cos(yr) * dz) / len, left = (Math.cos(yr) * dx + Math.sin(yr) * dz) / len;
            k.forward = ahead > 0.35 ? 1 : ahead < -0.35 ? -1 : 0;
            k.strafe = left > 0.35 ? 1 : left < -0.35 ? -1 : 0;
            k.sprint = k.forward > 0;
            k.jump = false;
            return;
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
        if (claimHop && mode == CLAIM) {
            k.jump = true;
        }
    }
}
