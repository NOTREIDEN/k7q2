package pvpai.client;

import java.lang.reflect.Field;
import java.util.List;
import net.minecraft.block.state.IBlockState;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityOtherPlayerMP;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.gui.inventory.GuiInventory;
import net.minecraft.entity.Entity;
import net.minecraft.entity.EntityLivingBase;
import net.minecraft.entity.SharedMonsterAttributes;
import net.minecraft.entity.item.EntityEnderPearl;
import net.minecraft.entity.projectile.EntityEgg;
import net.minecraft.entity.projectile.EntityFishHook;
import net.minecraft.entity.projectile.EntityPotion;
import net.minecraft.entity.projectile.EntitySnowball;
import net.minecraft.init.Items;
import net.minecraft.item.Item;
import net.minecraft.item.ItemFood;
import net.minecraft.item.ItemPotion;
import net.minecraft.item.ItemStack;
import net.minecraft.item.ItemSword;
import net.minecraft.potion.Potion;
import net.minecraft.potion.PotionEffect;
import net.minecraft.util.AxisAlignedBB;
import net.minecraft.util.BlockPos;
import net.minecraft.util.MovingObjectPosition;
import net.minecraft.util.Vec3;
import pvpai.core.Blocks;
import pvpai.core.Inv;
import pvpai.core.View;

/**
 * Reads out of the game client what an agent goes by: a {@link View} but
 * for the counts a {@link pvpai.core.Book} keeps. To be called when the
 * packets are in and the tick is to come.
 */
final class Reader {
    private static final Field JUMP_TICKS = Names.field(EntityLivingBase.class, "jumpTicks", "field_70773_bE");
    private static final Field SERVER_SPRINT = Names.field(EntityPlayerSP.class, "serverSprintState", "field_175171_bO");
    private static final Field TARGET_X = Names.field(EntityOtherPlayerMP.class, "otherPlayerMPX", "field_71185_c");
    private static final Field TARGET_Y = Names.field(EntityOtherPlayerMP.class, "otherPlayerMPY", "field_71182_d");
    private static final Field TARGET_Z = Names.field(EntityOtherPlayerMP.class, "otherPlayerMPZ", "field_71183_e");
    private static final Field INCREMENTS = Names.field(EntityOtherPlayerMP.class, "otherPlayerMPPosRotationIncrements", "field_71184_b");
    private static final Field SAID_X = Names.field(EntityPlayerSP.class, "lastReportedPosX", "field_175172_bI");
    private static final Field SAID_Y = Names.field(EntityPlayerSP.class, "lastReportedPosY", "field_175166_bJ");
    private static final Field SAID_Z = Names.field(EntityPlayerSP.class, "lastReportedPosZ", "field_175167_bK");
    private static final Field SAID_YAW = Names.field(EntityPlayerSP.class, "lastReportedYaw", "field_175164_bL");
    private static final Field SAID_PITCH = Names.field(EntityPlayerSP.class, "lastReportedPitch", "field_175165_bM");
    private static final Field CLICK_DELAY = Names.field(Minecraft.class, "rightClickDelayTimer", "field_71467_ac");

    private Reader() {
    }

    /**
     * What a stack of the player's own is to an agent, an item of
     * {@link Inv}. Of the potions, the ones an agent knows what to do
     * with: a splash potion that heals is thrown and one of Speed is
     * drunk. Any other potion, a golden apple and whatever else a duel of
     * training does not have only takes up its slot.
     */
    static int item(ItemStack s) {
        if (s == null || s.stackSize <= 0) {
            return Inv.EMPTY;
        }
        Item it = s.getItem();
        if (it instanceof ItemSword) {
            return Inv.SWORD;
        }
        if (it == Items.ender_pearl) {
            return Inv.PEARL;
        }
        if (it == Items.potionitem) {
            boolean splash = ItemPotion.isSplash(s.getMetadata());
            List<PotionEffect> effects = Items.potionitem.getEffects(s);
            if (effects != null) {
                for (PotionEffect e : effects) {
                    if (splash && e.getPotionID() == Potion.heal.id) {
                        return Inv.POTION;
                    }
                    if (!splash && e.getPotionID() == Potion.moveSpeed.id) {
                        return Inv.DRINK;
                    }
                }
            }
            return Inv.OTHER;
        }
        if (it == Items.glass_bottle) {
            return Inv.BOTTLE;
        }
        if (it instanceof ItemFood && it != Items.golden_apple) {
            return Inv.FOOD;
        }
        return tool(it, Inv.OTHER);
    }

    /** A fishing rod, snowballs or eggs as the tool of a kit, else {@code or}. */
    private static int tool(Item it, int or) {
        if (it == Items.fishing_rod) {
            return Inv.ROD;
        }
        if (it == Items.snowball) {
            return Inv.SNOWBALL;
        }
        if (it == Items.egg) {
            return Inv.EGG;
        }
        return or;
    }

    /** What the other player is seen to hold: thrown, drunk or eaten, whatever it does. */
    static int shown(ItemStack s) {
        if (s == null || s.stackSize <= 0) {
            return Inv.EMPTY;
        }
        Item it = s.getItem();
        if (it instanceof ItemSword) {
            return Inv.SWORD;
        }
        if (it == Items.ender_pearl) {
            return Inv.PEARL;
        }
        if (it == Items.potionitem) {
            return ItemPotion.isSplash(s.getMetadata()) ? Inv.POTION : Inv.DRINK;
        }
        if (it == Items.glass_bottle) {
            return Inv.BOTTLE;
        }
        if (it instanceof ItemFood) {
            return Inv.FOOD;
        }
        return tool(it, Inv.EMPTY);
    }

    /** {@code Minecraft.rightClickDelayTimer} as it stands. */
    static int clickDelay(Minecraft mc) {
        return Names.getInt(CLICK_DELAY, mc);
    }

    /** The place the server was last told this player is at. */
    static double[] said(EntityPlayerSP p) {
        return new double[] {Names.getDouble(SAID_X, p), Names.getDouble(SAID_Y, p), Names.getDouble(SAID_Z, p)};
    }

    static float saidYaw(EntityPlayerSP p) {
        return Names.getFloat(SAID_YAW, p);
    }

    static float saidPitch(EntityPlayerSP p) {
        return Names.getFloat(SAID_PITCH, p);
    }

    /** Where the copy of the other player is heading. */
    static double[] target(EntityOtherPlayerMP o) {
        return new double[] {Names.getDouble(TARGET_X, o), Names.getDouble(TARGET_Y, o), Names.getDouble(TARGET_Z, o)};
    }

    static double[] pos(Entity e) {
        return new double[] {e.posX, e.posY, e.posZ};
    }

    static boolean thrown(Entity e) {
        return e instanceof EntityEnderPearl || e instanceof EntityPotion || e instanceof EntitySnowball || e instanceof EntityEgg
            || e instanceof EntityFishHook;
    }

    /** The kind of a thrown thing, see {@link Inv#KIND_PEARL}. */
    static int kind(Entity e) {
        if (e instanceof EntityEnderPearl) {
            return Inv.KIND_PEARL;
        }
        if (e instanceof EntitySnowball) {
            return Inv.KIND_SNOWBALL;
        }
        if (e instanceof EntityEgg) {
            return Inv.KIND_EGG;
        }
        if (e instanceof EntityFishHook) {
            return Inv.KIND_HOOK;
        }
        return Inv.KIND_POTION;
    }

    /** The kind of a thrown thing by the type its spawn packet gives, or -1 for none of them. */
    static int kindOfSpawn(int type) {
        switch (type) {
            case 65:
                return Inv.KIND_PEARL;
            case 73:
                return Inv.KIND_POTION;
            case 61:
                return Inv.KIND_SNOWBALL;
            case 62:
                return Inv.KIND_EGG;
            case 90:
                return Inv.KIND_HOOK;
            default:
                return -1;
        }
    }

    /** The other player nearest to this one, or null. */
    static EntityOtherPlayerMP nearest(Minecraft mc) {
        EntityOtherPlayerMP best = null;
        double least = Double.MAX_VALUE;
        for (Object o : mc.theWorld.playerEntities) {
            if (o instanceof EntityOtherPlayerMP) {
                double d = ((EntityOtherPlayerMP) o).getDistanceSqToEntity(mc.thePlayer);
                if (d < least) {
                    least = d;
                    best = (EntityOtherPlayerMP) o;
                }
            }
        }
        return best;
    }

    /**
     * The view of this moment. clickDelay is the click delay as the last
     * tick left it: by the time a mod is called in a tick the game has
     * already counted it down. The hotbar is told in the agent's slots.
     */
    static View read(Minecraft mc, EntityOtherPlayerMP o, int clickDelay, Layout layout) {
        EntityPlayerSP p = mc.thePlayer;
        View v = new View();
        v.pos[0] = p.posX;
        v.pos[1] = p.posY;
        v.pos[2] = p.posZ;
        v.motion[0] = p.motionX;
        v.motion[1] = p.motionY;
        v.motion[2] = p.motionZ;
        v.onGround = p.onGround;
        v.collidedH = p.isCollidedHorizontally;
        v.fallDistance = p.fallDistance;
        v.yaw = p.rotationYaw;
        v.pitch = p.rotationPitch;
        v.sprinting = p.isSprinting();
        v.jumpTicks = Names.getInt(JUMP_TICKS, p);
        v.speed = p.getEntityAttribute(SharedMonsterAttributes.movementSpeed).getAttributeValue();
        v.serverSprint = Names.getBoolean(SERVER_SPRINT, p);
        v.hurtTime = p.hurtTime;
        v.usingItem = p.isUsingItem();
        v.rightClickDelay = clickDelay;
        v.flagUsing = p.isEating();
        v.held = layout.agent[p.inventory.currentItem];
        for (int r = 0; r < 9; r++) {
            ItemStack s = p.inventory.mainInventory[r];
            int kind = item(s);
            int i = layout.agent[r];
            if (kind == Inv.POTION) {
                v.potions |= 1 << i;
            } else if (kind == Inv.DRINK) {
                v.drinks |= 1 << i;
            } else if (kind == Inv.BOTTLE) {
                v.bottles |= 1 << i;
            } else if (kind == Inv.PEARL && i == Inv.SLOT_PEARL) {
                v.pearls = s.stackSize;
            } else if (kind == Inv.FOOD && i == Inv.SLOT_FOOD) {
                v.steak = s.stackSize;
            } else if ((kind == Inv.ROD || kind == Inv.SNOWBALL || kind == Inv.EGG) && v.tool == Inv.EMPTY && i != Inv.SLOT_SWORD) {
                // The first of them is the tool; a rod counts as one, and wears.
                v.tool = kind;
                v.toolSlot = i;
                v.toolCount = kind == Inv.ROD ? 1 : s.stackSize;
                v.toolWear = kind == Inv.ROD ? s.getItemDamage() : 0;
            } else if (kind != Inv.EMPTY && i != Inv.SLOT_SWORD) {
                // More pearls, more food, another sword or something else.
                v.others |= 1 << i;
            }
        }
        v.food = p.getFoodStats().getFoodLevel();
        v.saturation = p.getFoodStats().getSaturationLevel();
        PotionEffect speed = p.getActivePotionEffect(Potion.moveSpeed);
        v.speedTicks = speed == null ? -1 : speed.getDuration();
        v.health = p.getHealth();
        v.useSlot = v.usingItem ? v.held : 0;
        v.useCount = v.usingItem ? p.getItemInUseCount() : 0;
        v.invOpen = mc.currentScreen instanceof GuiInventory;
        for (int i = 0; i < 27; i++) {
            ItemStack s = p.inventory.mainInventory[9 + i];
            int kind = item(s);
            v.rows[i] = s == null || s.stackSize <= 0 ? 0 : kind == Inv.POTION ? 1 : kind == Inv.DRINK ? 2 : kind == Inv.BOTTLE ? 2 + s.stackSize : 255;
        }

        System.arraycopy(pos(o), 0, v.oPos, 0, 3);
        v.oPrevPos[0] = o.prevPosX;
        v.oPrevPos[1] = o.prevPosY;
        v.oPrevPos[2] = o.prevPosZ;
        System.arraycopy(target(o), 0, v.oTarget, 0, 3);
        v.oIncrements = Names.getInt(INCREMENTS, o);
        AxisAlignedBB bb = o.getEntityBoundingBox();
        v.oMin[0] = bb.minX;
        v.oMin[1] = bb.minY;
        v.oMin[2] = bb.minZ;
        v.oMax[0] = bb.maxX;
        v.oMax[1] = bb.maxY;
        v.oMax[2] = bb.maxZ;
        v.oOnGround = o.onGround;
        v.oSprinting = o.isSprinting();
        v.oHurtTime = o.hurtTime;
        v.oSwinging = o.isSwingInProgress;
        v.oSwingProgress = o.swingProgressInt;
        v.oYaw = o.rotationYaw;
        v.oPitch = o.rotationPitch;
        v.oUsing = o.isEating();
        v.oItem = shown(o.getHeldItem());
        v.oHealth = o.getHealth();
        v.oFx = o.getDataWatcher().getWatchableObjectInt(7) != 0;

        // What the crosshair is on if the mouse does not move: the game
        // works this out again at the start of its tick.
        mc.entityRenderer.getMouseOver(1.0F);
        MovingObjectPosition m = mc.objectMouseOver;
        v.onTarget = m != null && m.typeOfHit == MovingObjectPosition.MovingObjectType.ENTITY && m.entityHit == o;

        for (Object e : mc.theWorld.loadedEntityList) {
            if (!thrown((Entity) e)) {
                continue;
            }
            Entity t = (Entity) e;
            View.Thrown x = new View.Thrown();
            x.id = t.getEntityId();
            x.kind = kind(t);
            if (x.kind == Inv.KIND_HOOK) {
                // Where the server last said it is: the client's copy moves in ways of its own.
                x.pos[0] = (double) t.serverPosX / 32.0;
                x.pos[1] = (double) t.serverPosY / 32.0;
                x.pos[2] = (double) t.serverPosZ / 32.0;
            } else {
                System.arraycopy(pos(t), 0, x.pos, 0, 3);
                x.motion[0] = t.motionX;
                x.motion[1] = t.motionY;
                x.motion[2] = t.motionZ;
            }
            v.thrown.add(x);
        }
        return v;
    }

    /** The blocks of the world the client is in. */
    static Blocks blocks(final Minecraft mc) {
        return new Blocks() {
            public double[] rayTrace(double[] a, double[] b) {
                MovingObjectPosition m = mc.theWorld.rayTraceBlocks(new Vec3(a[0], a[1], a[2]), new Vec3(b[0], b[1], b[2]));
                return m == null ? null : new double[] {m.hitVec.xCoord, m.hitVec.yCoord, m.hitVec.zCoord};
            }

            public double height(int x, int y, int z) {
                BlockPos p = new BlockPos(x, y, z);
                IBlockState st = mc.theWorld.getBlockState(p);
                AxisAlignedBB bb = st.getBlock().getCollisionBoundingBox(mc.theWorld, p, st);
                if (bb == null) {
                    return 0.0;
                }
                double h = bb.maxY - (double) y;
                return h <= 0.0 ? 0.0 : Math.min(h, 1.5);
            }

            public boolean web(int x, int y, int z) {
                return mc.theWorld.getBlockState(new BlockPos(x, y, z)).getBlock() == net.minecraft.init.Blocks.web;
            }
        };
    }
}
