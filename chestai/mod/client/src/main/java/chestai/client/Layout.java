package chestai.client;

import java.util.List;
import net.minecraft.entity.player.InventoryPlayer;
import net.minecraft.init.Items;
import net.minecraft.item.Item;
import net.minecraft.item.ItemAxe;
import net.minecraft.item.ItemBow;
import net.minecraft.item.ItemFood;
import net.minecraft.item.ItemPotion;
import net.minecraft.item.ItemStack;
import net.minecraft.item.ItemSword;
import net.minecraft.potion.Potion;
import net.minecraft.potion.PotionEffect;
import chestai.core.Pilot;

/**
 * Where the agent's five items are in the hotbar: sword, bow, golden
 * apples, pearls and splash potions of healing, and how many there are;
 * and the best plain food, eaten through the apple's slot when the bar is short.
 */
final class Layout {
    /** Hotbar index of each of the agent's slots, or -1. */
    final int[] real = {-1, -1, -1, -1, -1};
    int gapples, pearls, potions, arrows;
    /** Golden apples and heal splash potions in the inventory above the hotbar (brought down by Refill). */
    int storedGapples, storedPotions;
    /** Hotbar index of the food that fills the bar most (not a golden apple), or -1, and how much of it there is. */
    int food = -1, foodCount;
    private int foodFill;

    /** Food that does no harm: not rotten flesh, a spider eye, a poisonous potato, raw chicken or a pufferfish. */
    static int fill(ItemStack s) {
        Item it = s.getItem();
        if (!(it instanceof ItemFood) || it == Items.golden_apple || it == Items.rotten_flesh || it == Items.spider_eye
                || it == Items.poisonous_potato || it == Items.chicken || (it == Items.fish && s.getMetadata() == 3)) {
            return 0;
        }
        return ((ItemFood) it).getHealAmount(s);
    }

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

    /** A sword or an axe: what the left button fights with. */
    static boolean isMeleeWeapon(ItemStack s) {
        return s != null && s.stackSize > 0 && (s.getItem() instanceof ItemSword || s.getItem() instanceof ItemAxe);
    }

    static Layout of(InventoryPlayer inv) {
        Layout l = new Layout();
        // The first axe, for the sword's slot when there is no sword.
        int axe = -1;
        for (int i = 0; i < 9; i++) {
            ItemStack s = inv.mainInventory[i];
            if (s == null || s.stackSize <= 0) {
                continue;
            }
            Item it = s.getItem();
            if (isMeleeWeapon(s)) {
                // The weapon in the hand, else the first sword.
                if (i == inv.currentItem || (it instanceof ItemSword && l.real[Pilot.SWORD] < 0)) {
                    l.real[Pilot.SWORD] = i;
                } else if (it instanceof ItemAxe && axe < 0) {
                    axe = i;
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
            } else {
                int fill = fill(s);
                if (fill > l.foodFill) {
                    l.foodFill = fill;
                    l.food = i;
                    l.foodCount = s.stackSize;
                }
            }
        }
        if (l.real[Pilot.SWORD] < 0) {
            l.real[Pilot.SWORD] = axe;
        }
        for (ItemStack s : inv.mainInventory) {
            if (s != null && s.getItem() == Items.arrow) {
                l.arrows += s.stackSize;
            }
        }
        l.storedGapples = Refill.stored(inv, true);
        l.storedPotions = Refill.stored(inv, false);
        return l;
    }
}
