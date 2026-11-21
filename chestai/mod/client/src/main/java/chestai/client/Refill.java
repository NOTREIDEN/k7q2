package chestai.client;

import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.gui.inventory.GuiInventory;
import net.minecraft.client.settings.KeyBinding;
import net.minecraft.entity.player.InventoryPlayer;
import net.minecraft.init.Items;
import net.minecraft.item.ItemStack;
import net.minecraft.network.play.server.S32PacketConfirmTransaction;

/**
 * Splash potions of healing (and golden apples) brought down from the inventory to the hotbar, which
 * is all the agent uses, the way the PvP AI does it (pvpai.core.Agent.refillKeys, pvpai.client.Hands
 * and Session with InventoryRefill):
 *
 * <ul>
 * <li>asked with the screen shut, the weapon in hand and nothing in use, the inventory key is
 *     pressed (every key let go, as the game leaves them when a screen opens);</li>
 * <li>from the next tick on, the first potion of the inventory rows is shift-clicked (the game puts
 *     it in the first empty hotbar slot), one click at most as the clicks per second allow;</li>
 * <li>each click waits for the server's answer, and no click goes while the player still sprints;</li>
 * <li>the last one shuts the screen in the same tick; one the server refuses, or no answer within a
 *     second, shuts it and the agent plays on.</li>
 * </ul>
 *
 * In the PvP AI its policy asks for a potion; here it is asked while the hotbar has fewer than
 * {@code pociones_barra} potions and the inventory has more, not with a mob at the sword unless the
 * hotbar has none left. With no golden apple in the hotbar, a stack of them is brought down the same way.
 */
final class Refill {
    private static final int POTION = 0, APPLE = 1;
    /** After a refill the server refused or did not answer, this long before the next one. */
    private static final int REFUSED_WAIT = 100;

    private final Minecraft mc;
    private final double cps;
    private final InventoryRefill confirm = new InventoryRefill();
    /** 0: no refill; 1: asked, the inventory key pressed; 2: clicking. */
    private int refill;
    private double clickTokens;
    private long refusedAt = -100000;
    /** A line for the player (a click refused), once. */
    String note;

    Refill(Minecraft mc, double cps) {
        this.mc = mc;
        this.cps = cps;
    }

    /** For the tap on the connection: the server's answer to a click. */
    final Wire.Tap tap = new Wire.Tap() {
        public void after(S32PacketConfirmTransaction p) {
            confirm.confirmed(p);
        }
    };

    /** Whether this screen is the inventory a refill opened. */
    boolean owns(Object screen) {
        return refill != 0 && screen instanceof GuiInventory;
    }

    /** The inventory shut and nothing waited for (the agent stopped). */
    void reset() {
        if (refill != 0 && mc.thePlayer != null && mc.currentScreen instanceof GuiInventory) {
            mc.thePlayer.closeScreen();
        }
        refill = 0;
        confirm.reset();
    }

    /** Heal splash potions (or golden apples) in the inventory above the hotbar. */
    static int stored(InventoryPlayer inv, boolean apples) {
        int n = 0;
        for (int i = 9; i < 36; i++) {
            ItemStack s = inv.mainInventory[i];
            if (s != null && s.stackSize > 0 && is(s, apples ? APPLE : POTION)) {
                n += s.stackSize;
            }
        }
        return n;
    }

    private static boolean is(ItemStack s, int kind) {
        return kind == APPLE ? s.getItem() == Items.golden_apple && s.getMetadata() == 0 : Layout.healSplash(s);
    }

    /**
     * One tick. {@code want} potions kept in the hotbar (0: never); {@code mobClose} a mob at the
     * sword; {@code ready} the weapon in hand and nothing under way (no item in use, no potion or
     * pearl being thrown). Returns whether the tick is the inventory's: the agent does nothing else.
     */
    boolean tick(int want, boolean mobClose, boolean ready) {
        EntityPlayerSP me = mc.thePlayer;
        // The server's answers first (InventoryRefill), as the PvP AI's Session does.
        InventoryRefill.Step step = confirm.step(mc.currentScreen, me);
        switch (step) {
            case WAIT:
            case CLOSE:
                KeyBinding.unPressAllKeys();
                if (step == InventoryRefill.Step.CLOSE) {
                    me.closeScreen();
                    refill = 0;
                }
                return true;
            case REJECTED:
            case TIMED_OUT:
                // The game takes this in stride: the client answers the refusal and the server sends
                // the inventory again. The screen shuts and the agent plays on.
                KeyBinding.unPressAllKeys();
                me.closeScreen();
                refill = 0;
                refusedAt = me.ticksExisted;
                note = step == InventoryRefill.Step.REJECTED
                    ? "Chest AI: el servidor rechazo un clic de inventario: cierro el inventario y sigo."
                    : "Chest AI: no llego la confirmacion de un clic de inventario: cierro el inventario y sigo.";
                return true;
            default:
                break;
        }
        boolean invOpen = mc.currentScreen instanceof GuiInventory;
        if (refill != 0 && !invOpen) {
            if (refill == 1 && mc.currentScreen == null) {
                // The key pressed last tick: the screen opens in this one.
                refill = 2;
                return true;
            }
            refill = 0;
            return false;
        }
        InventoryPlayer inv = me.inventory;
        int kind = wanted(inv, want, mobClose);
        if (refill == 0) {
            if (kind < 0 || !ready || invOpen || mc.currentScreen != null || me.ticksExisted - refusedAt < REFUSED_WAIT) {
                return false;
            }
            refill = 1;
            clickTokens = clicksAvailable();
            // The inventory key, read by the game later in this tick.
            KeyBinding.unPressAllKeys();
            KeyBinding.onTick(mc.gameSettings.keyBindInventory.getKeyCode());
            return true;
        }
        // Every key is up, as the game leaves them when a screen opens.
        KeyBinding.unPressAllKeys();
        refill = 2;
        int slot = kind < 0 ? -1 : click(inv, kind);
        if (slot < 0) {
            // Nothing (more) to bring down: shut.
            refill = 0;
            clickTokens = clicksAvailable();
            me.closeScreen();
            return true;
        }
        if (!takeClick()) {
            return true;
        }
        // What the screen does for a shift click.
        mc.playerController.windowClick(me.openContainer.windowId, slot, 0, 1, me);
        boolean more = wanted(inv, want, mobClose) >= 0;
        if (!more) {
            refill = 0;
            me.closeScreen();
            return true;
        }
        confirm.sent(mc.currentScreen, me.openContainer, false);
        return true;
    }

    /** What is to be brought down now: POTION, APPLE, or -1. */
    private static int wanted(InventoryPlayer inv, int want, boolean mobClose) {
        if (want <= 0 || freeSlot(inv) < 0) {
            return -1;
        }
        int potions = 0;
        boolean apples = false;
        for (int i = 0; i < 9; i++) {
            ItemStack s = inv.mainInventory[i];
            if (s == null) {
                continue;
            }
            potions += is(s, POTION) ? s.stackSize : 0;
            apples |= is(s, APPLE);
        }
        if (mobClose && potions > 0) {
            // In the middle of a fight with a potion still at hand: later.
            return -1;
        }
        if (potions < want && click(inv, POTION) >= 0) {
            return POTION;
        }
        if (!apples && !mobClose && click(inv, APPLE) >= 0) {
            return APPLE;
        }
        return -1;
    }

    /** The window slot (9 to 35) of the first of this kind in the inventory rows, or -1 (Inv.refillClick). */
    private static int click(InventoryPlayer inv, int kind) {
        for (int i = 9; i < 36; i++) {
            ItemStack s = inv.mainInventory[i];
            if (s != null && s.stackSize > 0 && is(s, kind)) {
                // In the player's inventory window the rows are slots 9 to 35, as in the inventory.
                return i;
            }
        }
        return -1;
    }

    private static int freeSlot(InventoryPlayer inv) {
        for (int i = 0; i < 9; i++) {
            if (inv.mainInventory[i] == null) {
                return i;
            }
        }
        return -1;
    }

    /** Clicks the player can still make on its next tick (Agent.clicksAvailable). */
    private double clicksAvailable() {
        double r = Math.min(cps / 20.0, 1.0);
        return Math.min(clickTokens + r, 1.0 + r);
    }

    /** One click of the budget, if there is one (Agent.takeClick). */
    private boolean takeClick() {
        if (cps >= 20.0) {
            return true;
        }
        double t = clicksAvailable();
        boolean can = t >= 1.0 - 1e-9;
        clickTokens = can ? t - 1.0 : t;
        return can;
    }
}
