package chestai.core;

/**
 * The block tests of the observation, as the simulator runs them:
 * {@code World.rayTraceBlocks} over full cubes and
 * {@code Entity.handleWaterMovement}.
 */
public final class Ray {
    private Ray() {
    }

    static int floor(double v) {
        int i = (int) v;
        return v < i ? i - 1 : i;
    }

    private static double[] intermediate(double[] a, double[] b, int axis, double value) {
        double d0 = b[0] - a[0], d1 = b[1] - a[1], d2 = b[2] - a[2];
        double[] d = {d0, d1, d2};
        if (d[axis] * d[axis] < 1.0000000116860974E-7) {
            return null;
        }
        double t = (value - a[axis]) / d[axis];
        if (t >= 0.0 && t <= 1.0) {
            return new double[] {a[0] + d0 * t, a[1] + d1 * t, a[2] + d2 * t};
        }
        return null;
    }

    private static double distSq(double[] a, double[] b) {
        double x = a[0] - b[0], y = a[1] - b[1], z = a[2] - b[2];
        return x * x + y * y + z * z;
    }

    private static double[] cubeTrace(int px, int py, int pz, double[] start, double[] end) {
        double[] s = {start[0] - px, start[1] - py, start[2] - pz};
        double[] e = {end[0] - px, end[1] - py, end[2] - pz};
        double[] best = null;
        for (int axis = 0; axis < 3; axis++) {
            int u = axis == 0 ? 1 : 0, w = axis == 2 ? 1 : 2;
            for (int k = 0; k < 2; k++) {
                double[] p = intermediate(s, e, axis, k);
                if (p != null && p[u] >= 0.0 && p[u] <= 1.0 && p[w] >= 0.0 && p[w] <= 1.0) {
                    if (best == null || distSq(s, p) < distSq(s, best)) {
                        best = p;
                    }
                }
            }
        }
        return best == null ? null : new double[] {best[0] + px, best[1] + py, best[2] + pz};
    }

    /** Where the ray from a to b first hits a solid block, or null. */
    public static double[] hit(Grid g, double[] a0, double[] b) {
        double[] a = a0.clone();
        int i = floor(b[0]), j = floor(b[1]), k = floor(b[2]);
        int l = floor(a[0]), i1 = floor(a[1]), j1 = floor(a[2]);
        if (g.solid(l, i1, j1)) {
            double[] h = cubeTrace(l, i1, j1, a, b);
            if (h != null) {
                return h;
            }
        }
        for (int n = 0; n < 201; n++) {
            if (l == i && i1 == j && j1 == k) {
                return null;
            }
            boolean fx = true, fy = true, fz = true;
            double d0 = 999.0, d1 = 999.0, d2 = 999.0;
            if (i > l) {
                d0 = l + 1.0;
            } else if (i < l) {
                d0 = l + 0.0;
            } else {
                fx = false;
            }
            if (j > i1) {
                d1 = i1 + 1.0;
            } else if (j < i1) {
                d1 = i1 + 0.0;
            } else {
                fy = false;
            }
            if (k > j1) {
                d2 = j1 + 1.0;
            } else if (k < j1) {
                d2 = j1 + 0.0;
            } else {
                fz = false;
            }
            double d3 = 999.0, d4 = 999.0, d5 = 999.0;
            double d6 = b[0] - a[0], d7 = b[1] - a[1], d8 = b[2] - a[2];
            if (fx) {
                d3 = (d0 - a[0]) / d6;
            }
            if (fy) {
                d4 = (d1 - a[1]) / d7;
            }
            if (fz) {
                d5 = (d2 - a[2]) / d8;
            }
            if (d3 == 0.0) {
                d3 = -1.0E-4;
            }
            if (d4 == 0.0) {
                d4 = -1.0E-4;
            }
            if (d5 == 0.0) {
                d5 = -1.0E-4;
            }
            boolean east = false, up = false, south = false;
            if (d3 < d4 && d3 < d5) {
                east = !(i > l);
                a = new double[] {d0, a[1] + d7 * d3, a[2] + d8 * d3};
            } else if (d4 < d5) {
                up = !(j > i1);
                a = new double[] {a[0] + d6 * d4, d1, a[2] + d8 * d4};
            } else {
                south = !(k > j1);
                a = new double[] {a[0] + d6 * d5, a[1] + d7 * d5, d2};
            }
            l = floor(a[0]) - (east ? 1 : 0);
            i1 = floor(a[1]) - (up ? 1 : 0);
            j1 = floor(a[2]) - (south ? 1 : 0);
            if (g.solid(l, i1, j1)) {
                double[] h = cubeTrace(l, i1, j1, a, b);
                if (h != null) {
                    return h;
                }
            }
        }
        return null;
    }

    /** {@code Entity.handleWaterMovement} for a player standing at (x, y, z). */
    public static boolean inWater(Grid g, double x, double y, double z) {
        double hw = 0.3, h = 1.8;
        double minX = x - hw + 0.001, minY = y + 0.4 + 0.001, minZ = z - hw + 0.001;
        double maxX = x + hw - 0.001, maxY = y + h - 0.001, maxZ = z + hw - 0.001;
        int i = floor(minX), j = floor(maxX + 1.0);
        int k = floor(minY), l = floor(maxY + 1.0);
        int i1 = floor(minZ), j1 = floor(maxZ + 1.0);
        for (int bx = i; bx < j; bx++) {
            for (int by = k; by < l; by++) {
                for (int bz = i1; bz < j1; bz++) {
                    if (g.water(bx, by, bz) && (by + 1) >= minY) {
                        return true;
                    }
                }
            }
        }
        return false;
    }
}
