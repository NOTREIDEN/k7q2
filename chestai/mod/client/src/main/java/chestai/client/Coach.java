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
    static final int NONE = 0, FIGHT = 1, WAIT = 2, CLAIM = 3, GUARD = 4, TRAVEL = 5, APPROACH = 6, EXPLORE = 7;
    static final int HEAL_NONE = 0, HEAL_POTION = 1, HEAL_APPLE = 2;
    private static final String[] NAMES = {"quieto", "pelea", "espera", "reclamo", "guardia", "viaje", "acercarse", "explorar"};

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

    private final Random rnd = new Random();
    private final Settings s;
    private final Planner planner;

    int mode = NONE, heal = HEAL_NONE;
    boolean boosting;
    private int target = -1;
    private double tx, ty, tz;
    private boolean aimSet, move, jumpUp, swim;
    private double pitchAbs = Double.NaN;
    private double gain = 0.55;

    private long lastPot = -1000, lastApple = -10000, lastBoost = -1000, boostFrom = -1;

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
            b.append("+manzana");
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
        swim = inWater(f);
        double d = hd(gx, gz, f.x, f.z);
        // Stuck: the way is planned again from here.
        if (hd(lastX, lastZ, f.x, f.z) > 1.0) {
            lastX = f.x;
            lastZ = f.z;
            movedAt = f.tick;
        }
        boolean stuck = f.tick - movedAt > 30;
        if (stuck) {
            path = null;
            movedAt = f.tick;
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

    /** The view clear up and ahead for an arrow shot up, and open ground ahead for the boost. */
    private boolean boostRoom(Frame f) {
        if (path == null || pi + 12 >= path.size()) {
            return false;
        }
        int[] far = path.get(Math.min(path.size() - 1, pi + 16));
        if (hd(far[0] + 0.5, far[2] + 0.5, f.x, f.z) < 11.0 || !planner.straight(f.x, f.y, f.z, far[0] + 0.5, far[2] + 0.5)) {
            return false;
        }
        double yr = Math.toRadians(f.yaw);
        double dx = -Math.sin(yr), dz = Math.cos(yr);
        int y0 = (int) Math.floor(f.y + 0.01);
        for (int i = 0; i <= 5; i++) {
            int bx = (int) Math.floor(f.x + dx * i), bz = (int) Math.floor(f.z + dz * i);
            for (int up = 2; up <= 2 + i; up++) {
                if (!planner.open(bx, y0 + up, bz)) {
                    return false;
                }
            }
        }
        return true;
    }

    /** Changes the network's choices {@code a} of this tick, before the pilot turns them into keys. */
    void before(Frame f, Memory mem, Pilot pilot, int[] a, Layout lay) {
        long t = f.tick;
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
        mode = NONE;
        heal = HEAL_NONE;
        aimSet = false;
        move = false;
        jumpUp = false;
        swim = false;
        pitchAbs = Double.NaN;
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
        boolean haveApple = lay.real[Pilot.GAPPLE] >= 0 && lay.gapples > 0;
        boolean potNeed = havePot && !eating && t - lastPot > POT_GAP
            && ((hp <= s.potHp && (mem.sinceHurt < 100 || close > 0)) || hp <= s.potHp * 0.6);
        boolean appleNeed = !potNeed && !eating && haveApple && t - lastApple > Math.round(s.appleWaitS * 20.0)
            && f.absorption < 0.5f && f.regenLeft == 0
            && ((hp <= s.appleHp && (mem.sinceHurt < 100 || close >= 2)) || (hp <= s.potHp && !havePot));
        if (drawing && (potNeed || appleNeed || !cand.isEmpty())) {
            // The bow is let go: something matters more.
            boosting = false;
            boostFrom = -1;
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
        } else if (appleNeed && !pilot.using) {
            heal = HEAL_APPLE;
            a[8] = 1 + Pilot.GAPPLE;
            a[7] = 1;
            lastApple = t;
        }

        // 2. Mobs.
        if (!cand.isEmpty()) {
            boosting = false;
            boostFrom = -1;
            Frame.Mob tgt = null;
            double bs = 1e9;
            for (Frame.Mob m : cand) {
                double r = reach(f, m);
                if (r > REACH) {
                    continue;
                }
                double sc = r + Math.abs(yawTo(f, m.x, m.z)) / 30.0 + (m.hurtTime > 0 ? 1.0 : 0.0) + (m.id == target ? -0.8 : 0.0);
                if (sc < bs) {
                    bs = sc;
                    tgt = m;
                }
            }
            double[] gc = groupCenter(f, cand);
            if (tgt != null) {
                mode = FIGHT;
                target = tgt.id;
                // Low on the body: the name a server keeps over a mob's head can be in the way higher up.
                double ax = tgt.x + tgt.mx * 2.0, az = tgt.z + tgt.mz * 2.0, ay = tgt.y + Math.min(tgt.h, 2.6) * 0.45;
                aim(ax, ay, az);
                if (heal == HEAL_NONE && !pilot.using && Math.abs(yawTo(f, ax, az)) < 25.0 && Math.abs(pitchTo(f, ax, ay, az)) < 35.0) {
                    a[6] = 1;
                }
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
            if (hd(k.cx(), k.cz(), f.x, f.z) > 3.0) {
                go(f, k.cx(), k.y, k.cz(), 2.0);
            } else {
                aim(k.cx(), eyeY(f), k.cz());
            }
        } else {
            // 3. Chests.
            int g = chooseGoal(f, mem);
            if (g >= 0) {
                Memory.Known k = mem.chests.get(g);
                double dc = hd(k.cx(), k.cz(), f.x, f.z);
                double ds = hd(standX, standZ, f.x, f.z);
                if (k.seen && k.state == Frame.CLAIMABLE && dc < 4.0) {
                    mode = CLAIM;
                    aim(k.cx(), k.cy(), k.cz());
                    a[7] = heal == HEAL_NONE && !pilot.using ? 1 : a[7];
                    if (dc > 2.5) {
                        move = true;
                        jumpUp = f.collidedH && f.onGround;
                    }
                } else if (k.seen && k.state == Frame.CLAIMABLE) {
                    mode = TRAVEL;
                    go(f, k.cx(), k.y, k.cz(), 2.0);
                } else if (ds < 2.0) {
                    mode = WAIT;
                    aim(k.cx(), k.cy() + 0.5, k.cz());
                } else {
                    mode = k.seen ? TRAVEL : EXPLORE;
                    go(f, standX, k.y, standZ, 1.5);
                    boost(f, pilot, a, lay, dc);
                }
            }
        }
        if (!boosting && heal == HEAL_NONE && !pilot.using && pilot.slot != Pilot.SWORD && lay.real[Pilot.SWORD] >= 0) {
            a[8] = 1 + Pilot.SWORD;
        }
    }

    /** Far to go on open ground: a Punch arrow shot up that carries the player along. */
    private void boost(Frame f, Pilot pilot, int[] a, Layout lay, double dist) {
        boolean can = s.bow && heal == HEAL_NONE && lay.real[Pilot.BOW] >= 0 && lay.arrows > 0 && f.punch >= 1 && !swim;
        if (boosting) {
            if (!can || f.tick - boostFrom > 40) {
                boosting = false;
                boostFrom = -1;
                return;
            }
        } else {
            if (!can || dist < 25.0 || f.tick - lastBoost < 60 || !f.onGround || !aimSet || Math.abs(yawTo(f, tx, tz)) > 10.0 || !boostRoom(f)) {
                return;
            }
            boosting = true;
            boostFrom = f.tick;
        }
        if (pilot.slot != Pilot.BOW) {
            a[8] = 1 + Pilot.BOW;
        } else if (!pilot.using) {
            if (f.pitch > -40.0f) {
                pitchAbs = -45.0;
            } else {
                a[7] = 1;
            }
            if (pitchAbs != pitchAbs) {
                pitchAbs = -45.0;
            }
        } else {
            pitchAbs = -45.0;
            if (pilot.useTicks < 5) {
                a[7] = 1;
            } else {
                // Let go: the arrow is off.
                a[7] = 0;
                lastBoost = f.tick;
                boosting = false;
                boostFrom = -1;
            }
        }
    }

    /** Changes the keys of this tick, after the pilot. */
    void after(Frame f, Pilot.Keys k) {
        if (k.throwIt && k.slot == Pilot.POTION) {
            lastPot = f.tick;
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
    }
}
