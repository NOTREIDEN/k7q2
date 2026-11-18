package pvpai.core;

/**
 * The arena of the training simulator: a flat floor of full blocks with
 * optional walls and terrain on it (full blocks and slabs, pits and
 * columns open to the void), and the game's own walk of a line through the
 * block grid ({@code World.rayTraceBlocks}). In the game the world itself
 * answers.
 */
public final strictfp class Arena implements Blocks {
    private final double floorY, half;
    private final int floorI, halfI, topI;
    private final boolean walls;
    /** The terrain's box: its corner and sizes, its blocks (0 air, 1 full, 2 slab) and its void columns. */
    private int x0, y0, z0, nx, ny, nz;
    private byte[] cells;
    private boolean[] voids;
    private double top;

    public Arena(double floorY, double half, boolean walls) {
        floorI = (int) Math.floor(floorY + 0.5);
        halfI = (int) Math.floor(half + 0.5);
        this.floorY = floorI;
        this.half = halfI;
        topI = floorI + 64;
        this.walls = walls;
        top = this.floorY;
    }

    /** Puts terrain in: the corner and sizes of its box, its blocks (x, then z, then y) and its void columns. */
    public void terrain(int[] box, byte[] cells, boolean[] voids) {
        x0 = box[0];
        y0 = box[1];
        z0 = box[2];
        nx = box[3];
        ny = box[4];
        nz = box[5];
        this.cells = cells;
        this.voids = voids;
        double t = floorY;
        for (int c = 0; c < nx * nz; c++) {
            for (int j = ny - 1; j >= 0; j--) {
                if (cells[c * ny + j] != 0) {
                    t = Math.max(t, (double) (y0 + j) + height(cells[c * ny + j]));
                    break;
                }
            }
        }
        top = t;
    }

    private static double height(int b) {
        return b == 0 ? 0.0 : b == 2 ? 0.5 : 1.0;
    }

    private boolean baseSolid(int x, int y, int z) {
        if (y < floorI) {
            return true;
        }
        return walls && y < topI && (x >= halfI || x < -halfI || z >= halfI || z < -halfI);
    }

    private int block(int x, int y, int z) {
        if (cells != null) {
            int i = x - x0, k = z - z0;
            if (i >= 0 && k >= 0 && i < nx && k < nz) {
                int c = i * nz + k, j = y - y0;
                if (j < 0) {
                    return voids[c] ? 0 : 1;
                }
                if (j < ny) {
                    return cells[c * ny + j];
                }
            }
        }
        return baseSolid(x, y, z) ? 1 : 0;
    }

    @Override
    public double height(int x, int y, int z) {
        return height(block(x, y, z));
    }

    private boolean open(double[] p) {
        return p[1] >= top && (!walls || (p[1] < (double) topI && p[0] >= -half && p[0] < half && p[2] >= -half && p[2] < half));
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

    /** {@code Block.collisionRayTrace} for a full cube or a slab, {@code top} high. */
    private static double[] cubeTrace(int x, int y, int z, double top, double[] start, double[] end) {
        double[] s = {start[0] - x, start[1] - y, start[2] - z};
        double[] e = {end[0] - x, end[1] - y, end[2] - z};
        double[] hi = {1.0, top, 1.0};
        double[] best = null;
        for (int axis = 0; axis < 3; axis++) {
            int u = axis == 0 ? 1 : 0, w = axis == 2 ? 1 : 2;
            for (int side = 0; side < 2; side++) {
                double[] p = intermediate(s, e, axis, side == 0 ? 0.0 : hi[axis]);
                if (p != null && p[u] >= 0.0 && p[u] <= hi[u] && p[w] >= 0.0 && p[w] <= hi[w]) {
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
        double t0 = height(l, i1, j1);
        if (t0 > 0.0) {
            double[] h = cubeTrace(l, i1, j1, t0, a, b);
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
            double t = height(l, i1, j1);
            if (t > 0.0) {
                double[] h = cubeTrace(l, i1, j1, t, a, b);
                if (h != null) {
                    return h;
                }
            }
        }
        return null;
    }
}
