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
import net.minecraft.client.settings.KeyBinding;
import net.minecraft.entity.Entity;
import net.minecraft.entity.EntityLivingBase;
import net.minecraft.util.MovingObjectPosition;
import net.minecraft.tileentity.TileEntity;
import net.minecraft.util.BlockPos;
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
    /** Blows queued through SilentAim, and of them the ones the client's own attack went out for. */
    private int dbgAttacks, dbgOnMob;
    /** Blows on a mob the client attacked, and how many of them were seen to take health off (or hurt it again). */
    private int blows, blowsLanded;
    /** The mob of this tick's queued blow, and the one of the blow being watched for whether it landed. */
    private EntityLivingBase queuedTarget, blowTarget;
    /** Whether the client's attack went out for this tick's queued blow already. */
    private boolean queuedAttackConsumed;
    private float blowHp;
    private int blowHurtTime;
    private long blowAt;
    private static final double PING = 200.0;

    /**
     * A mouse that does not turn the view while the agent aims; while SilentAim
     * fights the player may turn it (the agent then only walks), and when the
     * player last did is kept.
     */
    private final class CombatMouse extends MouseHelper {
        boolean enabled;
        long lastManualTurn = -1000;

        @Override
        public void mouseXYChange() {
            super.mouseXYChange();
            if (!enabled) {
                deltaX = 0;
                deltaY = 0;
            } else if (deltaX != 0 || deltaY != 0) {
                lastManualTurn = tick;
            }
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
    private Refill refill;
    /** -Dchestai.survive: after a death, the chests where it happened left aside a while; earlier retreat without potions. */
    static final boolean SURVIVE = Boolean.getBoolean("chestai.survive");
    private static final long DEATH_SKIP_MS = 180_000;
    /** Where the player died lately: x, y, z, and when (ms). Kept across restarts of the agent. */
    private static final java.util.List<double[]> DEATHS = new java.util.ArrayList<double[]>();
    /** The blows through Minecadia's SilentAim; null without it (no fighting then). */
    private VapeCombat combat;
    private final Reader reader = new Reader();
    private final Obs obs = new Obs();
    private Coach coach;
    private EntityPlayerSP me;
    private WorldClient world;
    private MouseHelper mouse;
    private CombatMouse combatMouse;
    private long tick;
    private long pearlUntil;
    private boolean first;
    /** Whether the SilentAim settings were open: the keys and SilentAim let go meanwhile. */
    private boolean editingSilentAim;
    private int saved;
    private double ping;
    private File chestFile;
    private int mostMobs;
    private MapStore map;
    private Planner planner;
    /** Chests of the loaded chunks not shown to be farm chests yet, and how long the player has been by each. */
    private final java.util.Map<Long, Integer> candidates = new java.util.HashMap<Long, Integer>();
    /** Chests that were looked at and are no farm chests. */
    private final java.util.Set<Long> notFarm = new java.util.HashSet<Long>();
    /** Whether the chat has been told it is out of apples and potions. */
    private boolean saidOut;
    private File notFarmFile;

    /** Farm chests the agent knows of. */
    public int chestsKnown() {
        return mem == null ? 0 : mem.chests.size();
    }

    /** What the coach is doing, for a test's log. */
    /** Clicks, clicks on a mob, blows on a mob that could take one, and of them the ones that took health. */
    public String hitLine() {
        return dbgAttacks + "/" + dbgOnMob + "/" + blows + "/" + blowsLanded;
    }

    public String coachLine() {
        return coach == null ? "-" : coach.describe();
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

    private String serverName() {
        ServerData sd = mc.getCurrentServerData();
        return sd == null ? "local" : sd.serverIP.toLowerCase(Locale.ROOT).replaceAll("[^a-z0-9.-]", "_");
    }

    private File chestFile() {
        // v2: the first version also took the names over mobs for chests, so its lists are left aside.
        return new File(dir, "cofres2-" + serverName() + ".txt");
    }

    private static long blockKey(int x, int y, int z) {
        return new BlockPos(x, y, z).toLong();
    }

    private void loadNotFarm() {
        if (!notFarmFile.isFile()) {
            return;
        }
        try {
            BufferedReader r = new BufferedReader(new FileReader(notFarmFile));
            try {
                String line;
                while ((line = r.readLine()) != null) {
                    String[] t = line.trim().split("\\s+");
                    if (t.length == 3) {
                        notFarm.add(blockKey(Integer.parseInt(t[0]), Integer.parseInt(t[1]), Integer.parseInt(t[2])));
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

    private void saveNotFarm() {
        try {
            dir.mkdirs();
            PrintWriter w = new PrintWriter(new FileWriter(notFarmFile));
            try {
                for (long k : notFarm) {
                    BlockPos p = BlockPos.fromLong(k);
                    w.println(p.getX() + " " + p.getY() + " " + p.getZ());
                }
            } finally {
                w.close();
            }
        } catch (IOException e) {
            e.printStackTrace();
        }
    }

    /**
     * The chests of the loaded chunks (as far as the server sends them)
     * within the search radius that are not known yet: possible farm
     * chests, gone to when there is nothing better, until a look at them
     * shows a farm hologram or none.
     */
    private void searchChests(Frame f) {
        for (Object o : new java.util.ArrayList<Object>(world.loadedTileEntityList)) {
            if (!(o instanceof net.minecraft.tileentity.TileEntityChest)) {
                continue;
            }
            BlockPos p = Reader.chestAnchor(world, ((TileEntity) o).getPos());
            double dx = p.getX() + 0.5 - f.x, dz = p.getZ() + 0.5 - f.z;
            long k = p.toLong();
            if (dx * dx + dz * dz > settings.searchRadius * settings.searchRadius || notFarm.contains(k)
                || mem.indexOfChest(p.getX(), p.getY(), p.getZ()) >= 0) {
                continue;
            }
            mem.chest(p.getX(), p.getY(), p.getZ());
            candidates.put(k, 0);
        }
    }

    /** A possible farm chest the player has stood by for three seconds without seeing a farm hologram: none. */
    private void checkCandidates(Frame f) {
        java.util.Iterator<java.util.Map.Entry<Long, Integer>> it = candidates.entrySet().iterator();
        boolean changed = false;
        while (it.hasNext()) {
            java.util.Map.Entry<Long, Integer> e = it.next();
            BlockPos p = BlockPos.fromLong(e.getKey());
            int ci = mem.indexOfChest(p.getX(), p.getY(), p.getZ());
            if (ci < 0 || mem.chests.get(ci).seen) {
                // Gone, or a farm chest after all.
                it.remove();
                continue;
            }
            double dx = p.getX() + 0.5 - f.x, dz = p.getZ() + 0.5 - f.z;
            if (dx * dx + dz * dz < 10.0 * 10.0 && Math.abs(p.getY() - f.y) < 8.0) {
                e.setValue(e.getValue() + 1);
                if (e.getValue() > 60 && f.tick - mem.holoWorkAt > 6000) {
                    // No farm hologram read anywhere for five minutes: the server may not be showing them,
                    // so not taken off for good (a wave from it says it is one); other chests for a minute.
                    mem.chests.get(ci).skipUntil = f.tick + 1200;
                    e.setValue(0);
                } else if (e.getValue() > 60) {
                    notFarm.add(e.getKey());
                    mem.chests.remove(ci);
                    it.remove();
                    changed = true;
                    log("CHESTAI cofre sin holograma de oleada, descartado: " + p.getX() + " " + p.getY() + " " + p.getZ());
                }
            }
        }
        if (changed) {
            saveNotFarm();
            saved = -1;
        }
    }

    private void loadChests() {
        if (!chestFile.isFile()) {
            return;
        }
        try {
            BufferedReader r = new BufferedReader(new FileReader(chestFile));
            try {
                String line;
                long nowMs = System.currentTimeMillis();
                while ((line = r.readLine()) != null) {
                    String[] t = line.trim().split("\\s+");
                    if (t.length >= 3) {
                        // Known by its block as the chests are now (a double chest by its west or north half).
                        BlockPos p = Reader.chestAnchor(world, new BlockPos(Integer.parseInt(t[0]), Integer.parseInt(t[1]), Integer.parseInt(t[2])));
                        if (mem.indexOfChest(p.getX(), p.getY(), p.getZ()) >= 0) {
                            continue;
                        }
                        int ci = mem.chest(p.getX(), p.getY(), p.getZ());
                        if (t.length >= 6) {
                            // What its hologram said last, and when: a wave timer still running counts on.
                            int state = Integer.parseInt(t[3]), secs = Integer.parseInt(t[4]);
                            double ago = (nowMs - Long.parseLong(t[5])) / 1000.0;
                            Memory.Known k = mem.chests.get(ci);
                            if (state == chestai.core.Frame.COOLDOWN && ago >= 0 && secs > ago) {
                                k.seen = true;
                                k.state = state;
                                k.secs = (int) Math.ceil(secs - ago);
                                k.guessed = true;
                                k.tick = 0;
                            }
                        }
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
        try {
            dir.mkdirs();
            PrintWriter w = new PrintWriter(new FileWriter(chestFile));
            long nowMs = System.currentTimeMillis();
            try {
                for (Memory.Known k : mem.chests) {
                    if (candidates.containsKey(blockKey(k.x, k.y, k.z))) {
                        // Not known to be a farm chest yet.
                        continue;
                    }
                    if (k.seen) {
                        // Where it is, what its hologram said last (state, seconds to the wave) and when, in ms.
                        int secs = k.state == chestai.core.Frame.COOLDOWN ? (int) Math.round(mem.estSecs(mem.chests.indexOf(k), tick)) : k.secs;
                        w.println(k.x + " " + k.y + " " + k.z + " " + k.state + " " + secs + " " + nowMs);
                    } else {
                        w.println(k.x + " " + k.y + " " + k.z);
                    }
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
        if (SURVIVE) {
            // A chest the player died at a little while ago (its wave too much for it): left aside
            // for DEATH_SKIP_MS from the death, not walked back into at once after the respawn.
            long now = System.currentTimeMillis();
            for (double[] d : DEATHS) {
                long left = DEATH_SKIP_MS - (now - (long) d[3]);
                if (left <= 0) {
                    continue;
                }
                for (Memory.Known k : mem.chests) {
                    if (Math.hypot(k.cx() - d[0], k.cz() - d[2]) < 14.0 && Math.abs(k.y - d[1]) < 8.0) {
                        k.skipUntil = Math.max(k.skipUntil, left / 50);
                    }
                }
            }
        }
        saved = mem.chests.size();
        candidates.clear();
        notFarm.clear();
        notFarmFile = new File(dir, "nocofres-" + serverName() + ".txt");
        loadNotFarm();
        map = s.map ? new MapStore(new File(dir, "mapa-" + serverName())) : null;
        planner = new Planner(world, map, reader);
        double cps = s.cpsMin + draws.nextDouble() * (s.cpsMax - s.cpsMin);
        try {
            combat = new VapeCombat(mc, cps, dir, reader);
            combat.open(this::allowNativeAttack);
        } catch (ClassNotFoundException e) {
            // Without Minecadia at all the agent goes on without fighting; any other class missing is an error.
            if (!"net.minecadia.client.LiquidBounce".equals(e.getMessage())) {
                return combatError(e);
            }
        } catch (NoSuchFieldException e) {
            // A SilentAim without Chest AI's hooks (or values), from another version.
            return combat == null ? "Chest AI: instal\u00e1 los dos JAR de SilentAim de la misma versi\u00f3n." : combatError(e);
        } catch (ReflectiveOperationException | IOException | RuntimeException | LinkageError e) {
            return combatError(e);
        }
        pilot = new Pilot(cps);
        hands = new Hands(mc);
        refill = new Refill(mc, cps);
        // The server's answers to inventory clicks, for the refill (as the PvP AI's Wire).
        if (!Wire.install(mc, mc.getNetHandler().getNetworkManager(), refill.tap)) {
            log("CHESTAI no pude enganchar la conexion: el refill no espera la confirmacion del servidor");
        }
        coach = new Coach(s, planner);
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
        editingSilentAim = false;
        mouse = mc.mouseHelper;
        mc.mouseHelper = combatMouse = new CombatMouse();
        String fighting = combat == null ? "Combate deshabilitado: falta Minecadia SilentAim." : "Combate con SilentAim.";
        return String.format(Locale.ROOT, "Chest AI prendido. %d cofres conocidos en este servidor. Ping %.0f ms%s, %.1f clics por segundo. Barra: %s. %s",
            mem.chests.size(), ping, guessed ? " (supuesto)" : "", cps, kit(lay), fighting);
    }

    /** SilentAim could not be set up: everything of it put back, and the line for the player. */
    private String combatError(Throwable t) {
        if (combat != null) {
            combat.close();
            combat = null;
        }
        t.printStackTrace();
        return "Chest AI: no se pudo iniciar Minecadia SilentAim. " + t;
    }

    private static String kit(Layout l) {
        String[] names = {"arma", "arco", "manzanas", "perlas", "pociones"};
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
            if (refill != null) {
                refill.reset();
                if (mc.getNetHandler() != null) {
                    Wire.remove(mc.getNetHandler().getNetworkManager());
                }
            }
            hands.release();
        } catch (RuntimeException e) {
            // Nothing left to let go.
        }
        if (mc.mouseHelper == combatMouse) {
            mc.mouseHelper = mouse;
        }
        if (combat != null) {
            combat.close();
            combat = null;
        }
        saveChests();
        if (map != null) {
            map.save();
        }
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
            if (SURVIVE && me != null) {
                // Where it died: the chests about there are left aside a while after the respawn.
                DEATHS.add(new double[] {me.posX, me.posY, me.posZ, System.currentTimeMillis()});
                if (DEATHS.size() > 16) {
                    DEATHS.remove(0);
                }
            }
            return "Chest AI apagado: el jugador murio.";
        }
        if (mc.currentScreen != null && !(mc.currentScreen instanceof GuiContainer) && !(mc.currentScreen instanceof GuiSilentAim)) {
            return "Chest AI apagado: se abrio otra pantalla.";
        }
        return null;
    }

    /** At the start of a client tick. A line for the player when the agent leaves the keys, else null. */
    public String tickStart() throws ReflectiveOperationException, IOException {
        if (pilot == null) {
            return null;
        }
        String why = check();
        if (why != null) {
            stop();
            return why;
        }
        // The SilentAim settings open: the keys and SilentAim let go until they are shut,
        // then SilentAim takes what was set.
        if (mc.currentScreen instanceof GuiSilentAim) {
            if (!editingSilentAim) {
                hands.release();
                combatMouse.enabled = false;
                if (combat != null) {
                    combat.suspend();
                }
                editingSilentAim = true;
            }
            return null;
        }
        if (mc.isGamePaused()) {
            return null;
        }
        if (editingSilentAim) {
            if (combat != null) {
                combat.reloadProfile();
            }
            editingSilentAim = false;
        }
        // A chest window opened by a claim: shut it, the reward is in.
        if (mc.currentScreen instanceof GuiContainer && !(mc.currentScreen instanceof net.minecraft.client.gui.inventory.GuiInventory)) {
            me.closeScreen();
        }
        tick++;
        queuedTarget = null;
        queuedAttackConsumed = false;
        observeHit();
        Layout lay = Layout.of(me.inventory);
        int pearlLeft = (int) Math.max(0, pearlUntil - tick);
        Frame f = reader.read(mc, mem, pilot, lay, settings, tick, ping, pearlLeft);
        mostMobs = Math.max(mostMobs, f.mobs.size());
        if (settings.search && (tick == 1 || tick % 40 == 0)) {
            searchChests(f);
        }
        mem.bosses = settings.bosses;
        mem.update(f, first);
        first = false;
        for (String bl : mem.notes) {
            log("CHESTAI t=" + tick + " " + bl);
        }
        mem.notes.clear();
        // Raw hologram texts (and what they were read as), each time one changes.
        for (String hl : reader.holoLog) {
            log("CHESTAI t=" + tick + " " + hl);
        }
        reader.holoLog.clear();
        // Potions (and apples) down from the inventory to the hotbar: nothing else meanwhile.
        double mobD = 1e9;
        for (Frame.Mob m : f.mobs) {
            mobD = Math.min(mobD, Math.hypot(m.x - f.x, m.z - f.z));
        }
        boolean ready = lay.real[Pilot.SWORD] == me.inventory.currentItem && !me.isUsingItem()
            && coach.heal == Coach.HEAL_NONE && coach.mode != Coach.ESCAPE;
        if (refill.tick(settings.potsHotbar, mobD < 4.0, ready)) {
            String said = refill.note;
            refill.note = null;
            if (refill.event != null) {
                log("CHESTAI t=" + tick + " " + refill.event);
                refill.event = null;
            }
            // Not hands.release(): letting go of every key here would also take back the inventory
            // key the refill pressed this tick (the game reads it later in the tick). Refill lets
            // go of the keys itself, before pressing it.
            hands.forgetUse();
            combatMouse.enabled = false;
            if (combat != null) {
                combat.suspend();
            }
            return said;
        }
        if (map != null && tick % 2 == 0) {
            map.tick(world, f.x, f.z, Math.max(2, Math.min(mc.gameSettings.renderDistanceChunks, 16)), tick);
        }
        // The chest the crosshair is on by itself, not its index: a chest taken off moves the others'.
        Memory.Known picked = f.pickChest >= 0 ? mem.chests.get(f.pickChest) : null;
        checkCandidates(f);
        f.pickChest = mem.chests.indexOf(picked);
        String note = null;
        boolean noFarm = true;
        for (Memory.Known k : mem.chests) {
            noFarm &= !k.seen;
        }
        if (noFarm && (tick == 1 || tick % 600 == 0)) {
            note = (mem.chests.isEmpty() ? "Chest AI: no veo ningun cofre de farmeo, salgo a buscar." : "Chest AI: no vi hologramas de oleada todavia, reviso los cofres cercanos.")
                + Reader.diag(mc, settings) + "\nSi el texto del holograma es otro, ponelo en texto_claim de ajustes.txt.";
        }
        int[] a;
        if (mem.chests.isEmpty()) {
            a = new int[] {1, 1, 0, 0, 7, 5, 0, 0, 0};
        } else {
            float[] o = obs.build(f, mem, new WorldGrid(world));
            a = net.act(o, draws, !settings.draw);
        }
        int[] fromNet = a.clone();
        coach.before(f, mem, pilot, a, lay);
        if (coach.event != null) {
            // What the coach decided about waves of several chests (multi_cofre).
            log("CHESTAI t=" + tick + " " + coach.event);
            coach.event = null;
        }
        if (coach.outOfHeals() != saidOut) {
            saidOut = coach.outOfHeals();
            String o = saidOut ? "Chest AI: me quedan pocas manzanas doradas y pociones de vida. No empiezo oleadas nuevas para no morir (sin_curas=seguir en ajustes.txt para seguir igual)."
                : "Chest AI: tengo curas de nuevo, sigo farmeando.";
            note = note == null ? o : note + "\n" + o;
        }
        // No click of the agent's own: SilentAim hits.
        a[6] = 0;
        if ((DEBUG || settings.log) && tick % 10 == 0) {
            StringBuilder b = new StringBuilder();
            b.append(String.format(java.util.Locale.ROOT, "CHESTAI t=%d pos=%.1f,%.1f,%.1f yaw=%.0f pitch=%.0f hp=%.1f abs=%.1f mobs=%d holos=%d known=%d posibles=%d mapa=%d modo=%s |",
                tick, f.x, f.y, f.z, f.yaw, f.pitch, f.hp, f.absorption, f.mobs.size(), f.holos.size(), mem.chests.size(), candidates.size(),
                map == null ? 0 : map.scanned(), coach.describe()));
            for (int i = 0; i < mem.chests.size(); i++) {
                chestai.core.Memory.Known k = mem.chests.get(i);
                int secs = k.state == Frame.COOLDOWN ? (int) Math.ceil(mem.estSecs(i, f.tick)) : k.secs;
                b.append(String.format(java.util.Locale.ROOT, " (%d,%d,%d s%d %ds)", k.x, k.y, k.z, k.state, secs));
                if (k.contestedWave || k.reserved) {
                    b.append(" ocupado");
                }
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
        // A chest another player has (their wave, or they stand by it) is not clicked.
        Memory.Known pickedChest = f.pickChest >= 0 && f.pickChest < mem.chests.size() ? mem.chests.get(f.pickChest) : null;
        boolean claimable = pickedChest != null && !pickedChest.contestedWave && !pickedChest.reserved;
        if (!claimable && f.pickChest >= 0 && coach.mode == Coach.CLAIM && coach.heal == Coach.HEAL_NONE) {
            a[7] = 0;
        }
        Pilot.Keys k = lastKeys = pilot.step(a, pearls, pearlLeft, potions, lay.real[Pilot.BOW] >= 0 ? lay.arrows : 0, lay.real[Pilot.GAPPLE] >= 0 ? lay.gapples : 0, f.pickMob >= 0, claimable);
        coach.after(f, k);
        if (k.claim && coach.mode == Coach.CLAIM) {
            Memory.Known c = mem.chests.get(f.pickChest);
            c.claimAttempts++;
            c.lastClaimAttempt = tick;
        }
        if (k.throwIt && k.slot == Pilot.PEARL) {
            pearlUntil = tick + (long) Math.round(settings.pearlCooldownS * 20.0);
        }
        if ((DEBUG || settings.log) && tick % 100 == 0) {
            log("CHESTAI clics=" + dbgAttacks + " ataques_cliente=" + dbgOnMob + " impactos_observados=" + blowsLanded);
        }
        // SilentAim on the agent's mob; while it fights, a turn of the player's own mouse in
        // the last 8 ticks leaves the view to the player, and the agent only walks.
        combatMouse.enabled = combat != null && combat.prepare(coach.silentTarget(k), k, lay);
        if (combatMouse.enabled && tick - combatMouse.lastManualTurn <= 8) {
            k.dyaw = k.dpitch = 0;
            coach.steerWhileLooking(f, k);
        }
        coach.limitCombatMovement(f, k);
        hands.press(k, lay);
        if (tick % 200 == 0) {
            saveChests();
        }
        if (map != null && tick % 600 == 0) {
            map.save();
        }
        return note;
    }

    /**
     * Late in the tick's start (after SilentAim has turned): the blow of the
     * tick through SilentAim, if it can land.
     */
    public void queueCombatClick() throws ReflectiveOperationException {
        if (combat == null && NO_AIM_CLICKS && pilot != null && lastKeys != null && mc.currentScreen == null && !mc.isGamePaused() && check() == null) {
            ownClick();
            return;
        }
        if (combat == null || mc.currentScreen != null || mc.isGamePaused() || check() != null) {
            return;
        }
        queuedTarget = combat.queueAttack(tick);
        if (queuedTarget != null) {
            dbgAttacks++;
        }
    }

    /**
     * Without SilentAim, in the arena and the self test only (-Dchestai.arena or -Dchestai.selftest,
     * where Minecadia is not there): the agent's own click, as SilentAim would give it, on the mob
     * Coach fights when the crosshair is on it within 3 blocks, at the clicks per second.
     */
    private static final boolean NO_AIM_CLICKS = Boolean.getBoolean("chestai.arena") || Boolean.getBoolean("chestai.selftest");
    private Pilot.Keys lastKeys;
    private long lastOwnClick = -1000;

    private void ownClick() {
        int id = coach.silentTarget(lastKeys);
        MovingObjectPosition mo = mc.objectMouseOver;
        if (id < 0 || me.isUsingItem() || tick - lastOwnClick < 20.0 / pilot.cps || mo == null
                || mo.typeOfHit != MovingObjectPosition.MovingObjectType.ENTITY || mo.entityHit == null || mo.hitVec == null) {
            return;
        }
        Entity e = mo.entityHit;
        boolean on = e.getEntityId() == id || (e.ridingEntity != null && e.ridingEntity.getEntityId() == id)
            || (e.riddenByEntity != null && e.riddenByEntity.getEntityId() == id);
        if (!on || !(e instanceof EntityLivingBase) || mo.hitVec.distanceTo(me.getPositionEyes(1.0F)) > 3.0) {
            return;
        }
        KeyBinding.onTick(mc.gameSettings.keyBindAttack.getKeyCode());
        lastOwnClick = tick;
        queuedTarget = (EntityLivingBase) e;
        queuedAttackConsumed = false;
        dbgAttacks++;
    }

    /** SilentAim's gate: a blow only for the queued one, once, while it still lands. */
    private boolean allowNativeAttack() {
        return queuedTarget == null || (!queuedAttackConsumed && combat.allowNativeAttack(queuedTarget));
    }

    /** The client's player attacked {@code target}: the queued blow went out. */
    public void attackObserved(net.minecraft.entity.Entity player, net.minecraft.entity.Entity target) {
        if (queuedTarget == null || queuedAttackConsumed || player != me || target != queuedTarget) {
            return;
        }
        dbgOnMob++;
        if (blowTarget == null) {
            blows++;
            blowTarget = queuedTarget;
            blowHp = blowTarget.getHealth();
            blowHurtTime = blowTarget.hurtTime;
            blowAt = tick;
        }
        queuedAttackConsumed = true;
    }

    /** Whether the blow watched landed: its mob lost health or was hurt again within 12 ticks, as the server sends it. */
    private void observeHit() {
        if (blowTarget == null) {
            return;
        }
        if (world.getEntityByID(blowTarget.getEntityId()) != blowTarget) {
            blowTarget = null;
        } else if (blowTarget.getHealth() < blowHp || blowTarget.hurtTime > blowHurtTime) {
            blowsLanded++;
            blowTarget = null;
        } else if (tick - blowAt >= 12) {
            blowTarget = null;
        } else {
            blowHurtTime = blowTarget.hurtTime;
        }
    }
}
