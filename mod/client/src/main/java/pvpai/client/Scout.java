package pvpai.client;

import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.SocketAddress;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityOtherPlayerMP;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.network.NetHandlerPlayClient;
import net.minecraft.client.network.NetworkPlayerInfo;
import net.minecraft.enchantment.Enchantment;
import net.minecraft.enchantment.EnchantmentHelper;
import net.minecraft.entity.SharedMonsterAttributes;
import net.minecraft.entity.ai.attributes.AttributeModifier;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.item.ItemStack;
import net.minecraft.util.MathHelper;
import net.minecraft.util.MovingObjectPosition;
import net.minecraft.util.Vec3;
import pvpai.core.Inv;

/**
 * Finds out, when a duel starts, what an agent is told about it and no
 * packet says: where the walls of the arena are, how high its floor is,
 * what a hit costs with the armour and the swords in play, and the pings.
 */
final class Scout {
    /** How far a wall is looked for, and how far away one is taken to be where none is found. */
    private static final double REACH = 48.0, OPEN = 24.0;

    private Scout() {
    }

    private static MovingObjectPosition ray(Minecraft mc, double[] a, double[] b) {
        // Grass and the like, which a player walks through, is no wall.
        MovingObjectPosition m = mc.theWorld.rayTraceBlocks(new Vec3(a[0], a[1], a[2]), new Vec3(b[0], b[1], b[2]), false, true, false);
        return m != null && m.typeOfHit == MovingObjectPosition.MovingObjectType.BLOCK ? m : null;
    }

    /**
     * The arena the two players stand in: x and z of its centre and half
     * its side. Looked for along both axes from where the two stand, at
     * the height of their knees and of their eyes; the wall of a side is
     * the farthest block found, so that a pillar in the way of one look
     * does not count. A side with no wall within reach is taken to end
     * as far from the two as the arenas of training are wide. Of an arena
     * that is not square, half the longer side.
     */
    static double[] arena(Minecraft mc, EntityOtherPlayerMP other) {
        EntityPlayerSP me = mc.thePlayer;
        double[][] from = {
            {me.posX, me.posY + 0.5, me.posZ}, {me.posX, me.posY + 1.62, me.posZ},
            {other.posX, other.posY + 0.5, other.posZ}, {other.posX, other.posY + 1.62, other.posZ},
        };
        double[] mid = {(me.posX + other.posX) / 2.0, 0.0, (me.posZ + other.posZ) / 2.0};
        double[] lim = new double[4];
        for (int side = 0; side < 4; side++) {
            int axis = side < 2 ? 0 : 2;
            double sign = side % 2 == 0 ? -1.0 : 1.0;
            double far = 0.0;
            boolean found = false, open = false;
            for (double[] f : from) {
                double[] to = f.clone();
                to[axis] += sign * REACH;
                MovingObjectPosition m = ray(mc, f, to);
                if (m == null) {
                    open = true;
                    break;
                }
                double at = axis == 0 ? m.hitVec.xCoord : m.hitVec.zCoord;
                if (!found || sign * at > sign * far) {
                    far = at;
                    found = true;
                }
            }
            lim[side] = open || !found ? mid[axis] + sign * OPEN : far;
        }
        double half = Math.max(Math.max(lim[1] - lim[0], lim[3] - lim[2]) / 2.0, 2.0);
        return new double[] {(lim[0] + lim[1]) / 2.0, (lim[2] + lim[3]) / 2.0, half};
    }

    /** The height of the floor under the player. */
    static double floor(Minecraft mc) {
        EntityPlayerSP me = mc.thePlayer;
        MovingObjectPosition m = ray(mc, new double[] {me.posX, me.posY + 0.5, me.posZ}, new double[] {me.posX, me.posY - 16.0, me.posZ});
        return m == null ? me.posY : m.hitVec.yCoord;
    }

    /**
     * The health a plain hit of the other player costs this one, on
     * average over what Protection draws: the other's sword, or this
     * one's own while the other holds something else, against the armour
     * this one wears.
     */
    static float hitHealth(EntityPlayerSP me, EntityOtherPlayerMP other, Layout layout) {
        ItemStack sword = other.getHeldItem();
        if (Reader.shown(sword) != Inv.SWORD) {
            sword = me.inventory.mainInventory[layout.real[Inv.SLOT_SWORD]];
        }
        float damage = 1.0F;
        if (sword != null) {
            for (AttributeModifier m : sword.getAttributeModifiers().get(SharedMonsterAttributes.attackDamage.getAttributeUnlocalizedName())) {
                if (m.getOperation() == 0) {
                    damage += (float) m.getAmount();
                }
            }
            damage += 1.25F * (float) EnchantmentHelper.getEnchantmentLevel(Enchantment.sharpness.effectId, sword);
        }
        int armor = Math.min(me.getTotalArmorValue(), 25);
        int prot = 0;
        for (ItemStack s : me.inventory.armorInventory) {
            int level = s == null ? 0 : EnchantmentHelper.getEnchantmentLevel(Enchantment.protection.effectId, s);
            if (level > 0) {
                prot += MathHelper.floor_float((float) (6 + level * level) / 3.0F * 0.75F);
            }
        }
        int m = Math.min(prot, 25);
        float k = Math.min((float) ((m + 1) >> 1) + (float) (m >> 1) * 0.5F, 20.0F);
        return damage * (float) (25 - armor) / 25.0F * (25.0F - k) / 25.0F;
    }

    /**
     * The ping of a player as the player list has it, in milliseconds; less
     * than zero if it does not say. The list holds a zero for a player the
     * server has no ping of yet, so a zero is taken as said only from a
     * server on this machine or in this network, where it is the ping.
     */
    static double ping(Minecraft mc, EntityPlayer p) {
        NetHandlerPlayClient net = mc.getNetHandler();
        NetworkPlayerInfo info = net == null ? null : net.getPlayerInfo(p.getUniqueID());
        if (info == null) {
            return -1.0;
        }
        int ms = info.getResponseTime();
        if (ms > 0) {
            return (double) ms;
        }
        return ms == 0 && near(mc, net) ? 0.0 : -1.0;
    }

    /** Whether the server is on this machine or in this network. */
    private static boolean near(Minecraft mc, NetHandlerPlayClient net) {
        if (mc.isIntegratedServerRunning() || net.getNetworkManager().isLocalChannel()) {
            return true;
        }
        SocketAddress a = net.getNetworkManager().getRemoteAddress();
        if (!(a instanceof InetSocketAddress)) {
            return false;
        }
        InetAddress host = ((InetSocketAddress) a).getAddress();
        return host != null && (host.isLoopbackAddress() || host.isSiteLocalAddress() || host.isLinkLocalAddress());
    }
}
