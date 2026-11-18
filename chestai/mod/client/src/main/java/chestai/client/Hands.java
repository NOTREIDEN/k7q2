package chestai.client;

import net.minecraft.client.Minecraft;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.client.settings.KeyBinding;
import chestai.core.Pilot;

/**
 * Presses the keys of one tick as a keyboard and a mouse would have by
 * the time the game reads them in that tick (the event at the start of
 * the tick comes before the game reads its keys and works out what the
 * crosshair is on).
 */
final class Hands {

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
            // What the crosshair is on after this turn, as the game works it out
            // later in the tick before it reads the buttons.
            mc.entityRenderer.getMouseOver(1.0F);
        }

        // The hand.
        int real = lay.real[k.slot];
        if (real >= 0 && mc.thePlayer.inventory.currentItem != real) {
            mc.thePlayer.inventory.currentItem = real;
        }
        // Everything goes through the game's own key handling, as a player's keys
        // would: no packets of our own, no reaching past what the crosshair is on.
        boolean throwNow = k.throwIt && real >= 0 && mc.thePlayer.inventory.mainInventory[real] != null;
        if (throwNow) {
            KeyBinding.onTick(gs.keyBindUseItem.getKeyCode());
        }
        // A click into the air costs half a second without clicks in 1.8, and the
        // simulator's air clicks do nothing: the left button is only pressed when
        // the crosshair is on an entity.
        boolean onEntity = mc.objectMouseOver != null
            && mc.objectMouseOver.typeOfHit == net.minecraft.util.MovingObjectPosition.MovingObjectType.ENTITY;
        if (k.attack && !onMount && onEntity) {
            KeyBinding.onTick(gs.keyBindAttack.getKeyCode());
        }
        boolean hold = k.useHeld;
        if (!throwNow && (k.claim || (hold && !useDown))) {
            KeyBinding.onTick(gs.keyBindUseItem.getKeyCode());
        }
        key(gs.keyBindUseItem, hold);
        useDown = hold;
    }

    void release() {
        KeyBinding.unPressAllKeys();
        useDown = false;
    }
}
