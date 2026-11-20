package chestai.client;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.HashSet;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Set;
import java.util.zip.DataFormatException;
import java.util.zip.Deflater;
import java.util.zip.Inflater;
import net.minecraft.block.Block;
import net.minecraft.block.BlockDoor;
import net.minecraft.block.BlockFence;
import net.minecraft.block.BlockFenceGate;
import net.minecraft.block.BlockWall;
import net.minecraft.block.material.Material;
import net.minecraft.block.state.IBlockState;
import net.minecraft.init.Blocks;
import net.minecraft.util.AxisAlignedBB;
import net.minecraft.util.BlockPos;
import net.minecraft.world.World;
import net.minecraft.world.chunk.Chunk;
import net.minecraft.world.chunk.storage.ExtendedBlockStorage;

/**
 * The blocks of every chunk the player has had loaded, kept per server in
 * config/chestai/mapa-&lt;servidor&gt;/ with no limit, as what each block is
 * to a walker (air, solid, water, danger, too tall to step on). The way
 * planner reads it where the client has no blocks loaded, so a way can be
 * worked out before the player gets there.
 */
final class MapStore {
    static final byte AIR = 0, SOLID = 1, WATER = 2, DANGER = 3, UNKNOWN = 4, TALL = 5;
    /** Chunks kept unpacked in memory at once; the rest stay on disk. */
    private static final int IN_MEMORY = 600;
    /** A chunk near the player is read again after this many ticks; one farther, after ten times as many. */
    private static final long FRESH = 600;

    private final File dir;
    private final LinkedHashMap<Long, byte[]> chunks = new LinkedHashMap<Long, byte[]>(64, 0.75f, true) {
        private static final long serialVersionUID = 1L;

        @Override
        protected boolean removeEldestEntry(Map.Entry<Long, byte[]> e) {
            if (size() <= IN_MEMORY) {
                return false;
            }
            if (dirty.contains(e.getKey())) {
                write(e.getKey(), e.getValue());
            }
            return true;
        }
    };
    private final Map<Long, Long> readAt = new java.util.HashMap<Long, Long>();
    private final Set<Long> dirty = new HashSet<Long>();
    private final Set<Long> notOnDisk = new HashSet<Long>();
    private int scanned;

    MapStore(File dir) {
        this.dir = dir;
    }

    /** Chunks read so far in this session. */
    int scanned() {
        return scanned;
    }

    private static long key(int cx, int cz) {
        return ((long) cx << 32) ^ (cz & 0xFFFFFFFFL);
    }

    private static int cxOf(long k) {
        return (int) (k >> 32);
    }

    private static int czOf(long k) {
        return (int) k;
    }

    /** What a block is to a walker. */
    static byte classify(World w, BlockPos p, IBlockState s) {
        Block b = s.getBlock();
        Material m = b.getMaterial();
        if (m == Material.air) {
            return AIR;
        }
        if (m == Material.water) {
            return WATER;
        }
        if (m == Material.lava || m == Material.fire || b == Blocks.cactus || b == Blocks.web) {
            return DANGER;
        }
        if (b instanceof BlockDoor) {
            IBlockState lower = s.getValue(BlockDoor.HALF) == BlockDoor.EnumDoorHalf.UPPER ? w.getBlockState(p.down()) : s;
            boolean open = lower.getBlock() instanceof BlockDoor && lower.getValue(BlockDoor.OPEN).booleanValue();
            return open ? AIR : SOLID;
        }
        if (b instanceof BlockFenceGate) {
            return s.getValue(BlockFenceGate.OPEN).booleanValue() ? AIR : TALL;
        }
        if (b instanceof BlockFence || b instanceof BlockWall) {
            return TALL;
        }
        if (b.isFullCube()) {
            return SOLID;
        }
        AxisAlignedBB box = b.getCollisionBoundingBox(w, p, s);
        if (box == null) {
            return AIR;
        }
        // A carpet or a thin layer of snow: walked over.
        return box.maxY - p.getY() <= 0.2 ? AIR : SOLID;
    }

    /** The block at (x, y, z) as last read, or UNKNOWN. */
    byte cell(int x, int y, int z) {
        if (y < 0) {
            return SOLID;
        }
        if (y > 255) {
            return AIR;
        }
        byte[] c = get(key(x >> 4, z >> 4));
        if (c == null) {
            return UNKNOWN;
        }
        int i = (y << 8) | ((z & 15) << 4) | (x & 15);
        int v = c[i >> 1];
        return (byte) ((i & 1) == 0 ? v & 15 : (v >> 4) & 15);
    }

    private byte[] get(long k) {
        byte[] c = chunks.get(k);
        if (c != null || notOnDisk.contains(k)) {
            return c;
        }
        c = read(k);
        if (c == null) {
            notOnDisk.add(k);
        } else {
            chunks.put(k, c);
        }
        return c;
    }

    /** Reads one loaded chunk of the client's world into the map. */
    void scan(World w, Chunk ch, long tick) {
        byte[] c = new byte[16 * 16 * 256 / 2];
        ExtendedBlockStorage[] st = ch.getBlockStorageArray();
        int bx = ch.xPosition << 4, bz = ch.zPosition << 4;
        for (int sy = 0; sy < 16; sy++) {
            ExtendedBlockStorage e = st[sy];
            if (e == null || e.isEmpty()) {
                continue;
            }
            for (int ly = 0; ly < 16; ly++) {
                int y = (sy << 4) + ly;
                for (int z = 0; z < 16; z++) {
                    for (int x = 0; x < 16; x++) {
                        IBlockState s = e.get(x, ly, z);
                        byte v;
                        Block b = s.getBlock();
                        if (b.getMaterial() == Material.air) {
                            continue;
                        } else if (b.isFullCube() && !(b instanceof BlockDoor) && b.getMaterial() != Material.water && b.getMaterial() != Material.lava) {
                            v = SOLID;
                        } else {
                            v = classify(w, new BlockPos(bx + x, y, bz + z), s);
                        }
                        int i = (y << 8) | (z << 4) | x;
                        c[i >> 1] |= (i & 1) == 0 ? v : (byte) (v << 4);
                    }
                }
            }
        }
        long k = key(ch.xPosition, ch.zPosition);
        chunks.put(k, c);
        notOnDisk.remove(k);
        dirty.add(k);
        readAt.put(k, tick);
        scanned++;
    }

    /**
     * One step of keeping the map up to date: reads at most one loaded
     * chunk, the nearest one not read yet or not read for a while.
     */
    void tick(World w, double px, double pz, int radius, long tick) {
        int pcx = (int) Math.floor(px) >> 4, pcz = (int) Math.floor(pz) >> 4;
        for (int r = 0; r <= radius; r++) {
            for (int dx = -r; dx <= r; dx++) {
                for (int dz = -r; dz <= r; dz++) {
                    if (Math.max(Math.abs(dx), Math.abs(dz)) != r) {
                        continue;
                    }
                    long k = key(pcx + dx, pcz + dz);
                    Long at = readAt.get(k);
                    long fresh = r <= 2 ? FRESH : FRESH * 10;
                    if (at != null && tick - at < fresh) {
                        continue;
                    }
                    Chunk ch = w.getChunkFromChunkCoords(pcx + dx, pcz + dz);
                    if (ch == null || ch.isEmpty()) {
                        continue;
                    }
                    scan(w, ch, tick);
                    return;
                }
            }
        }
    }

    /** Writes the chunks read since the last save. */
    void save() {
        Iterator<Long> it = dirty.iterator();
        while (it.hasNext()) {
            long k = it.next();
            byte[] c = chunks.get(k);
            if (c != null) {
                write(k, c);
            }
            it.remove();
        }
    }

    private File file(long k) {
        return new File(dir, cxOf(k) + "_" + czOf(k) + ".bin");
    }

    private void write(long k, byte[] c) {
        try {
            dir.mkdirs();
            Deflater d = new Deflater(6);
            d.setInput(c);
            d.finish();
            ByteArrayOutputStream bo = new ByteArrayOutputStream();
            byte[] buf = new byte[8192];
            while (!d.finished()) {
                int n = d.deflate(buf);
                bo.write(buf, 0, n);
            }
            d.end();
            OutputStream out = new FileOutputStream(file(k));
            try {
                out.write(bo.toByteArray());
            } finally {
                out.close();
            }
        } catch (IOException e) {
            // The map is a help, not a need: the chunk is read again next time.
        }
    }

    private byte[] read(long k) {
        File f = file(k);
        if (!f.isFile()) {
            return null;
        }
        try {
            InputStream in = new FileInputStream(f);
            ByteArrayOutputStream bo = new ByteArrayOutputStream();
            try {
                byte[] buf = new byte[8192];
                int n;
                while ((n = in.read(buf)) > 0) {
                    bo.write(buf, 0, n);
                }
            } finally {
                in.close();
            }
            Inflater inf = new Inflater();
            inf.setInput(bo.toByteArray());
            byte[] c = new byte[16 * 16 * 256 / 2];
            int got = 0;
            while (got < c.length && !inf.finished()) {
                int n = inf.inflate(c, got, c.length - got);
                if (n == 0 && (inf.needsInput() || inf.needsDictionary())) {
                    break;
                }
                got += n;
            }
            inf.end();
            return got == c.length ? c : null;
        } catch (IOException e) {
            return null;
        } catch (DataFormatException e) {
            return null;
        }
    }
}
