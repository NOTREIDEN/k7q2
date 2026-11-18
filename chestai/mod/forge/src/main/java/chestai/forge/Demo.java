package chestai.forge;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.gui.GuiMainMenu;
import net.minecraft.command.ICommandManager;
import net.minecraft.entity.Entity;
import net.minecraft.entity.EntityLiving;
import net.minecraft.entity.SharedMonsterAttributes;
import net.minecraft.entity.item.EntityArmorStand;
import net.minecraft.entity.monster.EntitySkeleton;
import net.minecraft.entity.monster.EntityZombie;
import net.minecraft.entity.passive.EntityHorse;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.init.Blocks;
import net.minecraft.server.MinecraftServer;
import net.minecraft.util.BlockPos;
import net.minecraft.util.ChatComponentText;
import net.minecraft.world.EnumDifficulty;
import net.minecraft.world.WorldServer;
import net.minecraft.world.WorldSettings;
import net.minecraft.world.WorldType;

/**
 * For a video, with -Dchestai.demo=true: a small chest farm that works like
 * the server's (5 mobs when the player comes within 12 blocks, 10 seconds
 * to kill them or the chest resets, a leash, the claim line and the
 * countdown on the hologram), the kit, and the agent at the keys for
 * chestai.demo.seconds. The farm runs on the integrated server's thread.
 */
final class Demo {
    private static final int COOLDOWN = 0, READY = 1, ACTIVE = 2, CLAIMABLE = 3;

    private final ChestAi mod;
    private final int seconds = Integer.getInteger("chestai.demo.seconds", 120);
    private int state, ticks;
    private final List<Chest> chests = new ArrayList<Chest>();
    private volatile boolean farmOn;
    private volatile String pendingName;
    private volatile double[] pendingAt;
    private int claims, resets;
    /** Like Expeditions: two-line holograms, tough armoured skeletons and wolves with names over them, NPC holograms about. */
    private static final boolean EXP = Boolean.getBoolean("chestai.demo.exp");

    private static EntityArmorStand stand(WorldServer w, double x, double y, double z, String name) {
        EntityArmorStand a = new EntityArmorStand(w, x, y, z);
        net.minecraft.nbt.NBTTagCompound tag = new net.minecraft.nbt.NBTTagCompound();
        a.writeEntityToNBT(tag);
        tag.setBoolean("Marker", true);
        tag.setBoolean("NoGravity", true);
        tag.setBoolean("Invisible", true);
        a.readEntityFromNBT(tag);
        a.setInvisible(true);
        a.setAlwaysRenderNameTag(true);
        a.setCustomNameTag(name);
        w.spawnEntityInWorld(a);
        return a;
    }

    private static final class Chest {
        BlockPos pos;
        EntityArmorStand holo, holo2;
        final java.util.Map<EntityLiving, EntityArmorStand> tags = new java.util.HashMap<EntityLiving, EntityArmorStand>();
        int state = READY, timer, waveTicks;
        long started;
        final List<EntityLiving> mobs = new ArrayList<EntityLiving>();
        String shown = "";
    }

    Demo(ChestAi mod) {
        this.mod = mod;
    }

    private static void run(MinecraftServer s, String cmd) {
        ICommandManager m = s.getCommandManager();
        m.executeCommand(s, cmd);
    }

    /** The client's side: the world, the kit, the keys, the end. */
    void step(Minecraft mc) throws Exception {
        if (state == 0) {
            if (!(mc.currentScreen instanceof GuiMainMenu)) {
                return;
            }
            mc.gameSettings.pauseOnLostFocus = false;
            mc.gameSettings.renderDistanceChunks = 5;
            mc.gameSettings.limitFramerate = 30;
            mc.gameSettings.fancyGraphics = false;
            mc.gameSettings.clouds = 0;
            WorldSettings ws = new WorldSettings(3L, WorldSettings.GameType.SURVIVAL, false, false, WorldType.FLAT);
            ws.enableCommands();
            ws.setWorldName("3;minecraft:bedrock,2*minecraft:dirt,minecraft:grass;1;");
            mc.launchIntegratedServer("demo" + System.currentTimeMillis(), "demo", ws);
            state = 1;
        } else if (state == 1) {
            EntityPlayerSP me = mc.thePlayer;
            if (me == null || mc.theWorld == null || me.ticksExisted < 100 || !me.onGround || mc.getIntegratedServer() == null) {
                return;
            }
            if (mc.currentScreen != null) {
                mc.displayGuiScreen(null);
                return;
            }
            pendingAt = new double[] {me.posX, me.posY, me.posZ};
            pendingName = me.getName();
            me.triggerAchievement(net.minecraft.stats.AchievementList.openInventory);
            state = 5;
        } else if (state == 5) {
            // The farm is built on the server's thread (see serverTick).
            if (!farmOn) {
                return;
            }
            state = 2;
        } else if (state == 2) {
            if (++ticks < 60) {
                return;
            }
            ticks = 0;
            mc.gameSettings.thirdPersonView = 1;
            mod.flip(mc);
            if (!mod.on()) {
                System.out.println("DEMO FAILED: the agent did not take the keys");
                mc.shutdown();
                return;
            }
            System.out.println("DEMO agent on");
            state = 3;
        } else if (state == 3) {
            ticks++;
            if (!mod.on() || ticks >= 20 * seconds) {
                if (mod.on()) {
                    mod.flip(mc);
                }
                System.out.println(String.format(Locale.ROOT, "DEMO done: %d ticks, %d claims, %d resets, agent %s",
                    ticks, claims, resets, mod.on() ? "on" : "off"));
                state = 4;
                mc.shutdown();
            }
        }
    }

    private static String clock(int ticks) {
        int s = (ticks + 19) / 20;
        return s >= 60 ? (s / 60) + " minutes " + (s % 60) + " seconds" : s + " seconds";
    }

    private void tell(MinecraftServer s, String line) {
        for (Object o : s.getConfigurationManager().playerEntityList) {
            ((EntityPlayer) o).addChatMessage(new ChatComponentText(line));
        }
    }

    /** Builds the farm and gives the kit, on the server's thread. */
    private void setup(MinecraftServer s, String n, double px, double py, double pz) {
        s.setDifficultyForAllWorlds(EnumDifficulty.EASY);
        run(s, "gamerule doMobSpawning false");
        run(s, "gamerule sendCommandFeedback false");
        // A flat world spawns slimes before the rule is in.
        for (int i = 0; i < 4; i++) {
            run(s, "kill @e[type=!Player]");
        }
        run(s, "gamerule logAdminCommands false");
        run(s, "gamerule doDaylightCycle false");
        run(s, "time set 6000");
        run(s, "give " + n + " diamond_sword 1 0 {ench:[{id:16,lvl:2}]}");
        run(s, "give " + n + " bow 1 0 {ench:[{id:49,lvl:2},{id:51,lvl:1}]}");
        run(s, "give " + n + " golden_apple 32");
        run(s, "give " + n + " ender_pearl 8");
        run(s, "give " + n + " potion 6 16421");
        run(s, "give " + n + " arrow 64");
        run(s, "replaceitem entity " + n + " slot.armor.head diamond_helmet 1 0 {ench:[{id:0,lvl:2}]}");
        run(s, "replaceitem entity " + n + " slot.armor.chest diamond_chestplate 1 0 {ench:[{id:0,lvl:2}]}");
        run(s, "replaceitem entity " + n + " slot.armor.legs diamond_leggings 1 0 {ench:[{id:0,lvl:2}]}");
        run(s, "replaceitem entity " + n + " slot.armor.feet diamond_boots 1 0 {ench:[{id:0,lvl:2},{id:2,lvl:4}]}");
        run(s, "effect " + n + " 1 1000000 2 true");
        WorldServer w = s.worldServerForDimension(0);
        int x0 = (int) Math.floor(px), y = (int) Math.floor(py), z0 = (int) Math.floor(pz);
        // Four chests on stepped hills 3 to 7 blocks high, 25 to 35 blocks apart: the
        // agent climbs, drops and can travel with Punch arrows between them.
        int[][] at = {{24, 4, 4}, {52, -10, 7}, {74, 16, 5}, {44, 38, 3}};
        for (int[] a : at) {
            int cx = x0 + a[0], cz = z0 + a[1], h = a[2];
            for (int k = 1; k <= h; k++) {
                int r = 2 * (h - k) + 3;
                run(s, "fill " + (cx - r) + " " + (y + k - 1) + " " + (cz - r) + " " + (cx + r) + " " + (y + k - 1) + " " + (cz + r) + " grass");
            }
            Chest c = new Chest();
            c.pos = new BlockPos(cx, y + h, cz);
            chests.add(c);
            run(s, "setblock " + cx + " " + (y + h) + " " + cz + " chest");
            run(s, "fill " + (cx - 3) + " " + (y + h) + " " + (cz + 3) + " " + (cx + 1) + " " + (y + h + 1) + " " + (cz + 3) + " stonebrick");
        }
        run(s, "fill " + (x0 + 36) + " " + y + " " + (z0 - 4) + " " + (x0 + 38) + " " + (y + 3) + " " + (z0 + 2) + " stone");
        for (Chest c : chests) {
            c.holo = new EntityArmorStand(w, c.pos.getX() + 0.5, c.pos.getY() + 0.6, c.pos.getZ() + 0.5);
            c.holo.setInvisible(true);
            if (!Boolean.getBoolean("chestai.demo.solidholo")) {
                net.minecraft.nbt.NBTTagCompound tag = new net.minecraft.nbt.NBTTagCompound();
                c.holo.writeEntityToNBT(tag);
                tag.setBoolean("Marker", true);
                tag.setBoolean("NoGravity", true);
                tag.setBoolean("Invisible", true);
                c.holo.readEntityFromNBT(tag);
            }
            c.holo.setAlwaysRenderNameTag(true);
            c.holo.setCustomNameTag("§eNext Wave: §f0 seconds");
            w.spawnEntityInWorld(c.holo);
        }
        chests.get(3).state = COOLDOWN;
        chests.get(3).timer = 20 * 25;
        if (EXP) {
            for (Chest c : chests) {
                c.holo.setPosition(c.pos.getX() + 0.5, c.pos.getY() + 1.0, c.pos.getZ() + 0.5);
                c.holo2 = stand(w, c.pos.getX() + 0.5, c.pos.getY() + 0.7, c.pos.getZ() + 0.5, "§eNext Wave: §f0 seconds");
            }
            // The first chest is about to come: the agent should go there and wait.
            chests.get(0).state = COOLDOWN;
            chests.get(0).timer = 20 * 15;
            // NPC holograms near where the player starts, over the ground.
            stand(w, x0 + 4.5, y + 2.3, z0 + 3.5, "§6§lEXPEDITIONS");
            stand(w, x0 + 4.5, y + 2.0, z0 + 3.5, "§eRight click to start!");
            stand(w, x0 - 3.5, y + 2.0, z0 - 4.5, "§c§lBOSS RUSH §7(soon)");
        }
        farmOn = true;
    }

    /** The server's side, every tick. */
    void serverTick(MinecraftServer s) {
        if (pendingName != null) {
            String n = pendingName;
            pendingName = null;
            setup(s, n, pendingAt[0], pendingAt[1], pendingAt[2]);
        }
        if (!farmOn) {
            return;
        }
        WorldServer w = s.worldServerForDimension(0);
        List<?> players = s.getConfigurationManager().playerEntityList;
        if (players.isEmpty()) {
            return;
        }
        EntityPlayer p = (EntityPlayer) players.get(0);
        // Slimes that a flat world spawned in chunks loaded later.
        for (Object o : new ArrayList<Object>(w.loadedEntityList)) {
            if (o instanceof net.minecraft.entity.monster.EntitySlime) {
                ((Entity) o).setDead();
            }
        }
        for (Chest c : chests) {
            double cx = c.pos.getX() + 0.5, cz = c.pos.getZ() + 0.5;
            String name;
            if (c.state == COOLDOWN) {
                if (--c.timer <= 0) {
                    c.state = READY;
                }
                name = "§eNext Wave: §f" + clock(Math.max(c.timer, 0));
            } else if (c.state == READY) {
                name = "§eNext Wave: §f0 seconds";
                if (p.getDistance(cx, c.pos.getY(), cz) <= 12.0) {
                    spawn(w, c);
                    c.state = ACTIVE;
                    c.waveTicks = EXP ? 20 * 60 : 200;
                    c.started = w.getTotalWorldTime();
                }
            } else if (c.state == ACTIVE) {
                for (int i = c.mobs.size() - 1; i >= 0; i--) {
                    EntityLiving m = c.mobs.get(i);
                    if (m.isDead || m.getHealth() <= 0) {
                        c.mobs.remove(i);
                    } else if (Math.hypot(m.posX - cx, m.posZ - cz) > 5.5) {
                        Entity top = m.ridingEntity != null ? m.ridingEntity : m;
                        top.setPositionAndUpdate(cx + 1.5, c.pos.getY(), cz + 1.5);
                    }
                }
                if (c.mobs.isEmpty()) {
                    c.state = CLAIMABLE;
                } else if (--c.waveTicks <= 0) {
                    for (EntityArmorStand t : c.tags.values()) {
                        t.setDead();
                    }
                    c.tags.clear();
                    for (EntityLiving m : c.mobs) {
                        if (m.ridingEntity != null) {
                            m.ridingEntity.setDead();
                        }
                        m.setDead();
                    }
                    c.mobs.clear();
                    c.state = COOLDOWN;
                    c.timer = 20 * 8;
                    resets++;
                    tell(s, "§7Se reseteó un cofre (no murieron a tiempo).");
                }
                name = "§cKill the mobs! §f" + clock(Math.max(c.waveTicks, 0));
            } else {
                name = "§aClick here to claim the rewards!";
            }
            String sub = null;
            if (EXP) {
                if (c.state == ACTIVE) {
                    name = "§c§lWAVE IN PROGRESS";
                    sub = "§7Kill the creatures to get the rewards!";
                } else if (c.state == CLAIMABLE) {
                    name = "§a§lWAVE DEFEATED";
                    sub = "§eClick here to claim the rewards!";
                } else {
                    sub = name;
                    name = "§a§lWAVE DEFEATED";
                }
                for (java.util.Iterator<java.util.Map.Entry<EntityLiving, EntityArmorStand>> it = c.tags.entrySet().iterator(); it.hasNext();) {
                    java.util.Map.Entry<EntityLiving, EntityArmorStand> en = it.next();
                    EntityLiving m = en.getKey();
                    if (m.isDead || m.getHealth() <= 0) {
                        en.getValue().setDead();
                        it.remove();
                    } else {
                        en.getValue().setPositionAndUpdate(m.posX, m.posY + m.height + 0.3, m.posZ);
                        en.getValue().setCustomNameTag("§c" + (int) Math.ceil(m.getHealth()) + " §4\u2764");
                    }
                }
            }
            if (!(name + sub).equals(c.shown)) {
                c.holo.setCustomNameTag(name);
                if (c.holo2 != null && sub != null) {
                    c.holo2.setCustomNameTag(sub);
                }
                c.shown = name + sub;
            }
        }
    }

    /** A click on a chest, on the server: the claim. Whether to cancel the click. */
    boolean click(EntityPlayer p, BlockPos pos) {
        for (Chest c : chests) {
            if (c.pos.equals(pos)) {
                if (c.state == CLAIMABLE) {
                    c.state = COOLDOWN;
                    c.timer = 20 * 40;
                    claims++;
                    double secs = (p.worldObj.getTotalWorldTime() - c.started) / 20.0;
                    p.addChatMessage(new ChatComponentText(String.format(Locale.ROOT,
                        "§aCofre reclamado §f(%d) §7%.1f s desde que salieron los mobs", claims, secs)));
                }
                return true;
            }
        }
        return false;
    }

    private void spawn(WorldServer w, Chest c) {
        if (EXP) {
            spawnExp(w, c);
            return;
        }
        double cx = c.pos.getX() + 0.5, cz = c.pos.getZ() + 0.5;
        int y = c.pos.getY();
        for (int i = 0; i < 5; i++) {
            double a = i * Math.PI * 2 / 5, x = cx + 2.5 * Math.cos(a), z = cz + 2.5 * Math.sin(a);
            EntityLiving m;
            if (i == 4) {
                EntitySkeleton sk = new EntitySkeleton(w);
                sk.setCurrentItemOrArmor(0, new net.minecraft.item.ItemStack(net.minecraft.init.Items.bow));
                sk.setCurrentItemOrArmor(4, new net.minecraft.item.ItemStack(net.minecraft.init.Items.iron_helmet));
                EntityHorse h = new EntityHorse(w);
                h.setHorseType(4);
                h.setHorseTamed(true);
                h.setLocationAndAngles(x, y, z, 0, 0);
                w.spawnEntityInWorld(h);
                sk.setLocationAndAngles(x, y, z, 0, 0);
                sk.getEntityAttribute(SharedMonsterAttributes.maxHealth).setBaseValue(30);
                sk.setHealth(30);
                w.spawnEntityInWorld(sk);
                sk.mountEntity(h);
                m = sk;
            } else {
                EntityZombie zo = new EntityZombie(w);
                zo.setLocationAndAngles(x, y, z, 0, 0);
                zo.setCurrentItemOrArmor(4, new net.minecraft.item.ItemStack(net.minecraft.init.Items.leather_helmet));
                zo.getEntityAttribute(SharedMonsterAttributes.maxHealth).setBaseValue(30);
                zo.getEntityAttribute(SharedMonsterAttributes.followRange).setBaseValue(24);
                zo.setHealth(30);
                w.spawnEntityInWorld(zo);
                m = zo;
            }
            c.mobs.add(m);
        }
    }

    private void spawnExp(WorldServer w, Chest c) {
        double cx = c.pos.getX() + 0.5, cz = c.pos.getZ() + 0.5;
        int y = c.pos.getY();
        for (int i = 0; i < 5; i++) {
            double a = i * Math.PI * 2 / 5, x = cx + 2.5 * Math.cos(a), z = cz + 2.5 * Math.sin(a);
            EntityLiving m;
            if (i < 3) {
                EntitySkeleton sk = new EntitySkeleton(w);
                sk.setCurrentItemOrArmor(0, new net.minecraft.item.ItemStack(net.minecraft.init.Items.stone_sword));
                sk.setCurrentItemOrArmor(4, new net.minecraft.item.ItemStack(net.minecraft.init.Items.skull, 1, 3));
                sk.setCurrentItemOrArmor(3, new net.minecraft.item.ItemStack(net.minecraft.init.Items.iron_chestplate));
                sk.setCurrentItemOrArmor(2, new net.minecraft.item.ItemStack(net.minecraft.init.Items.iron_leggings));
                sk.setCurrentItemOrArmor(1, new net.minecraft.item.ItemStack(net.minecraft.init.Items.iron_boots));
                m = sk;
            } else {
                net.minecraft.entity.passive.EntityWolf wo = new net.minecraft.entity.passive.EntityWolf(w);
                wo.setAngry(true);
                m = wo;
            }
            m.setLocationAndAngles(x, y, z, 0, 0);
            m.getEntityAttribute(SharedMonsterAttributes.maxHealth).setBaseValue(200);
            m.setHealth(50);
            w.spawnEntityInWorld(m);
            if (m instanceof net.minecraft.entity.passive.EntityWolf) {
                List<?> ps = w.playerEntities;
                if (!ps.isEmpty()) {
                    m.setAttackTarget((EntityPlayer) ps.get(0));
                }
            }
            c.mobs.add(m);
            c.tags.put(m, stand(w, x, y + m.height + 0.3, z, "§c50 §4\u2764"));
        }
    }
}
