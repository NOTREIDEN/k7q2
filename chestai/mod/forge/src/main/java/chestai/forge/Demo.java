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
import net.minecraft.block.material.Material;
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
    private volatile int claims, resets, firstClaim = -1;
    /**
     * With -Dchestai.arena=true: a natural world of the seed chestai.arena.seed
     * (hills, mountains, forests, rivers, villages) with the chests spread over
     * it, walls, houses and fences on the ways between them, and the play
     * measured into arena.txt: claims, deaths, time stuck, walking not facing
     * the way, potions and apples and when they were used.
     */
    private static final boolean ARENA = Boolean.getBoolean("chestai.arena");
    private static final long SEED = Long.getLong("chestai.arena.seed", 1L);
    /** Like Expeditions: two-line holograms, tough armoured skeletons and wolves with names over them, NPC holograms about. In the arena, every other seed. */
    private final boolean exp = Boolean.getBoolean("chestai.demo.exp") || (ARENA && (SEED & 1L) == 1L);

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
        /** Where its mobs are put back when they stray (a spot with ground by the chest). */
        double[] leash;
        long started, clearedAt;
        final List<EntityLiving> mobs = new ArrayList<EntityLiving>();
        /** Mobs of the wave with a bow that keep back and shoot, and where each stands. */
        final java.util.Map<EntityLiving, double[]> archers = new java.util.HashMap<EntityLiving, double[]>();
        /** Mobs of the wave that hurt all round them every second (an area blow, with its knockback). */
        final java.util.Set<EntityLiving> aoe = new java.util.HashSet<EntityLiving>();
        String shown = "";
    }

    /**
     * In the arena: skeletons with bows out in the field, no part of any wave, that
     * shoot at the player whenever it is in range and in sight (every second or so),
     * each kept to its spot; one killed comes back elsewhere a while later.
     */
    private final List<EntitySkeleton> snipers = new ArrayList<EntitySkeleton>();
    private final java.util.Map<EntitySkeleton, double[]> sniperAt = new java.util.HashMap<EntitySkeleton, double[]>();
    private int sniperBack = -1, sniperKills;
    /** A wave's mobs all dead: how long it took, the claim after it, and from a claim to the next wave. */
    private volatile int clears, clearTicks, claimWaits, claimWaitTicks, betweens, betweenTicks, projHits, aoeHits;
    private volatile long lastClaimAt = -1;
    private volatile float projDamage;
    /** Seeds with plain food in the kit, with one chest in a pool of water. */
    private static final boolean STEAK = (SEED % 4) < 2, POOL = SEED % 3 == 0;

    /** A mob's arrow or fireball hit the player (on the server). */
    void projHit(float amount) {
        projHits++;
        projDamage += amount;
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
            mc.gameSettings.renderDistanceChunks = ARENA ? 6 : 5;
            mc.gameSettings.limitFramerate = 30;
            mc.gameSettings.fancyGraphics = false;
            mc.gameSettings.clouds = 0;
            WorldSettings ws;
            if (ARENA) {
                ws = new WorldSettings(SEED, WorldSettings.GameType.SURVIVAL, true, false, WorldType.DEFAULT);
            } else {
                ws = new WorldSettings(3L, WorldSettings.GameType.SURVIVAL, false, false, WorldType.FLAT);
                ws.setWorldName("3;minecraft:bedrock,2*minecraft:dirt,minecraft:grass;1;");
            }
            ws.enableCommands();
            mc.launchIntegratedServer("demo" + System.currentTimeMillis(), "demo", ws);
            state = 1;
        } else if (state == 1) {
            EntityPlayerSP me = mc.thePlayer;
            if (me == null || mc.theWorld == null || me.ticksExisted < 100 || (!me.onGround && me.ticksExisted < 400) || mc.getIntegratedServer() == null) {
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
            // No "press E" hint over the picture.
            mc.guiAchievement.clearAchievements();
            if (!ARENA || Boolean.getBoolean("chestai.arena.video")) {
                // Seen from behind, for a video.
                mc.gameSettings.thirdPersonView = 1;
            }
            mod.flip(mc);
            if (!mod.on()) {
                System.out.println("DEMO FAILED: the agent did not take the keys");
                mc.shutdown();
                return;
            }
            System.out.println("DEMO agent on");
            state = 3;
        } else if (state == 3 && ARENA) {
            ticks++;
            EntityPlayerSP me = mc.thePlayer;
            if (me == null) {
                return;
            }
            if (me.isDead || me.getHealth() <= 0.0F) {
                deaths++;
                System.out.println(String.format(Locale.ROOT, "ARENA death %d at tick %d", deaths, ticks));
                me.respawnPlayer();
                mc.displayGuiScreen(null);
                waitOn = 0;
                state = 6;
                return;
            }
            if (!mod.on()) {
                // A screen or something else took the keys: back to the agent.
                if (mc.currentScreen != null) {
                    mc.displayGuiScreen(null);
                }
                if (++waitOn % 40 == 1) {
                    offs++;
                    mod.flip(mc);
                }
            } else {
                waitOn = 0;
            }
            measure(mc, me);
            if (ticks >= 20 * seconds) {
                finish(mc);
            }
        } else if (state == 6) {
            // Dead: back at the spawn with the kit, then the agent again.
            ticks++;
            EntityPlayerSP me = mc.thePlayer;
            if (mc.currentScreen != null) {
                mc.displayGuiScreen(null);
            }
            if (me != null && !me.isDead && me.getHealth() > 0.0F && ++waitOn > 40) {
                lastPots = -1;
                lastApples = -1;
                lastArrows = -1;
                mod.flip(mc);
                waitOn = 0;
                state = 3;
            }
            if (ticks >= 20 * seconds) {
                finish(mc);
            }
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

    private int deaths, offs, waitOn, stuck, moving, side, arrows, pots, potsHigh, apples, applesFull;
    private int lastPots = -1, lastApples = -1, lastArrows = -1, lastSteak = -1, steak, foodLow, minFood = 20;
    private float lastHp = 20.0F, eatHp = 20.0F, minHp = 99.0F;
    private boolean wasEating, wasDrawing, shotHit;
    private final java.util.Map<String, Integer> modeTicks = new java.util.TreeMap<String, Integer>(), stillTicks = new java.util.TreeMap<String, Integer>();
    private int shotAt = -1, boostHits;
    private double shotTop;

    /** The blocks by the player's feet and head, and ahead of them, for the log. */
    private static String around(Minecraft mc, EntityPlayerSP me) {
        StringBuilder b = new StringBuilder();
        double yr = Math.toRadians(me.rotationYaw);
        int x = (int) Math.floor(me.posX), y = (int) Math.floor(me.posY + 0.01), z = (int) Math.floor(me.posZ);
        int ax = (int) Math.floor(me.posX - Math.sin(yr)), az = (int) Math.floor(me.posZ + Math.cos(yr));
        int[][] at = {{x, y - 1, z}, {x, y, z}, {x, y + 1, z}, {ax, y, az}, {ax, y + 1, az}, {ax, y + 2, az}};
        String[] what = {"under", "feet", "head", "ahead", "aheadUp", "aheadUp2"};
        for (int i = 0; i < at.length; i++) {
            net.minecraft.block.Block bl = mc.theWorld.getBlockState(new BlockPos(at[i][0], at[i][1], at[i][2])).getBlock();
            b.append(what[i]).append('=').append(net.minecraft.block.Block.blockRegistry.getNameForObject(bl).toString().replace("minecraft:", "")).append(' ');
        }
        return b.toString();
    }
    private final double[] ringX = new double[40], ringZ = new double[40];

    private static int count(EntityPlayerSP me, String item) {
        int n = 0;
        for (net.minecraft.item.ItemStack st : me.inventory.mainInventory) {
            if (st != null && net.minecraft.item.Item.itemRegistry.getNameForObject(st.getItem()).toString().equals("minecraft:" + item)) {
                n += st.stackSize;
            }
        }
        return n;
    }

    private static boolean mobNear(Minecraft mc, EntityPlayerSP me, double r) {
        for (Object o : mc.theWorld.loadedEntityList) {
            if (o instanceof EntityLiving && ((Entity) o).getDistanceToEntity(me) < r) {
                return true;
            }
        }
        return false;
    }

    /** One tick of the arena's measures, on the client. */
    private void measure(Minecraft mc, EntityPlayerSP me) {
        float hp = me.getHealth();
        minHp = Math.min(minHp, hp);
        double dx = me.posX - me.prevPosX, dz = me.posZ - me.prevPosZ;
        if (Math.sqrt(dx * dx + dz * dz) > 0.12) {
            moving++;
            double err = ((Math.toDegrees(Math.atan2(-dx, dz)) - me.rotationYaw) % 360.0 + 540.0) % 360.0 - 180.0;
            if (Math.abs(err) > 60.0) {
                side++;
            }
        }
        int i = ticks % ringX.length;
        double ox = ringX[i], oz = ringZ[i];
        ringX[i] = me.posX;
        ringZ[i] = me.posZ;
        if (ticks > ringX.length && mod.on() && mc.gameSettings.keyBindForward.isKeyDown()
                && Math.hypot(me.posX - ox, me.posZ - oz) < 1.5 && !mobNear(mc, me, 5.0)) {
            if (stuck++ % 40 == 0) {
                System.out.println(String.format(Locale.ROOT, "ARENA stuck t=%d pos=%.2f,%.2f,%.2f yaw=%.0f ground=%b water=%b around %s",
                    ticks, me.posX, me.posY, me.posZ, me.rotationYaw, me.onGround, me.isInWater(), around(mc, me)));
            }
        }
        // Time in each thing the coach does, and of it standing still.
        String what = mod.session() == null ? "-" : mod.session().coachLine();
        String m0 = what.split(" ")[0].split("\\+")[0];
        boolean still = Math.sqrt(dx * dx + dz * dz) < 0.03;
        Integer n0 = modeTicks.get(m0);
        modeTicks.put(m0, n0 == null ? 1 : n0 + 1);
        if (still) {
            Integer n1 = stillTicks.get(m0);
            stillTicks.put(m0, n1 == null ? 1 : n1 + 1);
        }
        if (ticks % 20 == 0) {
            System.out.println(String.format(Locale.ROOT, "ARENA t=%d pos=%.1f,%.1f,%.1f yaw=%.0f pitch=%.0f hp=%.1f fwd=%b sprint=%b on=%b %s",
                ticks, me.posX, me.posY, me.posZ, me.rotationYaw, me.rotationPitch, me.getHealth(), mc.gameSettings.keyBindForward.isKeyDown(),
                me.isSprinting(), mod.on(), mod.session() == null ? "-" : mod.session().coachLine()));
        }
        if (shotAt >= 0) {
            shotTop = Math.max(shotTop, Math.sqrt(dx * dx + dz * dz));
            shotHit |= me.hurtTime > 0;
            if (ticks - shotAt >= 40) {
                System.out.println(String.format(Locale.ROOT, "ARENA shot t=%d hit=%b fastest=%.2f", shotAt, shotHit, shotTop));
                boostHits += shotHit && shotTop > 0.6 ? 1 : 0;
                shotAt = -1;
            }
        }
        boolean eating = me.isUsingItem() && me.getItemInUse() != null && me.getItemInUse().getItem() == net.minecraft.init.Items.golden_apple;
        if (eating && !wasEating) {
            eatHp = hp;
        }
        wasEating = eating;
        int p = count(me, "potion"), a = count(me, "golden_apple"), ar = count(me, "arrow"), st = count(me, "cooked_beef");
        int food = me.getFoodStats().getFoodLevel();
        minFood = Math.min(minFood, food);
        if (food < 20) {
            foodLow++;
        }
        if (lastSteak >= 0 && st < lastSteak) {
            steak += lastSteak - st;
        }
        lastSteak = st;
        if (lastPots >= 0 && p < lastPots) {
            pots += lastPots - p;
            if (lastHp >= 16.0F) {
                potsHigh += lastPots - p;
            }
        }
        if (lastApples >= 0 && a < lastApples) {
            apples += lastApples - a;
            if (eatHp >= 19.0F) {
                applesFull += lastApples - a;
            }
        }
        // The kit's bow has Infinity: a shot is a draw let go.
        boolean drawing = me.isUsingItem() && me.getItemInUse() != null && me.getItemInUse().getItem() == net.minecraft.init.Items.bow;
        if (wasDrawing && !drawing) {
            arrows++;
            shotAt = ticks;
            shotTop = 0.0;
            shotHit = false;
        }
        wasDrawing = drawing;
        lastPots = p;
        lastApples = a;
        lastArrows = ar;
        lastHp = hp;
        if (claims > 0 && firstClaim < 0) {
            firstClaim = ticks;
        }
    }

    private void finish(Minecraft mc) throws java.io.IOException {
        int known = mod.session() != null ? mod.session().chestsKnown() : -1;
        if (mod.on()) {
            mod.flip(mc);
        }
        String hits = mod.session() != null ? mod.session().hitLine() : "-";
        String line = String.format(Locale.ROOT,
            "seed=%d variant=%s exp=%b secs=%d claims=%d resets=%d deaths=%d offs=%d first=%.1f stuck=%.1f moving=%d side=%.3f arrows=%d boosts=%d pots=%d potsHigh=%d apples=%d applesFull=%d minHp=%.1f known=%d chests=%d obstacles=%d"
                + " clears=%d clearS=%.1f claimWaitS=%.1f betweenS=%.1f steak=%d foodLowS=%.0f minFood=%d projHits=%d projDmg=%.0f aoeHits=%d sniperKills=%d pool=%b hits=%s",
            SEED, System.getProperty("chestai.arena.variant", "base"), exp, ticks / 20, claims, resets, deaths, offs, firstClaim < 0 ? -1.0 : firstClaim / 20.0, stuck / 20.0, moving,
            moving == 0 ? 0.0 : (double) side / moving, arrows, boostHits, pots, potsHigh, apples, applesFull, minHp, known, chests.size(), obstacles,
            clears, clears == 0 ? -1.0 : clearTicks / 20.0 / clears, claimWaits == 0 ? -1.0 : claimWaitTicks / 20.0 / claimWaits,
            betweens == 0 ? -1.0 : betweenTicks / 20.0 / betweens, steak, foodLow / 20.0, minFood, projHits, projDamage, aoeHits, sniperKills, POOL, hits);
        System.out.println("ARENA " + line);
        StringBuilder modes = new StringBuilder();
        for (java.util.Map.Entry<String, Integer> e : modeTicks.entrySet()) {
            Integer st = stillTicks.get(e.getKey());
            modes.append(String.format(Locale.ROOT, " %s=%.0f/%.0f", e.getKey(), e.getValue() / 20.0, (st == null ? 0 : st) / 20.0));
        }
        System.out.println("ARENA modes (seconds / of them still)" + modes);
        line += " modes:" + modes.toString().trim().replace(' ', ',');
        java.io.OutputStream out = new java.io.FileOutputStream(new java.io.File(mc.mcDataDir, "arena.txt"));
        try {
            out.write((line + "\n").getBytes("UTF-8"));
        } finally {
            out.close();
        }
        state = 4;
        mc.shutdown();
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
        if (ARENA && STEAK) {
            run(s, "give " + n + " cooked_beef 32");
        }
        run(s, "give " + n + " potion 6 16421");
        run(s, "give " + n + " arrow 64");
        run(s, "replaceitem entity " + n + " slot.armor.head diamond_helmet 1 0 {ench:[{id:0,lvl:2}]}");
        run(s, "replaceitem entity " + n + " slot.armor.chest diamond_chestplate 1 0 {ench:[{id:0,lvl:2}]}");
        run(s, "replaceitem entity " + n + " slot.armor.legs diamond_leggings 1 0 {ench:[{id:0,lvl:2}]}");
        run(s, "replaceitem entity " + n + " slot.armor.feet diamond_boots 1 0 {ench:[{id:0,lvl:2},{id:2,lvl:4},{id:8,lvl:3}]}");
        run(s, "effect " + n + " 1 1000000 2 true");
        WorldServer w = s.worldServerForDimension(0);
        int x0 = (int) Math.floor(px), y = (int) Math.floor(py), z0 = (int) Math.floor(pz);
        if (ARENA) {
            run(s, "gamerule keepInventory true");
            run(s, "achievement give achievement.openInventory " + n);
            s.setDifficultyForAllWorlds(EnumDifficulty.NORMAL);
            buildArena(w, x0, y, z0);
            farmOn = true;
            return;
        }
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
        if (exp) {
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

    private int obstacles;
    /** Where the obstacles stand (x, z). */
    private final List<int[]> blocks = new ArrayList<int[]>();

    private static boolean groundLike(Material m) {
        return m == Material.grass || m == Material.ground || m == Material.sand || m == Material.rock || m == Material.clay
            || m == Material.craftedSnow || m == Material.ice || m == Material.packedIce;
    }

    /** The block over the ground at (x, z), or null where that is water, a tree or something built. */
    private static BlockPos ground(WorldServer w, int x, int z) {
        w.theChunkProviderServer.loadChunk(x >> 4, z >> 4);
        for (int y = 200; y > 1; y--) {
            BlockPos p = new BlockPos(x, y, z);
            Material m = w.getBlockState(p).getBlock().getMaterial();
            if (m == Material.air || m == Material.leaves || m == Material.plants || m == Material.vine || m == Material.snow) {
                continue;
            }
            return groundLike(m) ? p.up() : null;
        }
        return null;
    }

    /** The first block over whatever is solid at (x, z), water and trees counting as solid. */
    private static int top(WorldServer w, int x, int z) {
        w.theChunkProviderServer.loadChunk(x >> 4, z >> 4);
        for (int y = 200; y > 1; y--) {
            Material m = w.getBlockState(new BlockPos(x, y, z)).getBlock().getMaterial();
            if (m != Material.air && m != Material.plants && m != Material.vine && m != Material.snow) {
                return y + 1;
            }
        }
        return 2;
    }

    private static void fill(WorldServer w, int x, int z, int y0, int y1, net.minecraft.block.state.IBlockState st) {
        for (int y = y0; y <= y1; y++) {
            w.setBlockState(new BlockPos(x, y, z), st, 2);
        }
    }

    private Chest chestAt(WorldServer w, BlockPos at, boolean holo) {
        for (int up = 0; up < 3; up++) {
            w.setBlockState(at.up(up), Blocks.air.getDefaultState(), 2);
        }
        w.setBlockState(at, Blocks.chest.getDefaultState(), 2);
        Chest c = new Chest();
        c.pos = at;
        if (!holo) {
            return c;
        }
        c.holo = stand(w, at.getX() + 0.5, at.getY() + (exp ? 1.0 : 0.6), at.getZ() + 0.5, "§eNext Wave: §f0 seconds");
        if (exp) {
            c.holo2 = stand(w, at.getX() + 0.5, at.getY() + 0.7, at.getZ() + 0.5, "§eNext Wave: §f0 seconds");
        }
        return c;
    }

    /** Whether column (x, z) is within two blocks of a chest (an obstacle there would bury or wall it in). */
    private boolean byChest(int x, int z) {
        for (Chest c : chests) {
            if (Math.abs(c.pos.getX() - x) <= 2 && Math.abs(c.pos.getZ() - z) <= 2) {
                return true;
            }
        }
        return false;
    }

    /** How far the nearest chest is from (x, z), across. */
    private double chestGap(double x, double z) {
        double d = 1e9;
        for (Chest c : chests) {
            d = Math.min(d, Math.hypot(c.pos.getX() + 0.5 - x, c.pos.getZ() + 0.5 - z));
        }
        return d;
    }

    /**
     * A wall, a house or a fence across the way at (ox, oz), square to (ux, uz);
     * with build false only the random numbers it would take (the rest of the
     * arena stays as it was), none of it near a chest.
     */
    private void obstacle(WorldServer w, java.util.Random r, int ox, int oz, double ux, double uz, boolean build) {
        int kind = r.nextInt(3);
        if (kind == 1) {
            // A house of planks, 7 by 7 with a roof and one doorway.
            int base = top(w, ox, oz);
            int door = r.nextInt(4);
            if (!build) {
                return;
            }
            for (int dx = -3; dx <= 3; dx++) {
                for (int dz = -3; dz <= 3; dz++) {
                    int x = ox + dx, z = oz + dz;
                    if (byChest(x, z)) {
                        continue;
                    }
                    boolean edge = Math.abs(dx) == 3 || Math.abs(dz) == 3;
                    if (edge) {
                        boolean gap = (door == 0 && dx == 3 && dz == 0) || (door == 1 && dx == -3 && dz == 0)
                            || (door == 2 && dz == 3 && dx == 0) || (door == 3 && dz == -3 && dx == 0);
                        fill(w, x, z, Math.min(top(w, x, z), base), base + 3, Blocks.planks.getDefaultState());
                        if (gap) {
                            fill(w, x, z, base, base + 1, Blocks.air.getDefaultState());
                        }
                    } else {
                        fill(w, x, z, base, base + 3, Blocks.air.getDefaultState());
                    }
                    w.setBlockState(new BlockPos(x, base + 4, z), Blocks.planks.getDefaultState(), 2);
                }
            }
        } else {
            // A wall of stone 3 high, or a fence, 11 long, square to the way.
            if (!build) {
                return;
            }
            double px = -uz, pz = ux;
            for (int i = -5; i <= 5; i++) {
                int x = (int) Math.floor(ox + px * i), z = (int) Math.floor(oz + pz * i);
                if (byChest(x, z)) {
                    continue;
                }
                int b = top(w, x, z);
                if (kind == 0) {
                    fill(w, x, z, b, b + 2, Blocks.cobblestone.getDefaultState());
                } else {
                    w.setBlockState(new BlockPos(x, b, z), Blocks.oak_fence.getDefaultState(), 2);
                }
            }
        }
        obstacles++;
    }

    /** Water three deep round the chest (six blocks out), the chest on a pillar in the middle: waves fought swimming. */
    private void pool(WorldServer w, BlockPos c) {
        int g = c.getY();
        for (int dx = -6; dx <= 6; dx++) {
            for (int dz = -6; dz <= 6; dz++) {
                if (dx * dx + dz * dz > 36) {
                    continue;
                }
                int x = c.getX() + dx, z = c.getZ() + dz;
                if (dx == 0 && dz == 0) {
                    fill(w, x, z, g - 4, g - 1, Blocks.dirt.getDefaultState());
                    continue;
                }
                fill(w, x, z, g - 4, g - 4, Blocks.dirt.getDefaultState());
                fill(w, x, z, g - 3, g - 1, Blocks.water.getDefaultState());
                fill(w, x, z, g, g + 4, Blocks.air.getDefaultState());
            }
        }
        System.out.println("ARENA pool round chest 2 at " + c.getX() + " " + g + " " + c.getZ());
    }

    /** A skeleton with a bow out in the field, 9 to 15 blocks from a chest, that shoots at the player. */
    private void sniper(WorldServer w, java.util.Random r) {
        if (chests.isEmpty()) {
            return;
        }
        for (int tries = 0; tries < 40; tries++) {
            Chest c = chests.get(r.nextInt(chests.size()));
            double a = r.nextDouble() * Math.PI * 2.0, d = 9.0 + r.nextDouble() * 6.0;
            BlockPos at = ground(w, (int) Math.floor(c.pos.getX() + 0.5 + Math.cos(a) * d), (int) Math.floor(c.pos.getZ() + 0.5 + Math.sin(a) * d));
            if (at == null || Math.abs(at.getY() - c.pos.getY()) > 6) {
                continue;
            }
            EntitySkeleton sk = new EntitySkeleton(w);
            sk.setLocationAndAngles(at.getX() + 0.5, at.getY(), at.getZ() + 0.5, 0, 0);
            sk.setCurrentItemOrArmor(0, new net.minecraft.item.ItemStack(net.minecraft.init.Items.bow));
            // A head (it does not wear out) keeps it from burning in the sun.
            sk.setCurrentItemOrArmor(4, new net.minecraft.item.ItemStack(net.minecraft.init.Items.skull, 1, 0));
            sk.setCombatTask();
            w.spawnEntityInWorld(sk);
            snipers.add(sk);
            sniperAt.put(sk, new double[] {at.getX() + 0.5, at.getY(), at.getZ() + 0.5});
            System.out.println(String.format(Locale.ROOT, "ARENA sniper at %d %d %d", at.getX(), at.getY(), at.getZ()));
            return;
        }
    }

    /** One more arrow from a bow mob at the player, when it is in range and sight: they never stop shooting. */
    private static void shoot(EntitySkeleton sk, EntityPlayer p, double range) {
        if (!sk.isDead && sk.getHealth() > 0 && sk.getDistanceToEntity(p) < range && sk.canEntityBeSeen(p)) {
            sk.attackEntityWithRangedAttack(p, 1.0F);
        }
    }

    /** The chests of the arena over the natural ground about where the player starts, and things in the way. */
    private void buildArena(WorldServer w, int x0, int y0, int z0) {
        java.util.Random r = new java.util.Random(SEED * 7919L + 17L);
        for (int tries = 0; tries < 600 && chests.size() < 6; tries++) {
            double a = r.nextDouble() * Math.PI * 2.0, d = 16.0 + r.nextDouble() * 60.0;
            int x = x0 + (int) Math.round(Math.cos(a) * d), z = z0 + (int) Math.round(Math.sin(a) * d);
            boolean near = false;
            for (Chest c : chests) {
                near |= Math.hypot(c.pos.getX() - x, c.pos.getZ() - z) < 9.0;
            }
            BlockPos at = near ? null : ground(w, x, z);
            if (at == null || Math.abs(at.getY() - y0) > 30) {
                continue;
            }
            Chest c = chestAt(w, at, true);
            if (r.nextInt(3) == 0) {
                c.state = READY;
            } else {
                c.state = COOLDOWN;
                c.timer = 20 * (5 + r.nextInt(50));
            }
            chests.add(c);
            System.out.println(String.format(Locale.ROOT, "ARENA chest %d at %d %d %d (%.0f blocks), %s", chests.size(), at.getX(), at.getY(), at.getZ(), d,
                c.state == READY ? "ready" : (c.timer / 20) + " s"));
        }
        if (!chests.isEmpty()) {
            chests.get(0).state = READY;
        }
        if (POOL && chests.size() > 1) {
            pool(w, chests.get(1).pos);
        }
        for (Chest c : chests) {
            double dx = c.pos.getX() - x0, dz = c.pos.getZ() - z0, len = Math.hypot(dx, dz);
            if (len < 14.0 || r.nextInt(5) < 2) {
                continue;
            }
            double f = 0.4 + r.nextDouble() * 0.3;
            // Not on top of another chest: a little nearer or farther along the way, or none.
            double use = -1.0;
            for (double g : new double[] {f, f - 0.1, f + 0.1, f - 0.2, f + 0.2}) {
                if (g >= 0.25 && g <= 0.85 && chestGap(x0 + dx * g, z0 + dz * g) >= 8.0) {
                    use = g;
                    break;
                }
            }
            double g = use < 0.0 ? f : use;
            obstacle(w, r, (int) Math.floor(x0 + dx * g), (int) Math.floor(z0 + dz * g), dx / len, dz / len, use >= 0.0);
            if (use >= 0.0) {
                blocks.add(new int[] {(int) Math.floor(x0 + dx * g), (int) Math.floor(z0 + dz * g)});
            } else {
                System.out.println("ARENA obstacle left out: no room away from the chests");
            }
        }
        // A chest with no hologram: no farm chest, to be looked at once and then left.
        for (int tries = 0; tries < 60; tries++) {
            double a = r.nextDouble() * Math.PI * 2.0;
            BlockPos at = ground(w, x0 + (int) Math.round(Math.cos(a) * 20.0), z0 + (int) Math.round(Math.sin(a) * 20.0));
            boolean clear = true;
            for (int[] b : blocks) {
                clear &= at == null || Math.hypot(b[0] - at.getX(), b[1] - at.getZ()) >= 7.0;
            }
            if (at != null && clear) {
                chestAt(w, at, false);
                System.out.println("ARENA plain chest at " + at.getX() + " " + at.getY() + " " + at.getZ());
                break;
            }
        }
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
        // Slimes that a flat world spawned in chunks loaded later; in the arena, any animal of the world.
        if (!ARENA || w.getTotalWorldTime() % 20 == 0) {
            java.util.Set<Entity> keep = new java.util.HashSet<Entity>();
            for (Chest c : chests) {
                for (EntityLiving m : c.mobs) {
                    keep.add(m);
                    if (m.ridingEntity != null) {
                        keep.add(m.ridingEntity);
                    }
                }
            }
            keep.addAll(snipers);
            for (Object o : new ArrayList<Object>(w.loadedEntityList)) {
                if (o instanceof net.minecraft.entity.monster.EntitySlime || (ARENA && o instanceof EntityLiving && !keep.contains(o))) {
                    ((Entity) o).setDead();
                }
            }
        }
        if (ARENA) {
            long now = w.getTotalWorldTime();
            if (snipers.isEmpty() && sniperBack < 0 && sniperKills == 0) {
                java.util.Random r = new java.util.Random(SEED * 31L + 5L);
                sniper(w, r);
                sniper(w, r);
            }
            for (int i = snipers.size() - 1; i >= 0; i--) {
                EntitySkeleton sk = snipers.get(i);
                if (sk.isDead || sk.getHealth() <= 0) {
                    snipers.remove(i);
                    sniperAt.remove(sk);
                    sniperKills++;
                    sniperBack = 600;
                    continue;
                }
                double[] h = sniperAt.get(sk);
                if (h != null && Math.hypot(sk.posX - h[0], sk.posZ - h[2]) > 4.0) {
                    sk.setPositionAndUpdate(h[0], h[1], h[2]);
                }
                if ((now + i * 7) % 22 == 0) {
                    shoot(sk, p, 22.0);
                }
            }
            if (sniperBack >= 0 && --sniperBack < 0) {
                sniper(w, new java.util.Random(now));
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
                    if (ARENA) {
                        System.out.println(String.format(Locale.ROOT, "ARENA wave at chest %d, player %.1f away", chests.indexOf(c) + 1,
                            p.getDistance(cx, c.pos.getY(), cz)));
                    }
                    c.state = ACTIVE;
                    c.waveTicks = exp ? 20 * 60 : 200;
                    c.started = w.getTotalWorldTime();
                    if (lastClaimAt >= 0) {
                        betweens++;
                        betweenTicks += (int) (c.started - lastClaimAt);
                        lastClaimAt = -1;
                    }
                }
            } else if (c.state == ACTIVE) {
                for (int i = c.mobs.size() - 1; i >= 0; i--) {
                    EntityLiving m = c.mobs.get(i);
                    double[] home = c.archers.get(m);
                    if (m.isDead || m.getHealth() <= 0) {
                        c.mobs.remove(i);
                        c.archers.remove(m);
                        c.aoe.remove(m);
                        continue;
                    }
                    if (c.aoe.contains(m) && (w.getTotalWorldTime() + i * 3) % 20 == 0 && m.getDistanceToEntity(p) < 5.0F) {
                        p.attackEntityFrom(net.minecraft.util.DamageSource.causeMobDamage(m), 2.0F);
                        aoeHits++;
                    }
                    if (home != null) {
                        if (Math.hypot(m.posX - home[0], m.posZ - home[2]) > 4.0) {
                            m.setPositionAndUpdate(home[0], home[1], home[2]);
                        }
                        if ((w.getTotalWorldTime() + i * 5) % 20 == 0) {
                            shoot((EntitySkeleton) m, p, 24.0);
                        }
                    } else if (Math.hypot(m.posX - cx, m.posZ - cz) > 5.5) {
                        Entity top = m.ridingEntity != null ? m.ridingEntity : m;
                        if (c.leash == null) {
                            c.leash = place(w, c, cx + 1.5, cz + 1.5);
                        }
                        top.setPositionAndUpdate(c.leash[0], c.leash[1], c.leash[2]);
                    }
                }
                if (c.mobs.isEmpty()) {
                    c.state = CLAIMABLE;
                    c.clearedAt = w.getTotalWorldTime();
                    clears++;
                    clearTicks += (int) (c.clearedAt - c.started);
                    if (ARENA) {
                        System.out.println(String.format(Locale.ROOT, "ARENA cleared chest %d in %.1f s", chests.indexOf(c) + 1, (c.clearedAt - c.started) / 20.0));
                    }
                } else if (--c.waveTicks <= 0) {
                    if (ARENA) {
                        StringBuilder left = new StringBuilder();
                        for (EntityLiving m : c.mobs) {
                            left.append(String.format(Locale.ROOT, " %s:%.0fhp@%.1f", m.getClass().getSimpleName().replace("Entity", ""), m.getHealth(), m.getDistanceToEntity(p)));
                        }
                        System.out.println("ARENA reset of chest " + (chests.indexOf(c) + 1) + ", left" + left);
                    }
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
                    c.archers.clear();
                    c.aoe.clear();
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
            if (exp) {
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
                    long now = p.worldObj.getTotalWorldTime();
                    claimWaits++;
                    claimWaitTicks += (int) (now - c.clearedAt);
                    lastClaimAt = now;
                    c.state = COOLDOWN;
                    c.timer = 20 * (ARENA ? 30 + (int) (p.worldObj.rand.nextInt(31)) : 40);
                    claims++;
                    if (ARENA) {
                        System.out.println("ARENA claim of chest " + (chests.indexOf(c) + 1));
                    }
                    double secs = (p.worldObj.getTotalWorldTime() - c.started) / 20.0;
                    p.addChatMessage(new ChatComponentText(String.format(Locale.ROOT,
                        "§aCofre reclamado §f(%d) §7%.1f s desde que salieron los mobs", claims, secs)));
                }
                return true;
            }
        }
        return false;
    }

    /** Where a mob of the wave stands: on the ground by the chest, not in it. */
    /**
     * Where a mob of chest c can stand near (x, z): the nearest column within four blocks with ground
     * (or water) at most two blocks above or below the chest, not the chest's own; not up a tree, on
     * a roof or inside a hill, where it could be neither reached nor hit. As before if there is none.
     */
    private static double[] place(WorldServer w, Chest c, double x, double z) {
        int cy = c.pos.getY(), x0 = (int) Math.floor(x), z0 = (int) Math.floor(z);
        if (!ARENA) {
            return new double[] {x, cy, z};
        }
        for (int r = 0; r <= 4; r++) {
            for (int dx = -r; dx <= r; dx++) {
                for (int dz = -r; dz <= r; dz++) {
                    int bx = x0 + dx, bz = z0 + dz;
                    if (Math.max(Math.abs(dx), Math.abs(dz)) != r || (bx == c.pos.getX() && bz == c.pos.getZ())) {
                        continue;
                    }
                    int y = top(w, bx, bz);
                    Material under = w.getBlockState(new BlockPos(bx, y - 1, bz)).getBlock().getMaterial();
                    if (Math.abs(y - cy) <= 2 && (groundLike(under) || under == Material.water)) {
                        return new double[] {bx + 0.5, y, bz + 0.5};
                    }
                }
            }
        }
        return new double[] {x, cy + 1, z};
    }

    private void spawn(WorldServer w, Chest c) {
        if (exp) {
            spawnExp(w, c);
            return;
        }
        double cx = c.pos.getX() + 0.5, cz = c.pos.getZ() + 0.5;
        for (int i = 0; i < 5; i++) {
            double a = i * Math.PI * 2 / 5, x = cx + 2.5 * Math.cos(a), z = cz + 2.5 * Math.sin(a);
            double[] pl = place(w, c, x, z);
            x = pl[0];
            z = pl[2];
            int y = (int) pl[1];
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
        for (int i = 0; i < 5; i++) {
            double a = i * Math.PI * 2 / 5, x = cx + 2.5 * Math.cos(a), z = cz + 2.5 * Math.sin(a);
            double[] pl = place(w, c, x, z);
            x = pl[0];
            z = pl[2];
            int y = (int) pl[1];
            EntityLiving m;
            boolean archer = ARENA && i == 2;
            if (archer) {
                // The archer keeps back, 7 blocks out, and shoots.
                x = cx + 7.0 * Math.cos(a);
                z = cz + 7.0 * Math.sin(a);
                pl = place(w, c, x, z);
                x = pl[0];
                z = pl[2];
                y = (int) pl[1];
            }
            if (i < 3) {
                EntitySkeleton sk = new EntitySkeleton(w);
                sk.setCurrentItemOrArmor(0, new net.minecraft.item.ItemStack(archer ? net.minecraft.init.Items.bow : net.minecraft.init.Items.stone_sword));
                sk.setCurrentItemOrArmor(4, new net.minecraft.item.ItemStack(net.minecraft.init.Items.skull, 1, 3));
                sk.setCurrentItemOrArmor(3, new net.minecraft.item.ItemStack(net.minecraft.init.Items.iron_chestplate));
                sk.setCurrentItemOrArmor(2, new net.minecraft.item.ItemStack(net.minecraft.init.Items.iron_leggings));
                sk.setCurrentItemOrArmor(1, new net.minecraft.item.ItemStack(net.minecraft.init.Items.iron_boots));
                m = sk;
            } else if (ARENA && i == 4) {
                // A brute that hurts everything within five blocks every second.
                EntityZombie zo = new EntityZombie(w);
                zo.setCurrentItemOrArmor(4, new net.minecraft.item.ItemStack(net.minecraft.init.Items.skull, 1, 2));
                m = zo;
                c.aoe.add(m);
            } else {
                net.minecraft.entity.passive.EntityWolf wo = new net.minecraft.entity.passive.EntityWolf(w);
                wo.setAngry(true);
                m = wo;
            }
            m.setLocationAndAngles(x, y, z, 0, 0);
            m.getEntityAttribute(SharedMonsterAttributes.maxHealth).setBaseValue(200);
            m.setHealth(50);
            if (archer) {
                ((EntitySkeleton) m).setCombatTask();
                c.archers.put(m, new double[] {x, y, z});
            }
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
