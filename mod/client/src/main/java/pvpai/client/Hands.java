package pvpai.client;

import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.inventory.GuiInventory;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.client.settings.KeyBinding;
import pvpai.core.Inv;
import pvpai.core.Keys;

/**
 * Presses the keys and buttons of one tick the way a keyboard and a mouse
 * would have by the time the game reads them in that tick.
 */
final class Hands {
    private final Minecraft mc;
    /** The right button as the last tick left it. */
    private boolean useDown;

    Hands(Minecraft mc) {
        this.mc = mc;
    }

    /** What a mouse count turns the view by, for the sensitivity that is set. */
    static float mouseFactor(GameSettings gs) {
        float f = gs.mouseSensitivity * 0.6F + 0.2F;
        return f * f * f * 8.0F;
    }

    /** Whether a tick with these keys belongs to the inventory screen, on which the game reads nothing else. */
    boolean screenTick(Keys k) {
        return mc.currentScreen instanceof GuiInventory || k.inv != 0;
    }

    void press(Keys k) {
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
            KeyBinding.onTick(gs.keyBindsHotbar[k.slotKey - 1].getKeyCode());
        }
        // A left click is a press let go at once: the button is never left down.
        if (k.attack) {
            for (int n = 0; n <= k.extraClicks; n++) {
                KeyBinding.onTick(gs.keyBindAttack.getKeyCode());
            }
        }
        // The right button: a press when it goes down, then held.
        if (k.useItem && (!useDown || k.useAgain)) {
            KeyBinding.onTick(gs.keyBindUseItem.getKeyCode());
        }
        key(gs.keyBindUseItem, k.useItem);
        useDown = k.useItem;
        if (k.dx != 0 || k.dy != 0) {
            float f1 = mouseFactor(gs);
            mc.thePlayer.setAngles((float) k.dx * f1, -((float) k.dy * f1));
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
