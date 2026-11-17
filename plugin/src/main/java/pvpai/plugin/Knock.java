package pvpai.plugin;

/**
 * The velocity a hit sends to the player that was hit. The same arithmetic
 * as the simulator's server (sim/src/server.rs: knock_back and the sprint
 * part of use_entity_attack, and plugin_velocity), checked against it hit
 * by hit (KnockCheck).
 */
public final strictfp class Knock {
    /** The game's table of sines (MathHelper). */
    private static final float[] SIN = new float[65536];

    static {
        for (int i = 0; i < 65536; i++) {
            SIN[i] = (float) Math.sin((double) i * Math.PI * 2.0 / 65536.0);
        }
    }

    private Knock() {
    }

    static float sin(float v) {
        return SIN[(int) (v * 10430.378F) & 65535];
    }

    static float cos(float v) {
        return SIN[(int) (v * 10430.378F + 16384.0F) & 65535];
    }

    /**
     * The game's own way with a profile's numbers: what becomes of the
     * motion the victim had (x, y, z) when hit by an attacker at (ax, az)
     * that looks along yaw, with so many levels of sprint and Knockback.
     * The two must not stand on the very same spot.
     */
    public static double[] game(Profile p, double[] motion, double ax, double az, double vx, double vz, float yaw, int level) {
        double x = ax - vx, z = az - vz;
        float f = (float) Math.sqrt(x * x + z * z);
        double mx = motion[0] / p.friction, my = motion[1] / p.friction, mz = motion[2] / p.friction;
        mx -= x / (double) f * p.horizontal;
        my += p.vertical;
        mz -= z / (double) f * p.horizontal;
        if (my > p.verticalLimit) {
            my = p.verticalLimit;
        }
        if (level > 0) {
            mx += (double) (-sin(yaw * 3.1415927F / 180.0F) * (float) level * p.extraHorizontal);
            my += p.extraVertical;
            mz += (double) (cos(yaw * 3.1415927F / 180.0F) * (float) level * p.extraHorizontal);
        }
        return new double[] {mx, my, mz};
    }

    /**
     * From the hit alone: the victim at (vx, vy, vz), on the ground or not
     * as its client last said, hit by an attacker at (ax, az). touchedTop
     * is whether the victim went over the height limit since it last
     * stood; it is set here when it does now.
     */
    public static double[] hit(Profile p, double ax, double az, double vx, double vy, double vz, boolean ground, float yaw, int level,
                               double lastGroundY, boolean[] touchedTop) {
        double dx = vx - ax, dz = vz - az;
        double dist = Math.sqrt(dx * dx + dz * dz);
        double h = ground ? p.hGround : p.hAir;
        double[] v = {dx / dist * h, ground ? p.vGround : p.vAir, dz / dist * h};
        if (level > 0) {
            double rad = (double) (yaw * 0.017453292F);
            v[0] -= Math.sin(rad) * (double) level * p.hSprint;
            v[1] += p.vSprint;
            v[2] += Math.cos(rad) * (double) level * p.hSprint;
        }
        if (p.yLimit >= 0.0) {
            if (vy - lastGroundY > p.yLimit) {
                touchedTop[0] = true;
            }
            if (touchedTop[0]) {
                v[1] = p.vAfterLimit;
            }
        }
        return v;
    }

    /** A velocity as the packet carries it: 8000ths of a block per tick. */
    public static int[] sent(double[] v) {
        int[] r = new int[3];
        for (int a = 0; a < 3; a++) {
            double c = v[a] < -3.9 ? -3.9 : v[a] > 3.9 ? 3.9 : v[a];
            r[a] = (int) (c * 8000.0);
        }
        return r;
    }
}
