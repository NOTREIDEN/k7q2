package pvpai.client;

import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.inventory.GuiInventory;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.client.settings.KeyBinding;
import net.minecraft.entity.Entity;
import net.minecraft.util.MovingObjectPosition;
import pvpai.core.Inv;
import pvpai.core.Keys;

/**
 * Presses the keys and buttons of one tick the way a keyboard and a mouse
 * would have by the time the game reads them in that tick.
 */
final class Hands {
    private final Minecraft mc;
    private final Layout layout;
    /** The right button as the last tick left it. */
    private boolean useDown;

    Hands(Minecraft mc, Layout layout) {
        this.mc = mc;
        this.layout = layout;
    }

    /** What a mouse count turns the view by, for the sensitivity that is set. */
    static float mouseFactor(GameSettings gs) {
        float f = gs.mouseSensitivity * 0.6F + 0.2F;
        return f * f * f * 8.0F;
    }

    /**
     * The same at the sensitivity the agent was trained with, the game's
     * default (shown as 100 %): the agent's mouse counts are counted there.
     */
    static final float TRAINED_FACTOR = 1.0F;
    private static final int TRAINED_COUNTS_PER_TURN = 2400;

    /** The part of a count each way that the sensitivity that is set could not make yet. */
    private double carryX, carryY;

    /**
     * The agent's counts as counts of a mouse at the sensitivity that is
     * set, which turn the view as far: whole counts, as a mouse makes them,
     * and what is left over goes with the next.
     */
    private int[] userCounts(int dx, int dy, float f1) {
        double x = (double) dx * TRAINED_FACTOR / f1 + carryX, y = (double) dy * TRAINED_FACTOR / f1 + carryY;
        int nx = (int) Math.round(x), ny = (int) Math.round(y);
        carryX = x - nx;
        carryY = y - ny;
        return new int[] {nx, ny};
    }

    /** Whether a tick with these keys belongs to the inventory screen, on which the game reads nothing else. */
    boolean screenTick(Keys k) {
        return mc.currentScreen instanceof GuiInventory || k.inv != 0;
    }

    void press(Keys k) {
        press(k, null);
    }

    void press(Keys k, Entity opponent) {
        GameSettings gs = mc.gameSettings;
        boolean open = mc.currentScreen instanceof GuiInventory;
        if (open || k.inv != 0) {
            // Every key is up, as the game leaves them when a screen opens.
            KeyBinding.unPressAllKeys();
            useDown = false;
            if (!open) {
                // The inventory key, read by the game later in this tick.
                KeyBinding.onTick(gs.keyBindInventory.getKeyCode());
                return;
            }
            int slot = 0, last = Inv.WINDOW_BAR + 8;
            boolean shut = k.inv == 0;
            if (k.inv >= Inv.WINDOW_ROWS && k.inv <= last) {
                slot = k.inv;
            } else if (k.inv >= Inv.SHUT + Inv.WINDOW_ROWS && k.inv <= Inv.SHUT + last) {
                slot = k.inv - Inv.SHUT;
                shut = true;
            }
            if (slot >= Inv.WINDOW_BAR) {
                // A slot of the hotbar: the player's own for the agent's.
                slot = Inv.WINDOW_BAR + layout.real[slot - Inv.WINDOW_BAR];
            }
            if (slot != 0) {
                // What the screen does for a shift click.
                mc.playerController.windowClick(mc.thePlayer.openContainer.windowId, slot, 0, 1, mc.thePlayer);
            }
            if (shut) {
                mc.thePlayer.closeScreen();
            }
            return;
        }
        key(gs.keyBindForward, k.forward > 0);
        key(gs.keyBindBack, k.forward < 0);
        key(gs.keyBindLeft, k.strafe > 0);
        key(gs.keyBindRight, k.strafe < 0);
        key(gs.keyBindJump, k.jump);
        key(gs.keyBindSprint, k.sprintKey);
        // A number key: the game takes it before the clicks of the same tick.
        if (k.slotKey > 0) {
            KeyBinding.onTick(gs.keyBindsHotbar[layout.real[k.slotKey - 1]].getKeyCode());
        }
        // The right button: a press when it goes down, then held.
        if (k.useItem && (!useDown || k.useAgain)) {
            KeyBinding.onTick(gs.keyBindUseItem.getKeyCode());
        }
        key(gs.keyBindUseItem, k.useItem);
        useDown = k.useItem;

        // The policy can add several horizontal turns in one tick. Apply the
        // shortest turn to the same heading before rounding to mouse counts.
        k.dx %= TRAINED_COUNTS_PER_TURN;
        if (Math.abs(k.dx) > TRAINED_COUNTS_PER_TURN / 2) {
            k.dx -= Integer.signum(k.dx) * TRAINED_COUNTS_PER_TURN;
        }
        if (k.dx != 0 || k.dy != 0) {
            float f1 = mouseFactor(gs);
            int[] n = userCounts(k.dx, k.dy, f1);
            if (n[0] != 0 || n[1] != 0) {
                mc.thePlayer.setAngles((float) n[0] * f1, -((float) n[1] * f1));
            }
        }
        if (opponent != null) {
            mc.thePlayer.setRotationYawHead(mc.thePlayer.rotationYaw);
            if (k.attack) {
                mc.entityRenderer.getMouseOver(1.0F);
                MovingObjectPosition hit = mc.objectMouseOver;
                k.attack = hit != null && hit.typeOfHit == MovingObjectPosition.MovingObjectType.ENTITY
                    && hit.entityHit == opponent;
            }
        }
        // A left click is a press let go at once: the button is never left down.
        if (k.attack) {
            for (int n = 0; n <= k.extraClicks; n++) {
                KeyBinding.onTick(gs.keyBindAttack.getKeyCode());
            }
        }
    }

    /** Lets every key go. */
    void release() {
        KeyBinding.unPressAllKeys();
        useDown = false;
    }

    private static void key(KeyBinding k, boolean down) {
        KeyBinding.setKeyBindState(k.getKeyCode(), down);
    }
}
