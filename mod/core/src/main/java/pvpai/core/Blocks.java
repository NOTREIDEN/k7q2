package pvpai.core;

/** The blocks of the world, as far as an agent asks about them. */
public interface Blocks {
    /**
     * {@code World.rayTraceBlocks(a, b)}: the point at which the straight
     * line from a to b first meets a block, or null if it meets none.
     */
    double[] rayTrace(double[] a, double[] b);

    /**
     * How high the box a player collides with in the block at (x, y, z)
     * reaches above the block's bottom: 0 for air and for what is walked
     * through, 1 for a full block, 0.5 for a slab.
     */
    double height(int x, int y, int z);

    /** Whether the block at (x, y, z) is a cobweb. */
    boolean web(int x, int y, int z);
}
