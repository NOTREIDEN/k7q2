package chestai.client;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.enchantment.Enchantment;
import net.minecraft.enchantment.EnchantmentHelper;
import net.minecraft.entity.Entity;
import net.minecraft.entity.EntityList;
import net.minecraft.entity.EntityLivingBase;
import net.minecraft.entity.item.EntityArmorStand;
import net.minecraft.entity.item.EntityEnderPearl;
import net.minecraft.entity.monster.EntityGolem;
import net.minecraft.entity.monster.EntityMob;
import net.minecraft.entity.monster.EntitySkeleton;
import net.minecraft.entity.monster.EntitySlime;
import net.minecraft.entity.monster.EntitySnowman;
import net.minecraft.entity.passive.EntityHorse;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.entity.projectile.EntityArrow;
import net.minecraft.entity.projectile.EntityPotion;
import net.minecraft.entity.projectile.EntitySnowball;
import net.minecraft.init.Blocks;
import net.minecraft.block.Block;
import net.minecraft.potion.Potion;
import net.minecraft.potion.PotionEffect;
import net.minecraft.util.BlockPos;
import net.minecraft.util.MovingObjectPosition;
import net.minecraft.world.World;
import chestai.core.Frame;
import chestai.core.HoloText;
import chestai.core.Memory;
import chestai.core.Pilot;

/**
 * Reads out of the game client what the agent goes by, in the simulator's
 * terms: the player, the mobs (a skeleton on a horse is one mob, a mob
 * riding the player is one on its head), the chests and what their
 * holograms say, other players and projectiles.
 */
final class Reader {
    /** How far under a hologram its chest may be. */
    private static final int HOLO_DEPTH = 8;

    private Reader() {
    }

    private static int effect(EntityPlayerSP p, Potion potion) {
        PotionEffect e = p.getActivePotionEffect(potion);
        return e == null ? 0 : e.getDuration();
    }

    static boolean isChest(World w, BlockPos p) {
        Block b = w.getBlockState(p).getBlock();
        // A chest, or a head made to look like one, as some servers use.
        return b == Blocks.chest || b == Blocks.trapped_chest || b == Blocks.ender_chest || b == Blocks.skull;
    }

    /** The chest under a hologram stand, or null. */
    static BlockPos chestUnder(World w, Entity stand) {
        int x = (int) Math.floor(stand.posX), z = (int) Math.floor(stand.posZ);
        int y0 = (int) Math.floor(stand.posY);
        for (int y = y0 + 1; y >= y0 - HOLO_DEPTH; y--) {
            BlockPos p = new BlockPos(x, y, z);
            if (isChest(w, p)) {
                return p;
            }
        }
        // A stand set off the chest's centre line.
        for (int y = y0 + 1; y >= y0 - HOLO_DEPTH; y--) {
            for (int dx = -1; dx <= 1; dx++) {
                for (int dz = -1; dz <= 1; dz++) {
                    BlockPos p = new BlockPos(x + dx, y, z + dz);
                    if (isChest(w, p)) {
                        return p;
                    }
                }
            }
        }
        // Any other block that can be clicked straight under the hologram (a
        // resource pack can make anything look like a chest): the first solid one
        // that is not the ground's grass.
        for (int y = y0; y >= y0 - HOLO_DEPTH; y--) {
            BlockPos p = new BlockPos(x, y, z);
            Block b = w.getBlockState(p).getBlock();
            if (b.getMaterial() != net.minecraft.block.material.Material.air && b != Blocks.tallgrass && b != Blocks.double_plant
                && b != Blocks.carpet && b != Blocks.snow_layer) {
                // The ground itself: the "chest" is something drawn over it (an entity
                // of the server's), so the spot is the air block on the ground.
                return b == Blocks.grass || b == Blocks.dirt || b == Blocks.stone ? p.up() : p;
            }
        }
        return null;
    }

    /** Kind of a mob of a wave, or -1 for an entity that is none. */
    static int kind(Entity e, EntityPlayerSP me, Settings s) {
        if (e instanceof EntityArmorStand || e instanceof EntityPlayer || !(e instanceof EntityLivingBase)) {
            return -1;
        }
        // Invisible ones are the server's own (hologram carriers, markers), never a wave's.
        if (e.isInvisible()) {
            return -1;
        }
        String name = EntityList.getEntityString(e);
        if (name != null && s.ignore.contains(name.toLowerCase(Locale.ROOT))) {
            return -1;
        }
        if (e.ridingEntity == me) {
            return Frame.RIDER;
        }
        if (e instanceof EntityHorse) {
            return e.riddenByEntity instanceof EntityMob ? Frame.HORSEMAN : -1;
        }
        if (e instanceof EntitySkeleton && e.ridingEntity instanceof EntityHorse) {
            // Seen through its horse.
            return -1;
        }
        if (e instanceof EntitySnowman) {
            return Frame.GOLEM;
        }
        if (e instanceof net.minecraft.entity.passive.EntityWolf) {
            // The wolves of a wave are wild; a tamed one is someone's dog.
            return ((net.minecraft.entity.passive.EntityWolf) e).isTamed() ? -1 : Frame.ZOMBIE;
        }
        if (!(e instanceof EntityMob || e instanceof EntitySlime || e instanceof EntityGolem)) {
            // Waves can be of passive animals too; those count only near a chest (see read).
            return passive(e) ? Frame.ZOMBIE : -1;
        }
        if (e instanceof EntitySkeleton) {
            // On foot (with armour or a head or not): it comes at the player like a zombie.
            return Frame.ZOMBIE;
        }
        if (e.width <= 0.5f && e.height <= 1.2f) {
            return Frame.RIDER;
        }
        return Frame.ZOMBIE;
    }

    /**
     * The frame of this tick. Holograms over chests are matched to the
     * chests of {@code mem}, which learns of every chest that shows one.
     */
    static Frame read(Minecraft mc, Memory mem, Pilot pilot, Layout lay, Settings s, long tick, double pingMs, int pearlCdLeft) {
        EntityPlayerSP me = mc.thePlayer;
        World w = mc.theWorld;
        Frame f = new Frame();
        f.tick = tick;
        f.x = me.posX;
        f.y = me.posY;
        f.z = me.posZ;
        f.mx = me.motionX;
        f.my = me.motionY;
        f.mz = me.motionZ;
        f.yaw = me.rotationYaw;
        f.pitch = me.rotationPitch;
        f.onGround = me.onGround;
        f.collidedH = me.isCollidedHorizontally;
        f.sprinting = me.isSprinting();
        f.air = me.getAir();
        f.headInWater = me.isInsideOfMaterial(net.minecraft.block.material.Material.water);
        f.inLava = me.isInLava();
        f.burning = me.isBurning();
        f.inWall = me.isEntityInsideOpaqueBlock();
        // The simulator's player has 20 health: a kit with more counts in twentieths.
        float hpScale = me.getMaxHealth() > 20.0f ? 20.0f / me.getMaxHealth() : 1.0f;
        f.hp = me.getHealth() * hpScale;
        f.absorption = me.getAbsorptionAmount() * hpScale;
        f.food = me.getFoodStats().getFoodLevel();
        f.saturation = me.getFoodStats().getSaturationLevel();
        f.regenLeft = effect(me, Potion.regeneration);
        f.slowLeft = effect(me, Potion.moveSlowdown);
        f.blindLeft = effect(me, Potion.blindness);
        f.gapples = lay.gapples;
        f.pearls = lay.pearls;
        f.potions = lay.potions;
        f.arrows = lay.arrows;
        f.pearlCdLeft = pearlCdLeft;
        f.slot = pilot.slot;
        f.using = pilot.using;
        f.useTicks = pilot.useTicks;
        f.tokens = pilot.tokens;
        PotionEffect sp = me.getActivePotionEffect(Potion.moveSpeed);
        f.speedAmp = sp == null ? -1 : Math.min(sp.getAmplifier(), 4);
        f.pingMs = pingMs;
        f.cps = pilot.cps;
        int bow = lay.real[Pilot.BOW];
        f.punch = bow < 0 ? 0 : EnchantmentHelper.getEnchantmentLevel(Enchantment.punch.effectId, me.inventory.mainInventory[bow]);
        f.leashR = s.leash;
        f.extreme = false;

        // Holograms: the named stands that stand still over a chest, lines grouped by chest.
        // A mob's own name (its health bar, a stand riding it or kept on it) is no
        // chest's: those move, ride something or sit on a living thing.
        List<Entity> all = new ArrayList<Entity>(w.loadedEntityList);
        Entity hitE = mc.objectMouseOver != null && mc.objectMouseOver.typeOfHit == MovingObjectPosition.MovingObjectType.ENTITY ? mc.objectMouseOver.entityHit : null;
        Map<Long, List<Entity>> cols = new HashMap<Long, List<Entity>>();
        Map<Long, BlockPos> colChest = new HashMap<Long, BlockPos>();
        for (Entity e : all) {
            if (!(e instanceof EntityArmorStand) || !e.hasCustomName() || e.ridingEntity != null || e.riddenByEntity != null) {
                continue;
            }
            double mx = e.posX - e.lastTickPosX, my = e.posY - e.lastTickPosY, mz = e.posZ - e.lastTickPosZ;
            if (mx * mx + my * my + mz * mz > 1e-4 || onLiving(all, e)) {
                continue;
            }
            BlockPos c = chestUnder(w, e);
            if (c == null) {
                continue;
            }
            int ki = mem.near(c.getX(), c.getY(), c.getZ(), 2.5);
            if (ki >= 0) {
                Memory.Known k = mem.chests.get(ki);
                c = new BlockPos(k.x, k.y, k.z);
            }
            long key = c.toLong();
            List<Entity> l = cols.get(key);
            if (l == null) {
                cols.put(key, l = new ArrayList<Entity>());
                colChest.put(key, c);
            }
            l.add(e);
        }
        Map<Integer, List<Entity>> holo = new HashMap<Integer, List<Entity>>();
        for (Map.Entry<Long, List<Entity>> en : cols.entrySet()) {
            BlockPos c = colChest.get(en.getKey());
            int ci = mem.near(c.getX(), c.getY(), c.getZ(), 2.5);
            if (ci < 0) {
                // A new chest only where the hologram says what a farm chest's says.
                StringBuilder all2 = new StringBuilder();
                List<String> lines = new ArrayList<String>();
                for (Entity e : en.getValue()) {
                    lines.add(e.getCustomNameTag());
                    all2.append(' ').append(HoloText.plain(e.getCustomNameTag()).toLowerCase(Locale.ROOT));
                }
                if (HoloText.parse(lines, s.claimText, s.activeColors) == null || !looksLikeChest(all2.toString(), s.claimText)) {
                    continue;
                }
                ci = mem.chest(c.getX(), c.getY(), c.getZ());
            }
            holo.put(ci, en.getValue());
        }
        for (Map.Entry<Integer, List<Entity>> en : holo.entrySet()) {
            List<Entity> l = en.getValue();
            // Top line first.
            java.util.Collections.sort(l, new java.util.Comparator<Entity>() {
                public int compare(Entity a, Entity b) {
                    return Double.compare(b.posY, a.posY);
                }
            });
            List<String> lines = new ArrayList<String>();
            for (Entity e : l) {
                lines.add(e.getCustomNameTag());
                if (e == hitE) {
                    // "Click here to claim" is clicked on the hologram itself.
                    f.pickChest = en.getKey();
                }
            }
            int[] st = HoloText.parse(lines, s.claimText, s.activeColors);
            if (st != null) {
                Frame.Holo h = new Frame.Holo();
                h.chest = en.getKey();
                h.state = st[0];
                h.secs = st[1];
                f.holos.add(h);
            }
        }

        // Mobs, projectiles, other players.
        Entity hit = mc.objectMouseOver != null && mc.objectMouseOver.typeOfHit == MovingObjectPosition.MovingObjectType.ENTITY ? mc.objectMouseOver.entityHit : null;
        if (hit instanceof EntitySkeleton && hit.ridingEntity instanceof EntityHorse) {
            hit = hit.ridingEntity;
        }
        for (Entity e : all) {
            if (e == me) {
                continue;
            }
            if (e instanceof EntityPlayer) {
                if (!e.isInvisible() && e.getDistanceToEntity(me) < 96.0f) {
                    f.rivals.add(new double[] {e.posX, e.posY, e.posZ});
                }
                continue;
            }
            if (e instanceof EntityArrow || e instanceof EntitySnowball || e instanceof EntityEnderPearl || e instanceof EntityPotion
                    || e instanceof net.minecraft.entity.projectile.EntityFireball) {
                double dx = e.posX - e.lastTickPosX, dy = e.posY - e.lastTickPosY, dz = e.posZ - e.lastTickPosZ;
                if (dx * dx + dy * dy + dz * dz < 1e-4) {
                    // Stuck in a block.
                    continue;
                }
                Frame.Proj p = new Frame.Proj();
                if (e instanceof EntityArrow) {
                    p.kind = Frame.ARROW;
                    p.owner = ((EntityArrow) e).shootingEntity == me ? -1 : 0;
                } else if (e instanceof EntitySnowball || e instanceof net.minecraft.entity.projectile.EntityFireball) {
                    // (A fireball counts as a snowball: something thrown at the player.)
                    p.kind = Frame.SNOWBALL;
                    p.owner = 0;
                } else {
                    p.kind = e instanceof EntityEnderPearl ? Frame.PEARL : Frame.POTION;
                    p.owner = -1;
                }
                p.x = e.posX;
                p.y = e.posY;
                p.z = e.posZ;
                boolean fire = e instanceof net.minecraft.entity.projectile.EntityFireball;
                p.mx = fire ? dx : e.motionX;
                p.my = fire ? dy : e.motionY;
                p.mz = fire ? dz : e.motionZ;
                f.projs.add(p);
                continue;
            }
            int k = kind(e, me, s);
            if (k < 0 || e.isDead) {
                continue;
            }
            if (passive(e) && !nearChest(mem, e, 24.0)) {
                // A farm animal out in the field, not a mob of a chest.
                continue;
            }
            EntityLivingBase body = (EntityLivingBase) e;
            EntityLivingBase life = body;
            if (k == Frame.HORSEMAN && e instanceof EntityHorse) {
                life = (EntityLivingBase) e.riddenByEntity;
            }
            if (life.getHealth() <= 0.0f && (life.deathTime > 0 || life.isDead)) {
                continue;
            }
            Frame.Mob m = new Frame.Mob();
            m.id = e.getEntityId();
            m.kind = k;
            m.x = e.posX;
            m.y = e.posY;
            m.z = e.posZ;
            // What it moved by in the last tick: a client's motion of others is not the server's.
            m.mx = e.posX - e.lastTickPosX;
            m.my = e.posY - e.lastTickPosY;
            m.mz = e.posZ - e.lastTickPosZ;
            // A server that hides health sends 0 (or 1): then it counts as unhurt.
            m.hp = life.getHealth() > 0.0f ? life.getHealth() : life.getMaxHealth();
            m.maxHp = life.getMaxHealth();
            m.hurtTime = Math.max(body.hurtTime, life.hurtTime);
            m.mounted = k == Frame.RIDER && e.ridingEntity == me;
            if (k == Frame.HORSEMAN && e instanceof EntityHorse) {
                m.hw = 0.7;
                m.h = 2.6;
            } else {
                m.hw = e.width / 2.0;
                m.h = e.height;
            }
            f.mobs.add(m);
            if (hit == e) {
                f.pickMob = m.id;
            }
        }
        if (mc.objectMouseOver != null && mc.objectMouseOver.typeOfHit == MovingObjectPosition.MovingObjectType.BLOCK) {
            BlockPos b = mc.objectMouseOver.getBlockPos();
            int ci = b == null ? -1 : mem.near(b.getX(), b.getY(), b.getZ(), 1.5);
            // The chest itself (or the other half of it), not the ground beside it: a click there claims nothing.
            Memory.Known k = ci >= 0 ? mem.chests.get(ci) : null;
            if (k != null && ((k.x == b.getX() && k.y == b.getY() && k.z == b.getZ()) || isChest(w, b))) {
                f.pickChest = ci;
            }
        }
        return f;
    }

    /** What only a farm chest's hologram says (Expeditions, and the same in Spanish). */
    private static final String[] CHEST_WORDS = {"wave", "oleada", "to claim", "reclamar", "rewards", "recompensa"};

    static boolean looksLikeChest(String plain, String claimText) {
        for (String w : CHEST_WORDS) {
            if (plain.contains(w)) {
                return true;
            }
        }
        for (String t : claimText.split(",")) {
            t = t.trim().toLowerCase(Locale.ROOT);
            if (t.length() > 0 && plain.contains(t)) {
                return true;
            }
        }
        return false;
    }

    /** A stand kept on a living thing (a mob's name over its head). */
    static boolean onLiving(List<Entity> all, Entity stand) {
        for (Entity o : all) {
            if (o == stand || !(o instanceof EntityLivingBase) || o instanceof EntityArmorStand || o instanceof EntityPlayer) {
                continue;
            }
            double dx = o.posX - stand.posX, dz = o.posZ - stand.posZ, dy = stand.posY - o.posY;
            if (dx * dx + dz * dz < 0.6 * 0.6 && dy > -1.0 && dy < o.height + 1.5) {
                return true;
            }
        }
        return false;
    }

    /**
     * For the chat when the agent does not get the place: the named stands
     * around and whether a chest is under each, and the living things near
     * that it does not take for wave mobs, by type.
     */
    static String diag(net.minecraft.client.Minecraft mc, Settings s) {
        World w = mc.theWorld;
        EntityPlayerSP me = mc.thePlayer;
        StringBuilder b = new StringBuilder();
        int n = 0;
        java.util.Set<String> seen = new java.util.HashSet<String>();
        for (Object o : new ArrayList<Object>(w.loadedEntityList)) {
            Entity e = (Entity) o;
            if (!(e instanceof EntityArmorStand) || !e.hasCustomName() || e.getDistanceToEntity(me) > 48) {
                continue;
            }
            String t = HoloText.plain(e.getCustomNameTag());
            if (!seen.add(t) || n >= 6) {
                continue;
            }
            n++;
            BlockPos c = chestUnder(w, e);
            b.append(String.format(Locale.ROOT, "\nHolograma (%d,%d,%d): \"%s\" %s", (int) Math.floor(e.posX), (int) Math.floor(e.posY), (int) Math.floor(e.posZ),
                t, c == null ? "- sin cofre debajo (bloque: " + w.getBlockState(new BlockPos(e.posX, e.posY - 2, e.posZ)).getBlock().getLocalizedName() + ")" : "- cofre debajo"));
        }
        if (n == 0) {
            b.append("\nNo hay hologramas con nombre a menos de 48 bloques.");
        }
        Map<String, Integer> other = new HashMap<String, Integer>();
        for (Object o : new ArrayList<Object>(w.loadedEntityList)) {
            Entity e = (Entity) o;
            if (e == me || e instanceof EntityArmorStand || e instanceof EntityPlayer || !(e instanceof EntityLivingBase) || e.getDistanceToEntity(me) > 24) {
                continue;
            }
            if (kind(e, me, s) < 0 && !(e.ridingEntity != null)) {
                String k = EntityList.getEntityString(e);
                k = k == null ? e.getClass().getSimpleName() : k;
                Integer c = other.get(k);
                other.put(k, c == null ? 1 : c + 1);
            }
        }
        if (!other.isEmpty()) {
            b.append("\nSeres cerca que no toma por mobs de oleada: ").append(other);
        }
        return b.toString();
    }

    /** Not a monster: animals, villagers, bats, squid. */
    static boolean passive(Entity e) {
        return e instanceof net.minecraft.entity.EntityLiving && !(e instanceof EntityMob || e instanceof EntitySlime || e instanceof EntityGolem)
            && !(e instanceof net.minecraft.entity.passive.EntityTameable && ((net.minecraft.entity.passive.EntityTameable) e).isTamed());
    }

    static boolean nearChest(Memory mem, Entity e, double r) {
        for (Memory.Known k : mem.chests) {
            double dx = k.cx() - e.posX, dz = k.cz() - e.posZ;
            if (dx * dx + dz * dz <= r * r) {
                return true;
            }
        }
        return false;
    }
}
