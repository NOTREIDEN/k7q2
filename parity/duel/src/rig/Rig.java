package rig;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileWriter;
import java.io.InputStream;
import java.lang.reflect.Field;
import java.net.InetAddress;
import java.util.HashMap;
import java.util.Locale;
import java.util.Map;
import java.util.Random;
import net.minecraft.client.Minecraft;
import net.minecraft.client.audio.SoundCategory;
import net.minecraft.client.gui.GuiDisconnected;
import net.minecraft.client.gui.GuiGameOver;
import net.minecraft.client.gui.GuiMainMenu;
import net.minecraft.client.gui.inventory.GuiInventory;
import net.minecraft.client.multiplayer.GuiConnecting;
import net.minecraft.client.network.NetHandlerPlayClient;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.enchantment.Enchantment;
import net.minecraft.enchantment.EnchantmentHelper;
import net.minecraft.entity.Entity;
import net.minecraft.entity.player.EntityPlayerMP;
import net.minecraft.init.Blocks;
import net.minecraft.init.Items;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.network.INetHandler;
import net.minecraft.network.NetHandlerPlayServer;
import net.minecraft.network.Packet;
import net.minecraft.network.play.client.C02PacketUseEntity;
import net.minecraft.network.play.client.C08PacketPlayerBlockPlacement;
import net.minecraft.network.play.client.C0BPacketEntityAction;
import net.minecraft.network.play.server.S02PacketChat;
import net.minecraft.network.play.server.S12PacketEntityVelocity;
import net.minecraft.potion.Potion;
import net.minecraft.potion.PotionEffect;
import net.minecraft.scoreboard.IScoreObjectiveCriteria;
import net.minecraft.scoreboard.ScoreObjective;
import net.minecraft.scoreboard.Scoreboard;
import net.minecraft.server.MinecraftServer;
import net.minecraft.util.BlockPos;
import net.minecraft.util.ChatComponentText;
import net.minecraft.util.FoodStats;
import net.minecraft.world.EnumDifficulty;
import net.minecraft.world.WorldServer;
import net.minecraft.world.WorldSettings;
import net.minecraft.world.WorldType;
import pvpai.plugin.Knock;
import pvpai.plugin.Profile;

/**
 * Duels between two real clients, for watching: one client hosts a flat
 * world with an arena and lets the other in over the network, through a
 * line that holds every packet back like a far-away server would. On each
 * of the two, the mod's own code (mod/client: Session) puts a policy at
 * the keys when a round starts. The host's server hands out the kit,
 * counts the rounds and gives hits the knockback of a profile of the test
 * server's plugin (plugin/: Knock) in place of the game's.
 *
 * It is called from the same hook lines as the parity rig (see
 * ../../src/rig/Rig.java) and stands in for that class on the class path.
 *
 * System properties, all optional: duel.role (host or join), duel.port,
 * duel.dir (where the flag files go), duel.policy (the file of the policy
 * that plays; none: nobody is at the keys), duel.ping and duel.otherping
 * (what the policy is told), duel.cps (lowest and highest, "14-17"),
 * duel.velocity (the share of the horizontal knockback this client takes,
 * as a cheat would have it), duel.view (chunks drawn).
 * For the host also: duel.delay (milliseconds each way for the one that
 * joins), duel.kb (vanilla or servidor), duel.rounds, duel.seconds (the
 * longest a round lasts), duel.speed and duel.hostspeed (the level of
 * Speed of the one that joins and of the host, from 0; 1 is Speed II).
 */
public final class Rig {
    static final boolean HOST = !"join".equals(System.getProperty("duel.role", "host"));
    static final int PORT = Integer.getInteger("duel.port", 25580);
    static final File DIR = new File(System.getProperty("duel.dir", "."));
    static final String POLICY = System.getProperty("duel.policy");
    static final double PING = num("duel.ping", HOST ? 10.0 : 200.0), OTHER_PING = num("duel.otherping", HOST ? 200.0 : 10.0);
    static final double VELOCITY = num("duel.velocity", 1.0);
    static final int VIEW = Integer.getInteger("duel.view", 5);
    static final int DELAY = Integer.getInteger("duel.delay", 100);
    static final String KB = System.getProperty("duel.kb", "servidor");
    static final int ROUNDS = Integer.getInteger("duel.rounds", 3), SECONDS = Integer.getInteger("duel.seconds", 90);
    static final int SPEED = Integer.getInteger("duel.speed", 1), HOST_SPEED = Integer.getInteger("duel.hostspeed", 1);
    /** Half the side of the arena and the height of its floor, as in training. */
    static final int HALF = 24, FLOOR = 4;
    /** Server ticks between two pearls of a player. */
    static final int PEARL_WAIT = 320;
    static final int HEAL_II = 16421, SPEED_II = 8226;

    static double num(String key, double or) {
        String v = System.getProperty(key);
        return v == null ? or : Double.parseDouble(v);
    }

    static void say(String s) {
        System.out.println("duel: " + s);
        System.out.flush();
    }

    static void flag(String name, String text) {
        try {
            File tmp = new File(DIR, name + ".tmp");
            FileWriter w = new FileWriter(tmp);
            w.write(text + "\n");
            w.close();
            tmp.renameTo(new File(DIR, name));
        } catch (Exception e) {
            throw new RuntimeException(e);
        }
    }

    static void set(Object o, Class<?> c, String name, Object v) {
        try {
            Field f = c.getDeclaredField(name);
            f.setAccessible(true);
            f.set(o, v);
        } catch (Exception e) {
            throw new RuntimeException(e);
        }
    }

    static int geti(Object o, Class<?> c, String name) {
        try {
            Field f = c.getDeclaredField(name);
            f.setAccessible(true);
            return f.getInt(o);
        } catch (Exception e) {
            throw new RuntimeException(e);
        }
    }

    // The client.

    static int state = 0, wait = 0, cticks = 0;
    static pvpai.core.Policy policy;
    static pvpai.client.Session session;
    static boolean started;
    /** The client tick at which the policy takes the keys; less than zero: not asked. */
    static volatile int goAt = -1;
    static volatile boolean halt;
    static int deadFor = 0;

    public static void clientPre(Minecraft mc) {
        try {
            client(mc);
        } catch (Throwable t) {
            t.printStackTrace();
            say("client failed: " + t);
            Runtime.getRuntime().halt(3);
        }
    }

    static void client(Minecraft mc) throws Exception {
        started = false;
        cticks++;
        if (state == 0) {
            if (!(mc.currentScreen instanceof GuiMainMenu)) return;
            GameSettings gs = mc.gameSettings;
            gs.pauseOnLostFocus = false;
            gs.renderDistanceChunks = VIEW;
            gs.limitFramerate = 60;
            gs.enableVsync = false;
            gs.fancyGraphics = false;
            gs.ambientOcclusion = 0;
            gs.clouds = 0;
            gs.viewBobbing = true;
            gs.gammaSetting = 1.0F;
            gs.fovSetting = 80.0F;
            gs.guiScale = 2;
            gs.mouseSensitivity = 0.5F;
            gs.smoothCamera = false;
            gs.showDebugInfo = false;
            gs.heldItemTooltips = false;
            gs.difficulty = EnumDifficulty.NORMAL;
            gs.setSoundLevel(SoundCategory.MASTER, 0.0F);
            if (POLICY != null) {
                InputStream in = new FileInputStream(POLICY);
                try {
                    policy = pvpai.core.Policy.load(in);
                } finally {
                    in.close();
                }
            }
            if (HOST) {
                WorldSettings ws = new WorldSettings(1L, WorldSettings.GameType.SURVIVAL, false, false, WorldType.FLAT);
                ws.setWorldName("3;minecraft:bedrock,2*minecraft:dirt,minecraft:grass;0;");
                ws.enableCommands();
                mc.launchIntegratedServer("duel" + System.currentTimeMillis(), "duel", ws);
                state = 1;
            } else {
                mc.displayGuiScreen(new GuiConnecting(new GuiMainMenu(), mc, "127.0.0.1", PORT));
                state = 2;
            }
            wait = 0;
            return;
        }
        if (state == 1) {
            // The host: once it stands in its world, the server gets its arena and opens up.
            if (mc.thePlayer == null || mc.theWorld == null || mc.getIntegratedServer() == null) return;
            if (++wait < 60) return;
            final MinecraftServer server = mc.getIntegratedServer();
            server.addScheduledTask(new Runnable() {
                public void run() {
                    serverSetup(server);
                }
            });
            state = 2;
            wait = 0;
            return;
        }
        // In the game, or on the way into it.
        if (mc.currentScreen instanceof GuiDisconnected) {
            if (++wait > 60) {
                say("not let in, trying again");
                mc.displayGuiScreen(new GuiConnecting(new GuiMainMenu(), mc, "127.0.0.1", PORT));
                wait = 0;
            }
            return;
        }
        if (mc.thePlayer == null || mc.theWorld == null) return;
        if (state == 2) {
            state = 3;
            say("in the world as " + mc.thePlayer.getName());
            flag(HOST ? "host-in" : "join-in", mc.thePlayer.getName());
        }
        mc.inGameHasFocus = false;
        mc.guiAchievement.clearAchievements();
        if (cticks % 400 == 0) say(Minecraft.getDebugFPS() + " frames a second");
        if (mc.currentScreen instanceof GuiGameOver) {
            // Dead: back into the game after a moment.
            if (++deadFor > 40) {
                mc.thePlayer.respawnPlayer();
                mc.displayGuiScreen(null);
                deadFor = 0;
            }
        } else {
            deadFor = 0;
            if (mc.currentScreen != null && !(mc.currentScreen instanceof GuiInventory && on())) mc.displayGuiScreen(null);
        }
        if (halt) {
            halt = false;
            goAt = -1;
            if (on()) {
                session.stop();
                say("the policy leaves the keys: the round is over");
            }
            if (viewTicks > 0) {
                say(String.format(java.util.Locale.ROOT, "the crosshair took back %.0f degrees in %.1f s: %.1f a second",
                    undone, viewTicks / 20.0, undone / (viewTicks / 20.0)));
            }
            undone = 0.0;
            viewTicks = 0;
            viewHas = false;
        }
        if (goAt >= 0 && cticks >= goAt && !on() && policy != null) {
            goAt = -1;
            pvpai.client.Settings s = new pvpai.client.Settings();
            double[] cps = range(System.getProperty("duel.cps", "14-17"));
            s.cpsMin = cps[0];
            s.cpsMax = cps[1];
            s.ping = PING;
            s.otherPing = OTHER_PING;
            s.pearlCooldownS = PEARL_WAIT / 20.0;
            session = new pvpai.client.Session(mc, policy);
            say(session.start(s));
        }
        if (on()) {
            String why = session.tickStart();
            if (why != null) say(why);
            started = on();
        }
    }

    static boolean on() {
        return session != null && session.on();
    }

    static double[] range(String v) {
        String[] p = v.split("-");
        double lo = Double.parseDouble(p[0]);
        return new double[] {lo, p.length > 1 ? Double.parseDouble(p[1]) : lo};
    }

    public static void clientPost(Minecraft mc) {
        if (started && session != null) {
            session.tickEnd();
            if (mc.thePlayer != null) view(mc.thePlayer.rotationYaw, mc.thePlayer.rotationPitch);
        }
        started = false;
    }

    static float[] viewWas = new float[2], turnLast = new float[2];
    static int[] turnAge = new int[2];
    static double undone;
    static int viewTicks;
    static boolean viewHas;

    /** How much of its turning the policy takes straight back, counted as training counts it. */
    static void view(float yaw, float pitch) {
        float[] v = {yaw, pitch};
        if (viewHas) {
            for (int k = 0; k < 2; k++) {
                float d = v[k] - viewWas[k];
                if (d == 0.0F) {
                    turnAge[k] = Math.min(turnAge[k] + 1, 1000);
                    continue;
                }
                if (turnLast[k] * d < 0.0F && turnAge[k] <= 2) undone += Math.min(Math.abs(turnLast[k]), Math.abs(d));
                turnLast[k] = d;
                turnAge[k] = 0;
            }
            viewTicks++;
        } else {
            turnLast[0] = turnLast[1] = 0.0F;
            turnAge[0] = turnAge[1] = 1000;
        }
        viewWas = v;
        viewHas = true;
    }

    public static void onClientSend(Packet p) {
    }

    public static Packet noted(Packet p) {
        return p;
    }

    /** A packet about to be handled, by this client or by the host's server. */
    public static void onHandle(Packet p, INetHandler h) {
        try {
            if (h instanceof NetHandlerPlayClient) {
                if (p instanceof S02PacketChat) {
                    String text = ((S02PacketChat) p).getChatComponent().getUnformattedText();
                    if (text.startsWith("Ronda ")) {
                        goAt = cticks + 30;
                        // What was said before the round is taken off the screen.
                        Minecraft.getMinecraft().ingameGUI.getChatGUI().clearChatMessages();
                    } else if (text.startsWith("Fin de la ronda")) {
                        halt = true;
                    }
                } else if (p instanceof S12PacketEntityVelocity && VELOCITY != 1.0) {
                    Minecraft mc = Minecraft.getMinecraft();
                    S12PacketEntityVelocity v = (S12PacketEntityVelocity) p;
                    if (mc.thePlayer != null && v.getEntityID() == mc.thePlayer.getEntityId()) {
                        // What a cheat does: only a share of the push to the sides is taken.
                        set(v, S12PacketEntityVelocity.class, "motionX", Integer.valueOf((int) (v.getMotionX() * VELOCITY)));
                        set(v, S12PacketEntityVelocity.class, "motionZ", Integer.valueOf((int) (v.getMotionZ() * VELOCITY)));
                    }
                }
            } else if (h instanceof NetHandlerPlayServer) {
                served(p, ((NetHandlerPlayServer) h).playerEntity);
            }
        } catch (Throwable t) {
            t.printStackTrace();
            say("failed on a packet: " + t);
            Runtime.getRuntime().halt(3);
        }
    }

    // The host's server.

    /** What is kept about a player, by name: a player that comes back to life is a new one to the server. */
    static final class Fighter {
        double lastGroundY;
        boolean touchedTop, clientSprint;
        long pearlReady;
        /** The pearls the server is not to see until the end of this tick, and the slot they are in. */
        ItemStack held;
        int heldSlot;
        int wins;
    }

    /** A hit the server is about to work out. */
    static final class Hit {
        long tick;
        EntityPlayerMP victim;
        double ax, az, vx, vy, vz;
        double[] motion;
        float yaw;
        boolean ground, serverSprint, clientSprint;
        int ench;
    }

    static final Map<String, Fighter> fighters = new HashMap<String, Fighter>();
    static final Profile profile = new Profile();
    static final Random random = new Random(7);
    static boolean setup;
    static long stick = 0;
    static Hit pending;
    static String hostName;
    /** 0 waiting for two players, 1 the kits are out, 2 fighting, 3 over. */
    static int phase = 0, round = 0;
    static long phaseAt = 0;
    static long replaced = 0;
    static boolean met;

    static Fighter fighter(EntityPlayerMP p) {
        Fighter f = fighters.get(p.getName());
        if (f == null) {
            f = new Fighter();
            f.lastGroundY = p.posY;
            fighters.put(p.getName(), f);
        }
        return f;
    }

    static void serverSetup(MinecraftServer server) {
        try {
            WorldServer w = server.worldServerForDimension(0);
            w.getGameRules().setOrCreateGameRule("doDaylightCycle", "false");
            w.getGameRules().setOrCreateGameRule("doMobSpawning", "false");
            w.getGameRules().setOrCreateGameRule("doWeatherCycle", "false");
            w.setWorldTime(6000L);
            // Walls round the arena: the inside goes from -HALF up to HALF.
            for (int i = -HALF - 1; i <= HALF; i++) {
                for (int y = FLOOR; y < FLOOR + 6; y++) {
                    w.setBlockState(new BlockPos(i, y, -HALF - 1), Blocks.stonebrick.getDefaultState());
                    w.setBlockState(new BlockPos(i, y, HALF), Blocks.stonebrick.getDefaultState());
                    w.setBlockState(new BlockPos(-HALF - 1, y, i), Blocks.stonebrick.getDefaultState());
                    w.setBlockState(new BlockPos(HALF, y, i), Blocks.stonebrick.getDefaultState());
                }
            }
            w.setSpawnPoint(new BlockPos(0, FLOOR, 0));
            if (KB.equals("servidor")) {
                profile.kind = Profile.HIT;
                profile.name = "servidor";
            } else if (!KB.equals("vanilla")) {
                throw new IllegalArgumentException("no knockback called " + KB);
            }
            Scoreboard sb = w.getScoreboard();
            ScoreObjective o = sb.addScoreObjective("duelo", IScoreObjectiveCriteria.DUMMY);
            o.setDisplayName("Rondas ganadas");
            sb.setObjectiveInDisplaySlot(1, o);
            server.setOnlineMode(false);
            server.getNetworkSystem().addLanEndpoint((InetAddress) null, PORT);
            Delay.start(PORT + 1, PORT, DELAY);
            hostName = Minecraft.getMinecraft().getSession().getUsername();
            setup = true;
            say("the server lets players in on " + PORT + ", and on " + (PORT + 1) + " with " + DELAY + " ms each way; knockback " + KB);
            flag("ready", String.valueOf(PORT + 1));
        } catch (Throwable t) {
            t.printStackTrace();
            say("server setup failed: " + t);
            Runtime.getRuntime().halt(3);
        }
    }

    static EntityPlayerMP[] two(MinecraftServer server) {
        java.util.List<EntityPlayerMP> l = server.getConfigurationManager().getPlayerList();
        if (l.size() != 2) return null;
        EntityPlayerMP a = l.get(0), b = l.get(1);
        // The host second.
        return a.getName().equals(hostName) ? new EntityPlayerMP[] {b, a} : new EntityPlayerMP[] {a, b};
    }

    static void tell(MinecraftServer server, String text) {
        server.getConfigurationManager().sendChatMsg(new ChatComponentText(text));
    }

    public static void serverPre(MinecraftServer server) {
        if (!setup) return;
        try {
            stick++;
            EntityPlayerMP[] p = two(server);
            if (p == null) return;
            boolean alive = p[0].isEntityAlive() && p[1].isEntityAlive();
            if (!met) {
                met = true;
                phaseAt = stick;
                Scoreboard sb = server.worldServerForDimension(0).getScoreboard();
                for (int i = 0; i < 2; i++) {
                    // Got now, it is not announced in the middle of a round.
                    p[i].triggerAchievement(net.minecraft.stats.AchievementList.openInventory);
                    sb.getValueFromObjective(p[i].getName(), sb.getObjective("duelo")).setScorePoints(0);
                }
            }
            long since = stick - phaseAt;
            if (phase == 0) {
                if (alive && since > 60) {
                    round++;
                    // Apart by eight to twelve blocks, somewhere in the arena, facing each other.
                    double d = 8.0 + 4.0 * random.nextDouble(), ang = 2.0 * Math.PI * random.nextDouble();
                    double cx = (random.nextDouble() - 0.5) * 2.0 * (HALF - 10), cz = (random.nextDouble() - 0.5) * 2.0 * (HALF - 10);
                    for (int i = 0; i < 2; i++) {
                        double s = i == 0 ? -0.5 : 0.5;
                        double x = cx + s * d * Math.cos(ang), z = cz + s * d * Math.sin(ang);
                        double tx = cx - s * d * Math.cos(ang), tz = cz - s * d * Math.sin(ang);
                        float yaw = (float) Math.toDegrees(Math.atan2(-(tx - x), tz - z));
                        kit(p[i], i == 0 ? SPEED : HOST_SPEED);
                        p[i].playerNetServerHandler.setPlayerLocation(x, FLOOR, z, yaw, 0.0F);
                    }
                    phase = 1;
                    phaseAt = stick;
                }
            } else if (phase == 1) {
                if (since == 40) {
                    tell(server, "Ronda " + round + ": " + p[0].getName() + " contra " + p[1].getName());
                }
                if (since >= 40) {
                    phase = 2;
                    phaseAt = stick;
                }
            } else if (phase == 2) {
                String end = null;
                if (!alive) {
                    int won = p[0].isEntityAlive() ? 0 : 1;
                    if (p[won].isEntityAlive()) {
                        fighter(p[won]).wins++;
                        end = "gana " + p[won].getName() + " con " + String.format(Locale.ROOT, "%.1f", p[won].getHealth() / 2.0F) + " corazones";
                    } else {
                        end = "mueren los dos";
                    }
                } else if (since > SECONDS * 20L) {
                    end = "se acabo el tiempo";
                }
                if (end != null) {
                    tell(server, "Fin de la ronda " + round + ": " + end);
                    say("round " + round + ": " + end + " after " + since / 20 + " s; hits changed " + replaced);
                    Scoreboard sb = server.worldServerForDimension(0).getScoreboard();
                    ScoreObjective o = sb.getObjective("duelo");
                    for (int i = 0; i < 2; i++) {
                        sb.getValueFromObjective(p[i].getName(), o).setScorePoints(fighter(p[i]).wins);
                    }
                    phase = 3;
                    phaseAt = stick;
                }
            } else if (phase == 3) {
                if (since > 100) {
                    if (round >= ROUNDS) {
                        flag("done", "rounds " + round);
                        phase = 4;
                    } else {
                        phase = 0;
                        phaseAt = stick - 40;
                    }
                }
            }
        } catch (Throwable t) {
            t.printStackTrace();
            say("server failed: " + t);
            Runtime.getRuntime().halt(3);
        }
    }

    static void unbreakable(ItemStack s) {
        if (s.getTagCompound() == null) s.setTagCompound(new NBTTagCompound());
        s.getTagCompound().setBoolean("Unbreakable", true);
    }

    /** The kit the agents are trained with: sword first, pearls second, potions, steak last. */
    static void kit(EntityPlayerMP p, int speed) {
        p.clearActivePotions();
        if (speed >= 0) p.addPotionEffect(new PotionEffect(Potion.moveSpeed.id, speed == 1 ? 20 * 60 : 10000000, speed));
        p.extinguish();
        for (int i = 0; i < 36; i++) p.inventory.mainInventory[i] = null;
        ItemStack sword = new ItemStack(Items.diamond_sword);
        sword.addEnchantment(Enchantment.sharpness, 2);
        unbreakable(sword);
        Item[] set = {Items.diamond_boots, Items.diamond_leggings, Items.diamond_chestplate, Items.diamond_helmet};
        for (int i = 0; i < 4; i++) {
            ItemStack a = new ItemStack(set[i]);
            a.addEnchantment(Enchantment.protection, 2);
            if (i == 0) a.addEnchantment(Enchantment.featherFalling, 4);
            unbreakable(a);
            p.inventory.armorInventory[i] = a;
        }
        p.inventory.mainInventory[0] = sword;
        p.inventory.mainInventory[1] = new ItemStack(Items.ender_pearl, 16);
        for (int i = 2; i < 7; i++) p.inventory.mainInventory[i] = new ItemStack(Items.potionitem, 1, HEAL_II);
        p.inventory.mainInventory[7] = new ItemStack(Items.potionitem, 1, SPEED_II);
        p.inventory.mainInventory[8] = new ItemStack(Items.cooked_beef, 64);
        for (int i = 9; i < 21; i++) p.inventory.mainInventory[i] = new ItemStack(Items.potionitem, 1, HEAL_II);
        p.inventory.mainInventory[21] = new ItemStack(Items.potionitem, 1, SPEED_II);
        p.inventory.currentItem = 0;
        p.setHealth(p.getMaxHealth());
        p.getFoodStats().setFoodLevel(20);
        set(p.getFoodStats(), FoodStats.class, "foodSaturationLevel", Float.valueOf(5.0F));
        set(p.getFoodStats(), FoodStats.class, "foodExhaustionLevel", Float.valueOf(0.0F));
        p.maxHurtResistantTime = profile.hitDelay;
        p.hurtResistantTime = 0;
        p.inventoryContainer.detectAndSendChanges();
        p.sendContainerToPlayer(p.inventoryContainer);
        Fighter f = fighter(p);
        f.pearlReady = 0;
        f.touchedTop = false;
        f.lastGroundY = FLOOR;
    }

    /** A packet of a player, about to be handled by the server. */
    static void served(Packet p, EntityPlayerMP from) {
        if (!setup) return;
        Fighter f = fighter(from);
        if (p instanceof C0BPacketEntityAction) {
            C0BPacketEntityAction.Action a = ((C0BPacketEntityAction) p).getAction();
            if (a == C0BPacketEntityAction.Action.START_SPRINTING) f.clientSprint = true;
            if (a == C0BPacketEntityAction.Action.STOP_SPRINTING) f.clientSprint = false;
        } else if (p instanceof C08PacketPlayerBlockPlacement) {
            ItemStack held = from.inventory.getCurrentItem();
            if (((C08PacketPlayerBlockPlacement) p).getPlacedBlockDirection() == 255 && held != null && held.getItem() == Items.ender_pearl) {
                if (stick < f.pearlReady) {
                    // As a server's plugin has it: the click comes to nothing and the player
                    // is sent its inventory again. The server does not get to see the pearls
                    // until this tick is over.
                    if (f.held == null) {
                        f.held = held;
                        f.heldSlot = from.inventory.currentItem;
                        from.inventory.mainInventory[f.heldSlot] = null;
                    }
                } else {
                    f.pearlReady = stick + PEARL_WAIT;
                }
            }
        } else if (p instanceof C02PacketUseEntity) {
            C02PacketUseEntity u = (C02PacketUseEntity) p;
            pending = null;
            if (u.getAction() != C02PacketUseEntity.Action.ATTACK || profile.kind == Profile.VANILLA) return;
            Entity e = u.getEntityFromWorld(from.worldObj);
            if (!(e instanceof EntityPlayerMP)) return;
            EntityPlayerMP v = (EntityPlayerMP) e;
            // A hit in the time a player cannot be hit again knocks nobody back.
            if ((float) v.hurtResistantTime > (float) v.maxHurtResistantTime / 2.0F) return;
            Hit h = new Hit();
            h.tick = stick;
            h.victim = v;
            h.ax = from.posX;
            h.az = from.posZ;
            h.yaw = from.rotationYaw;
            h.vx = v.posX;
            h.vy = v.posY;
            h.vz = v.posZ;
            h.motion = new double[] {v.motionX, v.motionY, v.motionZ};
            h.ground = v.onGround;
            h.serverSprint = from.isSprinting();
            h.clientSprint = f.clientSprint;
            h.ench = EnchantmentHelper.getKnockbackModifier(from);
            Fighter fv = fighter(v);
            if (h.ground) {
                fv.lastGroundY = h.vy;
                fv.touchedTop = false;
            }
            pending = h;
        }
    }

    /** A packet the server is about to send: the velocity of a hit gets the profile's numbers. */
    public static void onServerSend(NetHandlerPlayServer handler, Packet p) {
        if (!(p instanceof S12PacketEntityVelocity) || pending == null) return;
        try {
            Hit h = pending;
            S12PacketEntityVelocity v = (S12PacketEntityVelocity) p;
            if (h.tick != stick || handler.playerEntity != h.victim || v.getEntityID() != h.victim.getEntityId()) return;
            pending = null;
            double dx = h.ax - h.vx, dz = h.az - h.vz;
            if (dx * dx + dz * dz < 1.0E-4) return;
            double[] out;
            if (profile.kind == Profile.HIT) {
                Fighter fv = fighter(h.victim);
                boolean sprint = profile.stopSprint ? h.serverSprint : h.clientSprint;
                boolean[] top = {fv.touchedTop};
                out = Knock.hit(profile, h.ax, h.az, h.vx, h.vy, h.vz, h.ground, h.yaw, h.ench + (sprint ? 1 : 0), fv.lastGroundY, top);
                fv.touchedTop = top[0];
            } else {
                out = Knock.game(profile, h.motion, h.ax, h.az, h.vx, h.vz, h.yaw, h.ench + (h.serverSprint ? 1 : 0));
            }
            int[] sent = Knock.sent(out);
            set(v, S12PacketEntityVelocity.class, "motionX", Integer.valueOf(sent[0]));
            set(v, S12PacketEntityVelocity.class, "motionY", Integer.valueOf(sent[1]));
            set(v, S12PacketEntityVelocity.class, "motionZ", Integer.valueOf(sent[2]));
            replaced++;
        } catch (Throwable t) {
            t.printStackTrace();
            say("failed on a hit: " + t);
            Runtime.getRuntime().halt(3);
        }
    }

    public static void serverPost(MinecraftServer server) {
        if (!setup) return;
        java.util.List<EntityPlayerMP> l = server.getConfigurationManager().getPlayerList();
        for (EntityPlayerMP p : l) {
            Fighter f = fighter(p);
            if (p.onGround) {
                f.lastGroundY = p.posY;
                f.touchedTop = false;
            }
            if (f.held != null) {
                if (p.inventory.mainInventory[f.heldSlot] == null) p.inventory.mainInventory[f.heldSlot] = f.held;
                f.held = null;
                p.sendContainerToPlayer(p.inventoryContainer);
            }
        }
    }
}
