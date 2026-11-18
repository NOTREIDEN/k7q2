package pvpai.client;

import java.util.Arrays;
import net.minecraft.entity.player.InventoryPlayer;
import pvpai.core.Inv;

/**
 * Where a player keeps the things of a duel in the hotbar. An agent knows
 * one hotbar: the sword in the first slot, the pearls in the second, the
 * food in the last, potions anywhere. A player's own may be any other way
 * round, so the slots of the two are paired off: the
 * agent's slot of the sword with the slot the player's sword is in, and the
 * same for the pearls and the food; the rest in order. What the agent is
 * shown is read through the pairs and the keys it presses go through them.
 */
final class Layout {
    /** The agent's slot for each slot of the player's hotbar, and the other way round. */
    final int[] agent = new int[9], real = new int[9];
    /** Whether the hotbar has a sword at all. */
    boolean sword;

    private Layout(InventoryPlayer inv) {
        update(inv, false);
    }

    private void map(int swordAt, int pearlAt, int foodAt, boolean sword) {
        this.sword = sword;
        Arrays.fill(agent, -1);
        Arrays.fill(real, -1);
        pair(Inv.SLOT_SWORD, swordAt);
        if (pearlAt >= 0) {
            pair(Inv.SLOT_PEARL, pearlAt);
        }
        if (foodAt >= 0) {
            pair(Inv.SLOT_FOOD, foodAt);
        }
        int a = 0;
        for (int r = 0; r < 9; r++) {
            if (agent[r] >= 0) {
                continue;
            }
            while (real[a] >= 0) {
                a++;
            }
            pair(a, r);
        }
    }

    private void pair(int a, int r) {
        agent[r] = a;
        real[a] = r;
    }

    /** The pairs for this hotbar as it is now, with this slot in hand. */
    static Layout of(InventoryPlayer inv) {
        return new Layout(inv);
    }

    void refresh(InventoryPlayer inv) {
        update(inv, true);
    }

    private void update(InventoryPlayer inv, boolean retain) {
        int oldSword = retain ? real[Inv.SLOT_SWORD] : inv.currentItem;
        int oldPearl = retain ? real[Inv.SLOT_PEARL] : -1;
        int oldFood = retain ? real[Inv.SLOT_FOOD] : -1;
        int sword = -1, pearl = -1, food = -1;
        for (int i = 0; i < 9; i++) {
            int kind = Reader.item(inv.mainInventory[i]);
            if (kind == Inv.SWORD && (sword < 0 || (retain && i == oldSword))) {
                sword = i;
            } else if (kind == Inv.PEARL && (pearl < 0 || i == oldPearl)) {
                pearl = i;
            } else if (kind == Inv.FOOD && (food < 0 || i == oldFood)) {
                food = i;
            }
        }
        boolean has = sword >= 0;
        if (!has) {
            // Whatever is in hand stands in for it, if nothing else of the duel is there.
            sword = oldSword != pearl && oldSword != food ? oldSword : 0;
            while (sword == pearl || sword == food) {
                sword++;
            }
        }
        // Keep an empty reserved slot unless an actual item now has that slot.
        if (pearl < 0 && oldPearl != sword && oldPearl != food) {
            pearl = oldPearl;
        }
        if (food < 0 && oldFood != sword && oldFood != pearl) {
            food = oldFood;
        }
        if (!retain || sword != oldSword || pearl != oldPearl || food != oldFood) {
            map(sword, pearl, food, has);
        } else {
            this.sword = has;
        }
    }

    /** The player's slot of the pearls or of the food, counted from one; zero if the hotbar has none. */
    int shown(InventoryPlayer inv, int slot, int kind) {
        int r = real[slot];
        return Reader.item(inv.mainInventory[r]) == kind ? r + 1 : 0;
    }
}
