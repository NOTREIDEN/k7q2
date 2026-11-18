package chestai.client;

import java.util.Random;
import chestai.core.Frame;
import chestai.core.Memory;
import chestai.core.Pilot;

/**
 * The plain parts of farming, done the same way every time on top of what
 * the network picks: in a fight the view is turned smoothly onto the
 * nearest mob (within the same turn limits as always) and the sword is
 * swung only when it is in reach; by a chest whose wave is about to come
 * the player stands still and waits; a chest that can be claimed close by
 * is turned to and clicked. Pearls are kept for going far, never thrown
 * at the ground next to a chest. Everything else (going from chest to
 * chest, the bow, apples) is the network's.
 */
final class Coach {
    static final int NONE = 0, FIGHT = 1, WAIT = 2, CLAIM = 3;

    /** A mob this close is fought. */
    private static final double FIGHT_R = 8.0;
    /** Reach of the sword, from the eyes to the mob's middle. */
    private static final double REACH = 3.2;
    /** A wave this far off at most is waited for by its chest. */
    private static final double WAIT_SECS = 90.0;

    private final Random rnd = new Random();
    int mode = NONE;
    private int target = -1;
    private double tx, ty, tz;
    private double gain = 0.55;

    private static double wrap(double a) {
        return ((a % 360.0) + 540.0) % 360.0 - 180.0;
    }

    private static double eyeY(Frame f) {
        return f.y + 1.62;
    }

    private double yawTo(Frame f, double x, double z) {
        return wrap(Math.toDegrees(Math.atan2(-(x - f.x), z - f.z)) - f.yaw);
    }

    private double pitchTo(Frame f, double x, double y, double z) {
        double dx = x - f.x, dz = z - f.z;
        return -Math.toDegrees(Math.atan2(y - eyeY(f), Math.sqrt(dx * dx + dz * dz))) - f.pitch;
    }

    private Frame.Mob pick(Frame f) {
        Frame.Mob best = null, kept = null;
        double bs = 1e9;
        for (Frame.Mob m : f.mobs) {
            double dx = m.x - f.x, dz = m.z - f.z, dy = m.y - f.y;
            double d = Math.sqrt(dx * dx + dz * dz);
            if (d > FIGHT_R || Math.abs(dy) > 5.0) {
                continue;
            }
            double s = d + Math.abs(yawTo(f, m.x, m.z)) / 45.0;
            if (m.id == target) {
                kept = m;
                s -= 1.5;
            }
            if (s < bs) {
                bs = s;
                best = m;
            }
        }
        return best != null ? best : kept;
    }

    /** Changes the network's choices {@code a} of this tick, before the pilot turns them into keys. */
    void before(Frame f, Memory mem, Pilot pilot, int[] a) {
        mode = NONE;
        int want = a[8] == 0 ? -1 : a[8] - 1;
        Frame.Mob m = pick(f);
        boolean anyMobNear = false;
        for (Frame.Mob o : f.mobs) {
            double dx = o.x - f.x, dz = o.z - f.z;
            if (dx * dx + dz * dz < 16.0 * 16.0) {
                anyMobNear = true;
            }
        }
        // The chest nearest by and what it is up to.
        int near = -1;
        double nd = 1e9;
        boolean claimableFar = false;
        for (int i = 0; i < mem.chests.size(); i++) {
            Memory.Known k = mem.chests.get(i);
            if (!k.seen) {
                continue;
            }
            double dx = k.cx() - f.x, dz = k.cz() - f.z;
            double d = Math.sqrt(dx * dx + dz * dz);
            if (d < nd) {
                nd = d;
                near = i;
            }
        }
        for (int i = 0; i < mem.chests.size(); i++) {
            Memory.Known k = mem.chests.get(i);
            double dx = k.cx() - f.x, dz = k.cz() - f.z;
            if (i != near && k.seen && k.state == Frame.CLAIMABLE && dx * dx + dz * dz < 40.0 * 40.0) {
                claimableFar = true;
            }
        }
        Memory.Known nk = near >= 0 ? mem.chests.get(near) : null;

        if (m != null) {
            mode = FIGHT;
            target = m.id;
            tx = m.x + m.mx * 2.0;
            tz = m.z + m.mz * 2.0;
            ty = m.y + Math.min(m.h, 2.6) * 0.6;
            double dx = tx - f.x, dz = tz - f.z, dy = ty - eyeY(f);
            double d3 = Math.sqrt(dx * dx + dy * dy + dz * dz) - m.hw;
            // Swing only with the mob in reach and in front.
            a[6] = d3 <= REACH && Math.abs(yawTo(f, tx, tz)) < 30.0 ? 1 : 0;
            // The sword in hand, unless an apple is wanted with little health left.
            boolean eat = (want == Pilot.GAPPLE && f.hp < 12.0f) || pilot.using && pilot.slot == Pilot.GAPPLE;
            if (!eat && want != Pilot.POTION && (pilot.slot != Pilot.SWORD || (want >= 0 && want != Pilot.SWORD))) {
                a[8] = 1 + Pilot.SWORD;
            }
            if (!eat) {
                a[7] = 0;
            }
            a[2] = 0;
        } else if (nk != null && !anyMobNear && !claimableFar && nk.state == Frame.CLAIMABLE && nd < 6.0) {
            mode = CLAIM;
            tx = nk.cx();
            ty = nk.cy();
            tz = nk.cz();
            a[6] = 0;
            a[7] = 1;
            a[2] = 0;
            a[8] = 0;
        } else if (nk != null && !anyMobNear && !claimableFar && nd < 8.0
            && (nk.state == Frame.READY || nk.state == Frame.ACTIVE || (nk.state == Frame.COOLDOWN && mem.estSecs(near, f.tick) <= WAIT_SECS))) {
            mode = WAIT;
            tx = nk.cx();
            ty = nk.cy() + 0.5;
            tz = nk.cz();
            a[0] = 1;
            a[1] = 1;
            a[2] = 0;
            a[3] = 0;
            a[6] = 0;
            a[7] = 0;
            a[8] = pilot.slot == Pilot.SWORD ? 0 : 1 + Pilot.SWORD;
        } else {
            target = -1;
        }
        // A pearl only to go far: not with mobs about, not by a chest, not at the ground.
        if (want == Pilot.PEARL && (mode != NONE || anyMobNear || f.pitch > 25.0f || (nk != null && nd < 15.0))) {
            a[8] = 0;
        }
    }

    /** Changes the keys of this tick, after the pilot. */
    void after(Frame f, Pilot.Keys k) {
        if (mode == NONE) {
            return;
        }
        double ey = yawTo(f, tx, tz), ep = pitchTo(f, tx, ty, tz);
        if (rnd.nextInt(20) == 0) {
            // A hand is not the same every moment.
            gain = 0.45 + rnd.nextDouble() * 0.25;
        }
        double g = gain, capY = Pilot.MAX_DYAW, capP = Pilot.MAX_DPITCH;
        if (mode == WAIT) {
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
        k.dyaw = (float) Math.max(-capY, Math.min(capY, ey * g));
        k.dpitch = (float) Math.max(-capP, Math.min(capP, ep * g));
        if (mode == FIGHT) {
            double dx = tx - f.x, dz = tz - f.z;
            double d = Math.sqrt(dx * dx + dz * dz);
            if (d > 2.6) {
                k.forward = 1;
                k.sprint = true;
            } else if (k.forward > 0 && d < 1.4) {
                k.forward = 0;
            }
            // Over a step or a block in the way, not for crits.
            k.jump = f.collidedH && f.onGround;
        } else if (mode == WAIT) {
            k.forward = 0;
            k.strafe = 0;
            k.jump = false;
            k.sprint = false;
        } else if (mode == CLAIM) {
            double dx = tx - f.x, dz = tz - f.z;
            k.forward = dx * dx + dz * dz > 3.0 * 3.0 ? 1 : 0;
            k.strafe = 0;
            k.jump = f.collidedH && f.onGround;
        }
    }
}
