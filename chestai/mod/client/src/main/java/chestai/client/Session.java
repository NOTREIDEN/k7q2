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

    public boolean on() {
        return pilot != null;
    }

    private File chestFile() {
        ServerData sd = mc.getCurrentServerData();
        String name = sd == null ? "local" : sd.serverIP.toLowerCase(Locale.ROOT).replaceAll("[^a-z0-9.-]", "_");
        return new File(dir, "cofres-" + name + ".txt");
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
                    if (t.length == 3) {
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
            hands.release();
            if (tick == 1 || tick % 600 == 0) {
                return "Chest AI: no veo ningun cofre de farmeo, espero quieto." + Reader.diag(mc, settings)
                    + "\nSi el texto del holograma es otro, ponelo en texto_claim de ajustes.txt.";
            }
            return null;
        }
        float[] o = obs.build(f, mem, new WorldGrid(world));
        int[] a = net.act(o, draws, !settings.draw);
        if (DEBUG && tick % 20 == 0) {
            StringBuilder b = new StringBuilder();
            b.append(String.format(java.util.Locale.ROOT, "CHESTAI t=%d pos=%.1f,%.1f,%.1f yaw=%.0f hp=%.1f mobs=%d holos=%d known=%d a=%s |",
                tick, f.x, f.y, f.z, f.yaw, f.hp, f.mobs.size(), f.holos.size(), mem.chests.size(), java.util.Arrays.toString(a)));
            for (chestai.core.Memory.Known k : mem.chests) {
                b.append(String.format(java.util.Locale.ROOT, " (%d,%d,%d s%d %ds)", k.x, k.y, k.z, k.state, k.secs));
            }
            for (chestai.core.Frame.Mob m : f.mobs) {
                b.append(String.format(java.util.Locale.ROOT, " [mob k%d %.0f,%.0f hp%.0f]", m.kind, m.x, m.z, m.hp));
            }
            System.out.println(b);
        }
        // What the agent can do with what the hotbar holds.
        int pearls = lay.real[Pilot.PEARL] >= 0 ? lay.pearls : 0;
        int potions = lay.real[Pilot.POTION] >= 0 ? lay.potions : 0;
        Pilot.Keys k = pilot.step(a, pearls, pearlLeft, potions, lay.real[Pilot.BOW] >= 0 ? lay.arrows : 0, lay.real[Pilot.GAPPLE] >= 0 ? lay.gapples : 0, f.pickMob >= 0, f.pickChest >= 0);
        if (k.throwIt && k.slot == Pilot.PEARL) {
            pearlUntil = tick + (long) Math.round(settings.pearlCooldownS * 20.0);
        }
        if (DEBUG && k.attack) {
            dbgAttacks++;
            if (mc.objectMouseOver != null && mc.objectMouseOver.entityHit != null) {
                dbgOnMob++;
                net.minecraft.entity.Entity eh = mc.objectMouseOver.entityHit;
                String key = eh.getClass().getSimpleName() + (eh instanceof net.minecraft.entity.EntityLivingBase && ((net.minecraft.entity.EntityLivingBase) eh).hurtTime > 0 ? "-hurt" : "-fresh");
                Integer c = dbgClasses.get(key);
                dbgClasses.put(key, c == null ? 1 : c + 1);
            }
        }
        if (DEBUG && tick % 100 == 0) {
            System.out.println("CHESTAI attacks=" + dbgAttacks + " on_entity=" + dbgOnMob + " pitch=" + me.rotationPitch + " " + dbgClasses);
        }
        hands.press(k, lay);
        if (tick % 200 == 0) {
            saveChests();
        }
        return null;
    }
}
