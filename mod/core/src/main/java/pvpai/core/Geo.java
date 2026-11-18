package pvpai.core;

/**
 * The game's own arithmetic, as far as an agent needs it: the sine table of
 * {@code MathHelper}, boxes, and how a thrown pearl or potion flies.
 */
public final strictfp class Geo {
    public static final double EYE_HEIGHT = (double) 1.62f;
    private static final float[] SIN = new float[65536];
    static final float PI_F = (float) Math.PI;

    static {
        for (int i = 0; i < 65536; i++) {
            SIN[i] = (float) Math.sin((double) i * Math.PI * 2.0 / 65536.0);
        }
    }

    static float sin(float v) {
        return SIN[(int) (v * 10430.378f) & 65535];
    }

    static float cos(float v) {
        return SIN[(int) (v * 10430.378f + 16384.0f) & 65535];
    }

    static double wrapDeg(double a) {
        a = a % 360.0;
        if (a >= 180.0) {
            a -= 360.0;
        }
        if (a < -180.0) {
            a += 360.0;
        }
        return a;
    }

    /** {@code MathHelper.floor_double}. */
    public static int floor(double v) {
        int i = (int) v;
        return v < (double) i ? i - 1 : i;
    }

    static double clamp(double v, double lo, double hi) {
        return v < lo ? lo : v > hi ? hi : v;
    }

    static double distSq(double[] a, double[] b) {
        double x = b[0] - a[0], y = b[1] - a[1], z = b[2] - a[2];
        return x * x + y * y + z * z;
    }

    static double dist(double[] a, double[] b) {
        return Math.sqrt(distSq(a, b));
    }

    /** Yaw and pitch in degrees that point from eye at p. */
    static double[] anglesTo(double[] eye, double[] p) {
        double dx = p[0] - eye[0], dy = p[1] - eye[1], dz = p[2] - eye[2];
        double h = Math.sqrt(dx * dx + dz * dz);
        double deg = 180.0 / Math.PI;
        return new double[] {Math.atan2(-dx, dz) * deg, -(Math.atan2(dy, h) * deg)};
    }

    /** {@code Vec3.getIntermediateWith?Value}. */
    private static double[] intermediate(double[] a, double[] b, int axis, double value) {
        double[] d = {b[0] - a[0], b[1] - a[1], b[2] - a[2]};
        if (d[axis] * d[axis] < 1.0000000116860974E-7) {
            return null;
        }
        double t = (value - a[axis]) / d[axis];
        if (t >= 0.0 && t <= 1.0) {
            return new double[] {a[0] + d[0] * t, a[1] + d[1] * t, a[2] + d[2] * t};
        }
        return null;
    }

    /** {@code AxisAlignedBB.calculateIntercept}: where the line from a to b enters the box, or null. */
    public static double[] intercept(double[] min, double[] max, double[] a, double[] b) {
        double[] best = null;
        double bestD = 0.0;
        for (int axis = 0; axis < 3; axis++) {
            int u = axis == 0 ? 1 : 0, w = axis == 2 ? 1 : 2;
            for (int side = 0; side < 2; side++) {
                double[] p = intermediate(a, b, axis, side == 0 ? min[axis] : max[axis]);
                if (p != null && p[u] >= min[u] && p[u] <= max[u] && p[w] >= min[w] && p[w] <= max[w]) {
                    double d = distSq(a, p);
                    if (best == null || d < bestD) {
                        best = p;
                        bestD = d;
                    }
                }
            }
        }
        return best;
    }

    static double gravity(int kind) {
        return kind == Inv.KIND_PEARL ? (double) 0.03f : (double) 0.05f;
    }

    static double speed(int kind) {
        return kind == Inv.KIND_PEARL ? 1.5 : 0.5;
    }

    /**
     * {@code new EntityThrowable(world, thrower)} up to the random part of
     * the throw: where a pearl or a potion thrown by a player standing at
     * feet with this view starts (the first three numbers) and the
     * direction it is thrown in, of length one (the last three).
     */
    static double[] throwHeading(int kind, double[] feet, float yaw, float pitch) {
        float rad = PI_F;
        double x = feet[0], y = feet[1] + EYE_HEIGHT, z = feet[2];
        x -= (double) (cos(yaw / 180.0f * rad) * 0.16f);
        y -= 0.10000000149011612;
        z -= (double) (sin(yaw / 180.0f * rad) * 0.16f);
        float f = 0.4f;
        float off = kind == Inv.KIND_PEARL ? 0.0f : -20.0f;
        double mx = (double) (-sin(yaw / 180.0f * rad) * cos(pitch / 180.0f * rad) * f);
        double mz = (double) (cos(yaw / 180.0f * rad) * cos(pitch / 180.0f * rad) * f);
        double my = (double) (-sin((pitch + off) / 180.0f * rad) * f);
        double len = (double) (float) Math.sqrt(mx * mx + my * my + mz * mz);
        return new double[] {x, y, z, mx / len, my / len, mz / len};
    }

    /**
     * Where a pearl or a potion at pos moving by m per tick stops if it
     * flies on undisturbed (the first three numbers: its place at the start
     * of the tick in which it reaches a block) and after how many ticks
     * (the fourth).
     */
    static double[] flight(Blocks blocks, int kind, double[] pos, double[] m) {
        double f2 = (double) 0.99f, f3 = gravity(kind);
        double[] p = pos.clone(), v = m.clone();
        for (int t = 0; t < 100; t++) {
            double[] next = {p[0] + v[0], p[1] + v[1], p[2] + v[2]};
            if (blocks.rayTrace(p, next) != null) {
                return new double[] {p[0], p[1], p[2], t};
            }
            p = next;
            for (int a = 0; a < 3; a++) {
                v[a] *= f2;
            }
            v[1] -= f3;
        }
        return new double[] {p[0], p[1], p[2], 100};
    }

    /** {@link #flight} for a pearl a player standing at feet with this view would throw. */
    static double[] pearlPreview(Blocks blocks, double[] feet, float yaw, float pitch) {
        double[] h = throwHeading(Inv.KIND_PEARL, feet, yaw, pitch);
        double v = speed(Inv.KIND_PEARL);
        return flight(blocks, Inv.KIND_PEARL, new double[] {h[0], h[1], h[2]}, new double[] {h[3] * v, h[4] * v, h[5] * v});
    }

    /**
     * {@code EntityPotion.onImpact} for Instant Health II: the health a
     * potion breaking at this place offers a player whose feet are at
     * feet. A player the potion hit itself gets all of it.
     */
    static float splashHeal(double[] at, double[] feet, boolean direct) {
        double d0 = distSq(at, feet);
        if (!(d0 < 16.0)) {
            return 0.0f;
        }
        double d1 = direct ? 1.0 : 1.0 - Math.sqrt(d0) / 4.0;
        return (float) (int) (d1 * 8.0 + 0.5);
    }

    private Geo() {}
}
