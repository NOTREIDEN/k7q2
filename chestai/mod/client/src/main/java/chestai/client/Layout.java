package chestai.client;

import java.util.List;
import net.minecraft.entity.player.InventoryPlayer;
import net.minecraft.init.Items;
import net.minecraft.item.Item;
import net.minecraft.item.ItemBow;
import net.minecraft.item.ItemPotion;
import net.minecraft.item.ItemStack;
import net.minecraft.item.ItemSword;
import net.minecraft.potion.Potion;
import net.minecraft.potion.PotionEffect;
import chestai.core.Pilot;

/**
 * Where the agent's five items are in the hotbar: sword, bow, golden
 * apples, pearls and splash potions of healing, and how many there are.
 */
final class Layout {
    /** Hotbar index of each of the agent's slots, or -1. */
    final int[] real = {-1, -1, -1, -1, -1};
    int gapples, pearls, potions, arrows;

    static boolean healSplash(ItemStack s) {
        if (s == null || s.getItem() != Items.potionitem || !ItemPotion.isSplash(s.getMetadata())) {
            return false;
        }
        List<PotionEffect> effects = Items.potionitem.getEffects(s);
        if (effects != null) {
            for (PotionEffect e : effects) {
                if (e.getPotionID() == Potion.heal.id) {
                    return true;
                }
            }
        }
        return false;
    }

    static Layout of(InventoryPlayer inv) {
        Layout l = new Layout();
        for (int i = 0; i < 9; i++) {
            ItemStack s = inv.mainInventory[i];
            if (s == null || s.stackSize <= 0) {
                continue;
            }
            Item it = s.getItem();
            if (it instanceof ItemSword) {
                if (l.real[Pilot.SWORD] < 0 || i == inv.currentItem) {
                    l.real[Pilot.SWORD] = i;
                }
            } else if (it instanceof ItemBow) {
                if (l.real[Pilot.BOW] < 0) {
                    l.real[Pilot.BOW] = i;
                }
            } else if (it == Items.golden_apple && s.getMetadata() == 0) {
                l.gapples += s.stackSize;
                if (l.real[Pilot.GAPPLE] < 0) {
                    l.real[Pilot.GAPPLE] = i;
                }
            } else if (it == Items.ender_pearl) {
                l.pearls += s.stackSize;
                if (l.real[Pilot.PEARL] < 0) {
                    l.real[Pilot.PEARL] = i;
                }
            } else if (healSplash(s)) {
                l.potions += s.stackSize;
                if (l.real[Pilot.POTION] < 0) {
                    l.real[Pilot.POTION] = i;
                }
            }
        }
        for (ItemStack s : inv.mainInventory) {
            if (s != null && s.getItem() == Items.arrow) {
                l.arrows += s.stackSize;
            }
        }
        return l;
    }
}
