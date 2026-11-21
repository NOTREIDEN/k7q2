package chestai.client;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.PriorityQueue;
import net.minecraft.entity.Entity;
import net.minecraft.util.AxisAlignedBB;
import net.minecraft.util.BlockPos;
import net.minecraft.util.Vec3;
import net.minecraft.world.World;

/**
 * A way on foot through the blocks the client has loaded: A* over the
 * spots a player can stand on (two blocks of air over something solid, or
 * water), going round walls, houses and trees, up one block with a jump,
 * down at most three, swimming through water, never through lava, fire,
 * cactus or cobwebs. A goal out of the loaded blocks or out of reach of the
 * search gets the way to the spot nearest to it.
 */
final class Planner {
    /** How far from the start the search goes, and how many spots it opens at most. */
    static final int RANGE = 80, MAX_NODES = 24000;
    /**
     * -Dchestai.move3: no step up from a spot held up by water alone (the player floats half a
     * block lower than that and cannot get two up out of a pool), falling water (a waterfall)
     * dear to go through or up, and no step up where the head has no room to jump.
     */
    static final boolean MOVE3 = !"false".equals(System.getProperty("chestai.move3"));

    private final World w;
    private final MapStore map;
    private final Reader reader;
    private final Map<Long, Byte> cache = new HashMap<Long, Byte>();
    /** Spots the player got stuck going to, left out of ways until the tick kept for each. */
    private final Map<Long, Long> bans = new HashMap<Long, Long>();
    private long now;
    /** Whether the last way planned gets to the goal (not only as near as it can). */
    boolean reached;

    Planner(World w, MapStore map, Reader reader) {
        this.w = w;
        this.map = map;
        this.reader = reader;
    }

    /** Forget the blocks read for the last way (they may have changed). */
    void clear() {
        cache.clear();
        brinks.clear();
    }

    /** The tick it is, for the spots left out. */
    void now(long tick) {
        now = tick;
    }

    /** Leaves the spot (x, y, z) out of ways until the tick {@code until}. */
    void ban(int x, int y, int z, long until) {
        bans.put(key(x, y, z), until);
        if (bans.size() > 200) {
            java.util.Iterator<Map.Entry<Long, Long>> it = bans.entrySet().iterator();
            while (it.hasNext()) {
                if (it.next().getValue() <= now) {
                    it.remove();
                }
            }
        }
    }

    private static long key(int x, int y, int z) {
        return ((long) (x & 0x3FFFFFF) << 38) | ((long) (y & 0xFFF) << 26) | (long) (z & 0x3FFFFFF);
    }

    /** What the block is to a walker (MapStore's kinds): live where the client has it loaded, else from the map. */
    private byte cell(int x, int y, int z) {
        long k = key(x, y, z);
        Byte c = cache.get(k);
        if (c != null) {
            return c;
        }
        byte v;
        if (y < 0) {
            v = MapStore.SOLID;
        } else if (y > 255) {
            v = MapStore.AIR;
        } else if (w.getChunkFromChunkCoords(x >> 4, z >> 4).isEmpty()) {
            v = map != null ? map.cell(x, y, z) : MapStore.UNKNOWN;
        } else {
            BlockPos p = new BlockPos(x, y, z);
            v = MapStore.classify(w, p, w.getBlockState(p));
        }
        cache.put(k, v);
        return v;
    }

    boolean solid(int x, int y, int z) {
        return cell(x, y, z) == MapStore.SOLID;
    }

    /**
     * Whether a blow from (ax, ay, az) gets to (bx, by, bz): no block on the
     * line, as the game's ray sees them, and no chest's hologram stand the
     * crosshair would stop on first.
     */
    boolean combatSight(double ax, double ay, double az, double bx, double by, double bz) {
        Vec3 from = new Vec3(ax, ay, az);
        Vec3 to = new Vec3(bx, by, bz);
        if (w.rayTraceBlocks(from, to, false, false, false) != null) {
            return false;
        }
        for (Entity e : reader.chestMarkers()) {
            if (!e.canBeCollidedWith()) {
                continue;
            }
            float border = e.getCollisionBorderSize();
            AxisAlignedBB box = e.getEntityBoundingBox().expand(border, border, border);
            if (box.isVecInside(from) || box.calculateIntercept(from, to) != null) {
                return false;
            }
        }
        return true;
    }

    boolean water(int x, int y, int z) {
        return cell(x, y, z) == MapStore.WATER;
    }

    /** Water falling (a waterfall's column, as the game's level 8 and up) where the client has it loaded. */
    boolean falling(int x, int y, int z) {
        if (!water(x, y, z) || y < 0 || y > 255 || !loaded(x, z)) {
            return false;
        }
        net.minecraft.block.state.IBlockState st = w.getBlockState(new BlockPos(x, y, z));
        if (!(st.getBlock() instanceof net.minecraft.block.BlockLiquid)) {
            return false;
        }
        return st.getValue(net.minecraft.block.BlockLiquid.LEVEL).intValue() >= 8;
    }

    /** Over water and not in it: the player is in fact half a block lower, in the water, and no higher step is in reach. */
    boolean floats(int x, int y, int z) {
        return !water(x, y, z) && water(x, y - 1, z);
    }

    /** Whether the client has the chunk of the column (x, z) loaded. */
    boolean loaded(int x, int z) {
        return !w.getChunkFromChunkCoords(x >> 4, z >> 4).isEmpty();
    }

    /** Air, water or plants: a body fits through. */
    boolean open(int x, int y, int z) {
        byte c = cell(x, y, z);
        return c == MapStore.AIR || c == MapStore.WATER || c == MapStore.PLANT;
    }

    /** Air or plants: dry room for a body. */
    private boolean dryOpen(int x, int y, int z) {
        byte c = cell(x, y, z);
        return c == MapStore.AIR || c == MapStore.PLANT;
    }

    /**
     * Whether a click from (ax, ay, az) reaches (bx, by, bz): nothing on the
     * line that the game's ray would stop at (a block, a fence, tall grass);
     * water lets it through. The block (sx, sy, sz), the one clicked, is not
     * counted.
     */
    boolean see(double ax, double ay, double az, double bx, double by, double bz, int sx, int sy, int sz) {
        // Every block the line goes through, corners too (a walk from block to block along it).
        int x = (int) Math.floor(ax), y = (int) Math.floor(ay), z = (int) Math.floor(az);
        int ex = (int) Math.floor(bx), ey = (int) Math.floor(by), ez = (int) Math.floor(bz);
        double dx = bx - ax, dy = by - ay, dz = bz - az;
        int stepX = dx > 0 ? 1 : -1, stepY = dy > 0 ? 1 : -1, stepZ = dz > 0 ? 1 : -1;
        double tdx = dx == 0 ? 1e30 : Math.abs(1.0 / dx), tdy = dy == 0 ? 1e30 : Math.abs(1.0 / dy), tdz = dz == 0 ? 1e30 : Math.abs(1.0 / dz);
        double tx = dx == 0 ? 1e30 : (dx > 0 ? (x + 1 - ax) : (ax - x)) * tdx;
        double ty = dy == 0 ? 1e30 : (dy > 0 ? (y + 1 - ay) : (ay - y)) * tdy;
        double tz = dz == 0 ? 1e30 : (dz > 0 ? (z + 1 - az) : (az - z)) * tdz;
        for (int n = 0; n < 64; n++) {
            if (x == ex && y == ey && z == ez) {
                return true;
            }
            if (tx <= ty && tx <= tz) {
                x += stepX;
                tx += tdx;
            } else if (ty <= tz) {
                y += stepY;
                ty += tdy;
            } else {
                z += stepZ;
                tz += tdz;
            }
            boolean end = x == ex && y == ey && z == ez;
            if (x == sx && y == sy && z == sz) {
                if (end) {
                    return true;
                }
                continue;
            }
            byte c = cell(x, y, z);
            if (c != MapStore.AIR && c != MapStore.WATER) {
                return false;
            }
            if (end) {
                return true;
            }
        }
        return true;
    }

    /** A player can be at this block: room for the body, and ground or water to keep it there. */
    boolean stand(int x, int y, int z) {
        if (!bans.isEmpty()) {
            Long until = bans.get(key(x, y, z));
            if (until != null && until > now) {
                return false;
            }
        }
        if (!open(x, y, z) || !open(x, y + 1, z)) {
            return false;
        }
        byte below = cell(x, y - 1, z);
        return below == MapStore.SOLID || below == MapStore.WATER || cell(x, y, z) == MapStore.WATER;
    }

    /**
     * The nearest spot within {@code r} blocks across (and three up or down)
     * on dry ground with air for the head, or null: out of water, a wall, lava
     * or fire.
     */
    int[] nearestDry(double px, double py, double pz, int r) {
        int x0 = (int) Math.floor(px), y0 = (int) Math.floor(py + 0.01), z0 = (int) Math.floor(pz);
        int[] best = null;
        double bd = 1e9;
        for (int dx = -r; dx <= r; dx++) {
            for (int dz = -r; dz <= r; dz++) {
                for (int dy = -3; dy <= 3; dy++) {
                    int x = x0 + dx, y = y0 + dy, z = z0 + dz;
                    double d = dx * dx + dz * dz + dy * dy * 2.0;
                    if (d >= bd || d < 1.0) {
                        continue;
                    }
                    if (dryOpen(x, y, z) && dryOpen(x, y + 1, z) && cell(x, y - 1, z) == MapStore.SOLID) {
                        bd = d;
                        best = new int[] {x, y, z};
                    }
                }
            }
        }
        return best;
    }

    private static final int[][] DIRS = {{1, 0}, {-1, 0}, {0, 1}, {0, -1}, {1, 1}, {1, -1}, {-1, 1}, {-1, -1}};

    private static final class Node implements Comparable<Node> {
        final int x, y, z;
        double g, f;
        Node from;
        boolean closed;

        Node(int x, int y, int z) {
            this.x = x;
            this.y = y;
            this.z = z;
        }

        public int compareTo(Node o) {
            return Double.compare(f, o.f);
        }
    }

    private static double h(int x, int y, int z, double gx, double gy, double gz) {
        double dx = Math.abs(x + 0.5 - gx), dz = Math.abs(z + 0.5 - gz);
        double lo = Math.min(dx, dz), hi = Math.max(dx, dz);
        return hi + 0.414 * lo + 0.3 * Math.abs(y - gy);
    }

    /** The block the player's feet are in, or the nearest one it could stand at. */
    int[] startAt(double px, double py, double pz) {
        int x = (int) Math.floor(px), y = (int) Math.floor(py + 0.01), z = (int) Math.floor(pz);
        int[] dy = {0, 1, -1, 2};
        for (int d : dy) {
            if (stand(x, y + d, z)) {
                return new int[] {x, y + d, z};
            }
        }
        return new int[] {x, y, z};
    }

    /**
     * A way from (px, py, pz) to within {@code tol} blocks (across) of the
     * goal and two blocks up or down of it, as the blocks to walk through,
     * the first one the start. Null if there is none that gets closer.
     */
    List<int[]> plan(double px, double py, double pz, double gx, double gy, double gz, double tol) {
        return plan(px, py, pz, gx, gy, gz, tol, MAX_NODES);
    }

    /**
     * Whether standing at (x, y, z) has a drop of four or more right beside it (a pit, a cliff's
     * edge): a knock or a slip there is a fall into somewhere hard to get out of.
     */
    boolean brink(int x, int y, int z) {
        long k = key(x, y, z);
        Boolean b = brinks.get(k);
        if (b != null) {
            return b;
        }
        boolean out = false;
        for (int i = 0; i < 4 && !out; i++) {
            int nx = x + DIRS[i][0], nz = z + DIRS[i][1];
            out = open(nx, y, nz) && open(nx, y - 1, nz) && open(nx, y - 2, nz) && open(nx, y - 3, nz) && !water(nx, y - 1, nz) && !water(nx, y - 2, nz);
        }
        brinks.put(k, out);
        return out;
    }

    private final Map<Long, Boolean> brinks = new HashMap<Long, Boolean>();

    List<int[]> plan(double px, double py, double pz, double gx, double gy, double gz, double tol, int budget) {
        brinks.clear();
        int[] s = startAt(px, py, pz);
        Map<Long, Node> nodes = new HashMap<Long, Node>();
        PriorityQueue<Node> open = new PriorityQueue<Node>();
        Node start = new Node(s[0], s[1], s[2]);
        start.f = h(s[0], s[1], s[2], gx, gy, gz);
        nodes.put(key(s[0], s[1], s[2]), start);
        open.add(start);
        Node best = start;
        double bestH = start.f;
        Node goal = null;
        int opened = 0;
        while (!open.isEmpty() && opened < budget) {
            Node n = open.poll();
            if (n.closed) {
                continue;
            }
            n.closed = true;
            opened++;
            double dxg = n.x + 0.5 - gx, dzg = n.z + 0.5 - gz;
            if (dxg * dxg + dzg * dzg <= tol * tol && Math.abs(n.y - gy) <= 2.5) {
                goal = n;
                break;
            }
            double hn = h(n.x, n.y, n.z, gx, gy, gz);
            if (hn < bestH) {
                bestH = hn;
                best = n;
            }
            boolean inWater = water(n.x, n.y, n.z);
            for (int[] d : DIRS) {
                int nx = n.x + d[0], nz = n.z + d[1];
                if (Math.abs(nx - s[0]) > RANGE || Math.abs(nz - s[2]) > RANGE) {
                    continue;
                }
                boolean diag = d[0] != 0 && d[1] != 0;
                if (diag && !(open(n.x + d[0], n.y, n.z) && open(n.x + d[0], n.y + 1, n.z) && open(n.x, n.y, n.z + d[1]) && open(n.x, n.y + 1, n.z + d[1]))) {
                    continue;
                }
                double base = diag ? 1.414 : 1.0;
                int ny = Integer.MIN_VALUE;
                double cost = 0;
                if (stand(nx, n.y, nz)) {
                    ny = n.y;
                    cost = base;
                } else if (stand(nx, n.y + 1, nz) && open(n.x, n.y + 2, n.z) && !(MOVE3 && floats(n.x, n.y, n.z))) {
                    ny = n.y + 1;
                    cost = base + 0.8;
                } else if (open(nx, n.y, nz) && open(nx, n.y + 1, nz)) {
                    for (int drop = 1; drop <= 3; drop++) {
                        if (!open(nx, n.y - drop + 1, nz)) {
                            break;
                        }
                        if (stand(nx, n.y - drop, nz)) {
                            ny = n.y - drop;
                            cost = base + 0.3 * drop;
                            break;
                        }
                    }
                }
                if (ny == Integer.MIN_VALUE) {
                    continue;
                }
                if (water(nx, ny, nz) || inWater) {
                    cost += 1.5;
                }
                if (MOVE3 && (falling(nx, ny, nz) || falling(nx, ny + 1, nz))) {
                    // A waterfall pushes down and across: round it if there is a way.
                    cost += 6.0;
                }
                if (brink(nx, ny, nz)) {
                    // Along the edge of a drop only if there is no other way.
                    cost += 1.5;
                }
                add(nodes, open, n, nx, ny, nz, cost, gx, gy, gz);
            }
            // Swimming up out of water, or up a column of it.
            if (inWater && stand(n.x, n.y + 1, n.z)) {
                // (Up still water a tenth of a block a tick; up a waterfall hardly at all.)
                double up = !MOVE3 ? 1.0 : falling(n.x, n.y, n.z) || falling(n.x, n.y + 1, n.z) ? 14.0 : 2.5;
                add(nodes, open, n, n.x, n.y + 1, n.z, up, gx, gy, gz);
            }
        }
        reached = goal != null;
        Node end = goal != null ? goal : best;
        if (end == start) {
            return null;
        }
        List<int[]> path = new ArrayList<int[]>();
        for (Node n = end; n != null; n = n.from) {
            path.add(0, new int[] {n.x, n.y, n.z});
        }
        return path;
    }

    private static void add(Map<Long, Node> nodes, PriorityQueue<Node> open, Node from, int x, int y, int z, double cost, double gx, double gy, double gz) {
        long k = key(x, y, z);
        Node m = nodes.get(k);
        double g = from.g + cost;
        if (m == null) {
            m = new Node(x, y, z);
            nodes.put(k, m);
        } else if (m.closed || g >= m.g) {
            return;
        }
        m.g = g;
        m.from = from;
        m.f = g + h(x, y, z, gx, gy, gz);
        open.add(m);
    }

    /**
     * Whether a straight walk on the ground from the player to (tx, tz) goes
     * without a wall, a hole deeper than three or a step up of more than one
     * block, for the whole width of the body (a corner it would brush against
     * stops it as a wall does).
     */
    boolean straight(double px, double py, double pz, double tx, double tz) {
        double dx = tx - px, dz = tz - pz, len = Math.sqrt(dx * dx + dz * dz);
        if (len < 0.01) {
            return true;
        }
        double ox = -dz / len * 0.28, oz = dx / len * 0.28;
        return line(px, py, pz, tx, tz) && line(px + ox, py, pz + oz, tx + ox, tz + oz) && line(px - ox, py, pz - oz, tx - ox, tz - oz);
    }

    private boolean line(double px, double py, double pz, double tx, double tz) {
        double dx = tx - px, dz = tz - pz;
        double len = Math.sqrt(dx * dx + dz * dz);
        int steps = (int) Math.ceil(len / 0.5);
        int y = startAt(px, py, pz)[1];
        int lx = (int) Math.floor(px), lz = (int) Math.floor(pz);
        for (int i = 1; i <= steps; i++) {
            double t = (double) i / steps;
            int bx = (int) Math.floor(px + dx * t), bz = (int) Math.floor(pz + dz * t);
            int fx = lx, fz = lz;
            lx = bx;
            lz = bz;
            if (stand(bx, y, bz)) {
                continue;
            }
            if (stand(bx, y + 1, bz) && open(bx, y + 2, bz) && (!MOVE3 || (open(fx, y + 2, fz) && !floats(fx, y, fz)))) {
                y++;
                continue;
            }
            boolean down = false;
            for (int drop = 1; drop <= 3 && open(bx, y - drop + 1, bz); drop++) {
                if (stand(bx, y - drop, bz)) {
                    y -= drop;
                    down = true;
                    break;
                }
            }
            if (!down) {
                return false;
            }
        }
        return true;
    }
}
