package chestai.client;

import net.minecraft.block.Block;
import net.minecraft.block.material.Material;
import net.minecraft.block.state.IBlockState;
import net.minecraft.util.BlockPos;
import net.minecraft.world.World;
import chestai.core.Grid;

/** The blocks of the world as the simulator has them: what stops a player is a full cube. */
final class WorldGrid implements Grid {
    private final World w;

    WorldGrid(World w) {
        this.w = w;
    }

    public boolean solid(int x, int y, int z) {
        BlockPos p = new BlockPos(x, y, z);
        IBlockState s = w.getBlockState(p);
        Block b = s.getBlock();
        if (b.getMaterial() == Material.air) {
            return false;
        }
        return b.getCollisionBoundingBox(w, p, s) != null;
    }

    public boolean water(int x, int y, int z) {
        return w.getBlockState(new BlockPos(x, y, z)).getBlock().getMaterial() == Material.water;
    }
}
