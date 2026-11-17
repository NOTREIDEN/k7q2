package pvpai.core;

/**
 * The arena of the training simulator: a flat floor of full blocks with
 * optional walls, and the game's own walk of a line through the block grid
 * ({@code World.rayTraceBlocks}). In the game the world itself answers.
 */
public final strictfp class Arena implements Blocks {
    private final double floorY, half;
    private final int floorI, halfI, topI;
    private final boolean walls;

    public Arena(double floorY, double half, boolean walls) {
        floorI = (int) Math.floor(floorY + 0.5);
        halfI = (int) Math.floor(half + 0.5);
        this.floorY = floorI;
        this.half = halfI;
        topI = floorI + 64;
        this.walls = walls;
    }

    private boolean solid(int x, int y, int z) {
        if (y < floorI) {
            return true;
        }
        return walls && y < topI && (x >= halfI || x < -halfI || z >= halfI || z < -halfI);
    }

    private boolean open(double[] p) {
        return p[1] >= floorY && (!walls || (p[1] < (double) topI && p[0] >= -half && p[0] < half && p[2] >= -half && p[2] < half));
    }

    private static int floor(double v) {
        int i = (int) v;
        return v < (double) i ? i - 1 : i;
    }

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

    /** {@code Block.collisionRayTrace} for a full cube. */
    private static double[] cubeTrace(int x, int y, int z, double[] start, double[] end) {
        double[] s = {start[0] - x, start[1] - y, start[2] - z};
        double[] e = {end[0] - x, end[1] - y, end[2] - z};
        double[] best = null;
        for (int axis = 0; axis < 3; axis++) {
            int u = axis == 0 ? 1 : 0, w = axis == 2 ? 1 : 2;
            for (int side = 0; side < 2; side++) {
                double[] p = intermediate(s, e, axis, (double) side);
                if (p != null && p[u] >= 0.0 && p[u] <= 1.0 && p[w] >= 0.0 && p[w] <= 1.0) {
                    if (best == null || Geo.distSq(s, p) < Geo.distSq(s, best)) {
                        best = p;
                    }
                }
            }
        }
        return best == null ? null : new double[] {best[0] + x, best[1] + y, best[2] + z};
    }

    @Override
    public double[] rayTrace(double[] from, double[] b) {
        if (open(from) && open(b)) {
            return null;
        }
        double[] a = from.clone();
        int i = floor(b[0]), j = floor(b[1]), k = floor(b[2]);
        int l = floor(a[0]), i1 = floor(a[1]), j1 = floor(a[2]);
        if (solid(l, i1, j1)) {
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
                d0 = (double) l + 1.0;
            } else if (i < l) {
                d0 = (double) l + 0.0;
            } else {
                fx = false;
            }
            if (j > i1) {
                d1 = (double) i1 + 1.0;
            } else if (j < i1) {
                d1 = (double) i1 + 0.0;
            } else {
                fy = false;
            }
            if (k > j1) {
                d2 = (double) j1 + 1.0;
            } else if (k < j1) {
                d2 = (double) j1 + 0.0;
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
            int east = 0, up = 0, south = 0;
            if (d3 < d4 && d3 < d5) {
                east = i > l ? 0 : 1;
                a = new double[] {d0, a[1] + d7 * d3, a[2] + d8 * d3};
            } else if (d4 < d5) {
                up = j > i1 ? 0 : 1;
                a = new double[] {a[0] + d6 * d4, d1, a[2] + d8 * d4};
            } else {
                south = k > j1 ? 0 : 1;
                a = new double[] {a[0] + d6 * d5, a[1] + d7 * d5, d2};
            }
            l = floor(a[0]) - east;
            i1 = floor(a[1]) - up;
            j1 = floor(a[2]) - south;
            if (solid(l, i1, j1)) {
                double[] h = cubeTrace(l, i1, j1, a, b);
                if (h != null) {
                    return h;
                }
            }
        }
        return null;
    }
}
