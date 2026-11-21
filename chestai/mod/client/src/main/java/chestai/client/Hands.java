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
    /** The view before and after this tick's turn, for drawing the turn smoothly over the frames. */
    private float fromYaw, fromPitch, toYaw, toPitch;
    private boolean turned;
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

        // The view, in whole mouse counts.
        float f1 = mouseFactor(gs);
        double x = k.dyaw / (0.15 * f1) + carryX, y = k.dpitch / (0.15 * f1) + carryY;
        long nx = Math.round(x), ny = Math.round(y);
        carryX = x - nx;
        carryY = y - ny;
        fromYaw = mc.thePlayer.rotationYaw;
        fromPitch = mc.thePlayer.rotationPitch;
        if (nx != 0 || ny != 0) {
            mc.thePlayer.setAngles(nx * f1, -(ny * f1));
            // What the crosshair is on after this turn, as the game works it out
            // later in the tick before it reads the buttons.
            mc.entityRenderer.getMouseOver(1.0F);
        }

        toYaw = mc.thePlayer.rotationYaw;
        toPitch = mc.thePlayer.rotationPitch;
        turned = true;

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
        // The left button is not pressed here: the blows go through SilentAim
        // (VapeCombat), at the end of the tick's start, onto the mob it aims at.
        boolean hold = k.useHeld;
        if (!throwNow && (k.claim || (hold && !useDown))) {
            KeyBinding.onTick(gs.keyBindUseItem.getKeyCode());
        }
        key(gs.keyBindUseItem, hold);
        useDown = hold;
    }

    /**
     * How far the drawn view is from the real one at this point of the tick:
     * the game turns the view once a tick, a mouse turns it all through the
     * frames, so the camera is drawn going from the old view to the new one.
     * Only the picture changes; the game and the server see the same turns.
     */
    float[] cameraOffset(float partial) {
        if (!turned) {
            return null;
        }
        float p = Math.max(0f, Math.min(1f, partial));
        return new float[] {(fromYaw - toYaw) * (1f - p), (fromPitch - toPitch) * (1f - p)};
    }

    void release() {
        KeyBinding.unPressAllKeys();
        useDown = false;
    }

    /** The keys left as they are (another one let go of them), the right button taken as up. */
    void forgetUse() {
        useDown = false;
    }
}
