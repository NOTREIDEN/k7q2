package chestai.core;

/** The blocks around the player: every solid block taken as a full cube. */
public interface Grid {
    boolean solid(int x, int y, int z);

    boolean water(int x, int y, int z);
}
