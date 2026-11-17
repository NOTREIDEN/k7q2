package pvpai.core;

/** The blocks of the world, as far as an agent asks about them. */
public interface Blocks {
    /**
     * {@code World.rayTraceBlocks(a, b)}: the point at which the straight
     * line from a to b first meets a block, or null if it meets none.
     */
    double[] rayTrace(double[] a, double[] b);
}
