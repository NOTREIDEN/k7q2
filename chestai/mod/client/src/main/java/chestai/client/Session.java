package chestai.client;

import java.io.BufferedReader;
import java.io.File;
import java.io.FileReader;
import java.io.FileWriter;
import java.io.IOException;
import java.io.PrintWriter;
import java.util.Locale;
import java.util.Random;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.gui.inventory.GuiContainer;
import net.minecraft.client.multiplayer.ServerData;
import net.minecraft.client.multiplayer.WorldClient;
import net.minecraft.client.network.NetworkPlayerInfo;
import net.minecraft.util.MouseHelper;
import chestai.core.Frame;
import chestai.core.Memory;
import chestai.core.Net;
import chestai.core.Obs;
import chestai.core.Pilot;

/**
 * The agent at the keys from the moment the player hands them over until
 * the player takes them back. The farm chests it has seen are kept per
 * server in config/chestai, so it knows where they are the next time.
 */
public final class Session {
    private static final boolean DEBUG = Boolean.getBoolean("chestai.debug");
    private int dbgAttacks, dbgOnMob;
    /** Blows on a mob that was not in its half second after a hit, and how many of them took health off. */
    private int blows, blowsLanded;
    private int blowMob = -1;
    private float blowHp;
    private long blowAt;
    private final java.util.Map<String, Integer> dbgClasses = new java.util.TreeMap<String, Integer>();
    private static final double PING = 200.0;

    /** A mouse that does not turn the view while the agent aims. */
    private static final class Still extends MouseHelper {
        @Override
        public void mouseXYChange() {
            super.mouseXYChange();
            deltaX = 0;
            deltaY = 0;
        }
    }

    private final Minecraft mc;
    private final Net net;
    private final File dir;
    private final Random draws = new Random();
    private Settings settings;
    private Memory mem;
    private Pilot pilot;
    private Hands hands;
    private final Obs obs = new Obs();
    private Coach coach;
    private EntityPlayerSP me;
    private WorldClient world;
    private MouseHelper mouse;
    private long tick;
    private long pearlUntil;
    private boolean first;
    private int saved;
    private double ping;
    private File chestFile;
    private int mostMobs;

    /** Farm chests the agent knows of. */
    public int chestsKnown() {
        return mem == null ? 0 : mem.chests.size();
    }

    /** The most mobs of waves seen at once. */
    public int mostMobs() {
        return mostMobs;
    }

    public Session(Minecraft mc, Net net, File dir) {
        this.mc = mc;
        this.net = net;
        this.dir = dir;
    }

    private java.io.PrintWriter logOut;

    /** A line of the play log (config/chestai/registro.txt), for working out what goes wrong on a server. */
    private void log(String line) {
        if (DEBUG) {
            System.out.println(line);
        }
        if (!settings.log) {
            return;
        }
        try {
            File lf = new File(dir, "registro.txt");
            if (logOut == null) {
                if (lf.length() > 8L * 1024 * 1024) {
                    lf.delete();
                }
                logOut = new java.io.PrintWriter(new java.io.FileWriter(lf, true), true);
                logOut.println("== " + new java.util.Date());
            }
            logOut.println(line);
        } catch (java.io.IOException e) {
            settings.log = false;
        }
    }

    /** {yaw, pitch} to add to the camera this frame, or null. */
    public float[] cameraOffset(float partial) {
        return pilot == null || hands == null ? null : hands.cameraOffset(partial);
    }

    public boolean on() {
        return pilot != null;
    }

    private File chestFile() {
        ServerData sd = mc.getCurrentServerData();
        String name = sd == null ? "local" : sd.serverIP.toLowerCase(Locale.ROOT).replaceAll("[^a-z0-9.-]", "_");
        // v2: the first version also took the names over mobs for chests, so its lists are left aside.
        return new File(dir, "cofres2-" + name + ".txt");
    }

    private void loadChests() {
        if (!chestFile.isFile()) {
            return;
        }
        try {
            BufferedReader r = new BufferedReader(new FileReader(chestFile));
            try {
                String line;
                while ((line = r.readLine()) != null) {
                    String[] t = line.trim().split("\\s+");
                    if (t.length == 3 && mem.near(Integer.parseInt(t[0]), Integer.parseInt(t[1]), Integer.parseInt(t[2]), 2.5) < 0) {
                        mem.chest(Integer.parseInt(t[0]), Integer.parseInt(t[1]), Integer.parseInt(t[2]));
                    }
                }
            } finally {
                r.close();
            }
        } catch (IOException e) {
            e.printStackTrace();
        } catch (NumberFormatException e) {
            e.printStackTrace();
        }
    }

    private void saveChests() {
        if (mem.chests.size() == saved) {
            return;
        }
        try {
            dir.mkdirs();
            PrintWriter w = new PrintWriter(new FileWriter(chestFile));
            try {
                for (Memory.Known k : mem.chests) {
                    w.println(k.x + " " + k.y + " " + k.z);
                }
            } finally {
                w.close();
            }
            saved = mem.chests.size();
        } catch (IOException e) {
            e.printStackTrace();
        }
    }

    public String start(Settings s) {
        stop();
        if (mc.thePlayer == null || mc.theWorld == null || mc.getNetHandler() == null) {
            return "Chest AI: no hay partida.";
        }
        settings = s;
        me = mc.thePlayer;
        world = mc.theWorld;
        mem = new Memory();
        chestFile = chestFile();
        loadChests();
        saved = mem.chests.size();
        double cps = s.cpsMin + draws.nextDouble() * (s.cpsMax - s.cpsMin);
        pilot = new Pilot(cps);
        hands = new Hands(mc);
        coach = new Coach();
        Layout lay = Layout.of(me.inventory);
        if (lay.real[Pilot.SWORD] >= 0) {
            me.inventory.currentItem = lay.real[Pilot.SWORD];
        }
        ping = s.ping;
        boolean guessed = false;
        if (ping < 0) {
            NetworkPlayerInfo info = mc.getNetHandler().getPlayerInfo(me.getUniqueID());
            ping = info != null && info.getResponseTime() > 0 ? info.getResponseTime() : -1;
            if (ping < 0) {
                ping = PING;
                guessed = true;
            }
        }
        net.reset();
        tick = 0;
        pearlUntil = 0;
        first = true;
        mouse = mc.mouseHelper;
        mc.mouseHelper = new Still();
        return String.format(Locale.ROOT, "Chest AI prendido. %d cofres conocidos en este servidor. Ping %.0f ms%s, %.1f clics por segundo. Barra: %s.",
            mem.chests.size(), ping, guessed ? " (supuesto)" : "", cps, kit(lay));
    }

    private static String kit(Layout l) {
        String[] names = {"espada", "arco", "manzanas", "perlas", "pociones"};
        StringBuilder b = new StringBuilder();
        for (int i = 0; i < 5; i++) {
            if (i > 0) {
                b.append(", ");
            }
            b.append(names[i]).append(l.real[i] >= 0 ? " en " + (l.real[i] + 1) : " no hay");
        }
        return b.toString();
    }

    public void stop() {
        if (pilot == null) {
            return;
        }
        try {
            hands.release();
        } catch (RuntimeException e) {
            // Nothing left to let go.
        }
        if (mc.mouseHelper instanceof Still) {
            mc.mouseHelper = mouse;
        }
        saveChests();
        if (logOut != null) {
            logOut.close();
            logOut = null;
        }
        pilot = null;
    }

    private String check() {
        if (mc.thePlayer != me || mc.theWorld != world || mc.getNetHandler() == null) {
            return "Chest AI apagado: cambio el mundo.";
        }
        if (me.isDead || me.getHealth() <= 0.0F) {
            return "Chest AI apagado: el jugador murio.";
        }
        if (mc.currentScreen != null && !(mc.currentScreen instanceof GuiContainer)) {
            return "Chest AI apagado: se abrio otra pantalla.";
        }
        return null;
    }

    /** At the start of a client tick. A line for the player when the agent leaves the keys, else null. */
    public String tickStart() {
        if (pilot == null) {
            return null;
        }
        String why = check();
        if (why != null) {
            stop();
            return why;
        }
        if (mc.isGamePaused()) {
            return null;
        }
        // A chest window opened by a claim: shut it, the reward is in.
        if (mc.currentScreen instanceof GuiContainer) {
            me.closeScreen();
        }
        tick++;
        Layout lay = Layout.of(me.inventory);
        int pearlLeft = (int) Math.max(0, pearlUntil - tick);
        Frame f = Reader.read(mc, mem, pilot, lay, settings, tick, ping, pearlLeft);
        mostMobs = Math.max(mostMobs, f.mobs.size());
        mem.update(f, first);
        first = false;
        if (mem.chests.isEmpty()) {
            // No chest known: nothing to go for, so the keys stay still
            // instead of the network wandering about with an empty map.
            // Mobs at hand are still fought.
            int[] idle = {1, 1, 0, 0, 7, 5, 0, 0, 0};
            coach.before(f, mem, pilot, idle);
            if (coach.mode == Coach.FIGHT) {
                Layout l2 = lay;
                Pilot.Keys k2 = pilot.step(idle, 0, 1, 0, 0, l2.real[Pilot.GAPPLE] >= 0 ? l2.gapples : 0, f.pickMob >= 0, f.pickChest >= 0);
                coach.after(f, k2);
                hands.press(k2, l2);
            } else {
                hands.release();
            }
            if (tick == 1 || tick % 600 == 0) {
                return "Chest AI: no veo ningun cofre de farmeo, espero quieto." + Reader.diag(mc, settings)
                    + "\nSi el texto del holograma es otro, ponelo en texto_claim de ajustes.txt.";
            }
            return null;
        }
        float[] o = obs.build(f, mem, new WorldGrid(world));
        int[] a = net.act(o, draws, !settings.draw);
        int[] fromNet = a.clone();
        coach.before(f, mem, pilot, a);
        if ((DEBUG || settings.log) && tick % 10 == 0) {
            StringBuilder b = new StringBuilder();
            b.append(String.format(java.util.Locale.ROOT, "CHESTAI t=%d pos=%.1f,%.1f,%.1f yaw=%.0f hp=%.1f mobs=%d holos=%d known=%d net=%s modo=%d |",
                tick, f.x, f.y, f.z, f.yaw, f.hp, f.mobs.size(), f.holos.size(), mem.chests.size(), java.util.Arrays.toString(fromNet), coach.mode));
            for (chestai.core.Memory.Known k : mem.chests) {
                b.append(String.format(java.util.Locale.ROOT, " (%d,%d,%d s%d %ds)", k.x, k.y, k.z, k.state, k.secs));
            }
            for (chestai.core.Frame.Mob m : f.mobs) {
                double dx = m.x - f.x, dz = m.z - f.z, dy = m.y + m.h * 0.5 - (f.y + 1.62);
                double want = Math.toDegrees(Math.atan2(-dx, dz));
                double err = ((want - f.yaw) % 360.0 + 540.0) % 360.0 - 180.0;
                double perr = -Math.toDegrees(Math.atan2(dy, Math.sqrt(dx * dx + dz * dz))) - me.rotationPitch;
                b.append(String.format(java.util.Locale.ROOT, " [mob k%d %.0f,%.0f d%.1f hp%.0f/%.0f err%.0f/%.0f]", m.kind, m.x, m.z,
                    Math.sqrt(dx * dx + dz * dz), m.hp, m.maxHp, err, perr));
            }
            b.append(" pick=").append(f.pickMob).append(" on=").append(mc.objectMouseOver != null && mc.objectMouseOver.entityHit != null
                ? mc.objectMouseOver.entityHit.getClass().getSimpleName() : "-");
            log(b.toString());
        }
        // What the agent can do with what the hotbar holds.
        int pearls = lay.real[Pilot.PEARL] >= 0 ? lay.pearls : 0;
        int potions = lay.real[Pilot.POTION] >= 0 ? lay.potions : 0;
        Pilot.Keys k = pilot.step(a, pearls, pearlLeft, potions, lay.real[Pilot.BOW] >= 0 ? lay.arrows : 0, lay.real[Pilot.GAPPLE] >= 0 ? lay.gapples : 0, f.pickMob >= 0, f.pickChest >= 0);
        coach.after(f, k);
        if (k.throwIt && k.slot == Pilot.PEARL) {
            pearlUntil = tick + (long) Math.round(settings.pearlCooldownS * 20.0);
        }
        if ((DEBUG || settings.log) && k.attack) {
            dbgAttacks++;
            if (mc.objectMouseOver != null && mc.objectMouseOver.entityHit != null) {
                dbgOnMob++;
                net.minecraft.entity.Entity eh = mc.objectMouseOver.entityHit;
                String key = eh.getClass().getSimpleName() + (eh instanceof net.minecraft.entity.EntityLivingBase && ((net.minecraft.entity.EntityLivingBase) eh).hurtTime > 0 ? "-hurt" : "-fresh");
                Integer c = dbgClasses.get(key);
                dbgClasses.put(key, c == null ? 1 : c + 1);
            }
        }
        if ((DEBUG || settings.log) && tick % 100 == 0) {
            log("CHESTAI golpes=" + blows + " con_dano=" + blowsLanded + " attacks=" + dbgAttacks + " on_entity=" + dbgOnMob + " pitch=" + me.rotationPitch + " " + dbgClasses);
        }
        // Whether blows land: the mob's health 10 ticks after a blow on it, as the server sends it.
        if (blowMob >= 0 && tick - blowAt >= 10) {
            boolean landed = true;
            for (chestai.core.Frame.Mob mm : f.mobs) {
                if (mm.id == blowMob) {
                    landed = mm.hp < blowHp;
                }
            }
            if (landed) {
                blowsLanded++;
            }
            blowMob = -1;
        }
        if (k.attack && blowMob < 0 && f.pickMob >= 0) {
            for (chestai.core.Frame.Mob mm : f.mobs) {
                if (mm.id == f.pickMob && mm.hurtTime == 0 && mm.hp < mm.maxHp + 1) {
                    blows++;
                    blowMob = mm.id;
                    blowHp = mm.hp;
                    blowAt = tick;
                }
            }
        }
        hands.press(k, lay);
        if (tick % 200 == 0) {
            saveChests();
        }
        return null;
    }
}
