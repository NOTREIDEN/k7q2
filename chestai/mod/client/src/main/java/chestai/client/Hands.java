package chestai.client;

import java.lang.reflect.Field;
import net.minecraft.client.Minecraft;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.client.settings.KeyBinding;
import net.minecraft.item.ItemStack;
import chestai.core.Pilot;

/**
 * Presses the keys of one tick as a keyboard and a mouse would have by
 * the time the game reads them in that tick (the event at the start of
 * the tick comes before the game reads its keys and works out what the
 * crosshair is on).
 */
final class Hands {
    /** 1.8 keeps the left button from clicking for half a second after a click into the air; the simulator does not. */
    private static final Field MISS_DELAY = Names.field(Minecraft.class, "leftClickCounter", "field_71429_W");

    private final Minecraft mc;
    private double carryX, carryY;
    private boolean useDown;

    Hands(Minecraft mc) {
        this.mc = mc;
    }

    /** What a mouse count turns the view by (times 0.15 degrees), at the sensitivity that is set. */
    static float mouseFactor(GameSettings gs) {
        float f = gs.mouseSensitivity * 0.6F + 0.2F;
        return f * f * f * 8.0F;
    }

    private static void key(KeyBinding k, boolean down) {
        KeyBinding.setKeyBindState(k.getKeyCode(), down);
    }

    void press(Pilot.Keys k, Layout lay) {
        GameSettings gs = mc.gameSettings;
        key(gs.keyBindForward, k.forward > 0);
        key(gs.keyBindBack, k.forward < 0);
        key(gs.keyBindLeft, k.strafe > 0);
        key(gs.keyBindRight, k.strafe < 0);
        key(gs.keyBindJump, k.jump);
        key(gs.keyBindSprint, k.sprint);

        // A mob riding a horse: the crosshair on the horse lands the blows on the
        // horse, which is not part of the wave. Turn the view up to the rider
        // instead, within the same turn limits, and keep the click for it.
        float dyaw = k.dyaw, dpitch = k.dpitch;
        boolean onMount = false;
        net.minecraft.entity.Entity under = mc.objectMouseOver != null ? mc.objectMouseOver.entityHit : null;
        if (under != null && under.riddenByEntity instanceof net.minecraft.entity.EntityLiving) {
            net.minecraft.entity.Entity r = under.riddenByEntity;
            double ex = mc.thePlayer.posX, ey = mc.thePlayer.posY + mc.thePlayer.getEyeHeight(), ez = mc.thePlayer.posZ;
            double tx = r.posX - ex, ty = r.posY + r.height * 0.6 - ey, tz = r.posZ - ez;
            double wantYaw = Math.toDegrees(Math.atan2(-tx, tz));
            double wantPitch = -Math.toDegrees(Math.atan2(ty, Math.sqrt(tx * tx + tz * tz)));
            double ey2 = net.minecraft.util.MathHelper.wrapAngleTo180_double(wantYaw - mc.thePlayer.rotationYaw);
            double ep = wantPitch - mc.thePlayer.rotationPitch;
            dyaw = (float) Math.max(-Pilot.MAX_DYAW, Math.min(Pilot.MAX_DYAW, ey2));
            dpitch = (float) Math.max(-Pilot.MAX_DPITCH, Math.min(Pilot.MAX_DPITCH, ep));
            onMount = true;
        }

        // The view, in whole mouse counts.
        float f1 = mouseFactor(gs);
        double x = dyaw / (0.15 * f1) + carryX, y = dpitch / (0.15 * f1) + carryY;
        long nx = Math.round(x), ny = Math.round(y);
        carryX = x - nx;
        carryY = y - ny;
        if (nx != 0 || ny != 0) {
            mc.thePlayer.setAngles(nx * f1, -(ny * f1));
        }

        // The hand.
        int real = lay.real[k.slot];
        if (real >= 0 && mc.thePlayer.inventory.currentItem != real) {
            mc.thePlayer.inventory.currentItem = real;
        }
        if (k.throwIt && real >= 0) {
            ItemStack s = mc.thePlayer.inventory.mainInventory[real];
            if (s != null) {
                mc.playerController.sendUseItem(mc.thePlayer, mc.theWorld, s);
            }
        }
        if (k.attack && !onMount) {
            Names.setInt(MISS_DELAY, mc, 0);
            net.minecraft.entity.Entity behind = mc.objectMouseOver != null && mc.objectMouseOver.entityHit instanceof net.minecraft.entity.item.EntityArmorStand
                ? throughStands() : null;
            if (behind != null) {
                // A hologram stand with a hitbox in front of a mob: the server's holograms
                // are not there to be hit, so the blow goes to what is behind.
                mc.thePlayer.swingItem();
                mc.playerController.attackEntity(mc.thePlayer, behind);
            } else {
                KeyBinding.onTick(gs.keyBindAttack.getKeyCode());
            }
        }
        boolean hold = k.useHeld;
        if ((k.claim || (hold && !useDown))) {
            KeyBinding.onTick(gs.keyBindUseItem.getKeyCode());
        }
        key(gs.keyBindUseItem, hold);
        useDown = hold;
    }

    /** The nearest living entity on the line of sight within reach, armor stands left out. */
    private net.minecraft.entity.Entity throughStands() {
        net.minecraft.entity.Entity me = mc.thePlayer;
        net.minecraft.util.Vec3 eye = me.getPositionEyes(1.0F), look = me.getLook(1.0F);
        double reach = mc.playerController.getBlockReachDistance();
        reach = Math.min(reach, 3.0);
        net.minecraft.util.Vec3 end = eye.addVector(look.xCoord * reach, look.yCoord * reach, look.zCoord * reach);
        net.minecraft.entity.Entity best = null;
        double bestD = reach;
        for (Object o : mc.theWorld.loadedEntityList) {
            net.minecraft.entity.Entity e = (net.minecraft.entity.Entity) o;
            if (e == me || e instanceof net.minecraft.entity.item.EntityArmorStand || !(e instanceof net.minecraft.entity.EntityLivingBase) || !e.canBeCollidedWith()) {
                continue;
            }
            float b = e.getCollisionBorderSize();
            net.minecraft.util.AxisAlignedBB box = e.getEntityBoundingBox().expand(b, b, b);
            net.minecraft.util.MovingObjectPosition hit = box.calculateIntercept(eye, end);
            double d = box.isVecInside(eye) ? 0.0 : hit != null ? eye.distanceTo(hit.hitVec) : -1.0;
            if (d >= 0.0 && d < bestD) {
                bestD = d;
                best = e;
            }
        }
        return best;
    }

    void release() {
        KeyBinding.unPressAllKeys();
        useDown = false;
    }
}
