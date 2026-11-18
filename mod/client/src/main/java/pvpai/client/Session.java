package pvpai.client;

import java.util.Locale;
import java.util.Random;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityOtherPlayerMP;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.gui.inventory.GuiInventory;
import net.minecraft.client.multiplayer.WorldClient;
import net.minecraft.network.NetworkManager;
import net.minecraft.network.Packet;
import net.minecraft.network.play.server.S32PacketConfirmTransaction;
import net.minecraft.util.MouseHelper;
import pvpai.core.Duel;
import pvpai.core.Inv;
import pvpai.core.Policy;

/**
 * A policy at the keys of the game from the moment the player hands them
 * over until the player takes them back or the duel is over. What a mod
 * loader has to do is call {@link #tickStart} and {@link #tickEnd} around
 * each tick of the client and show the player the lines it gets back.
 */
public final class Session implements Wire.Tap {
    /** Seconds of Speed left at which the agents of training take a drink. */
    private static final double DRINK_WINDOW_S = 30.0;
    /** What stands in for a ping the player list does not say. */
    private static final double PING = 200.0, OTHER_PING = 60.0;

    /** A mouse that does not turn the view: while the agent aims, the player's hand must not. */
    private static final class Still extends MouseHelper {
        @Override
        public void mouseXYChange() {
            super.mouseXYChange();
            deltaX = 0;
            deltaY = 0;
        }
    }

    private final Minecraft mc;
    private final Policy policy;
    private final Random draws = new Random();
    private final InventoryRefill refill = new InventoryRefill();
    private Targets targets = new Targets();
    private int switches;
    private Bot bot;
    private Duel duel;
    private EntityPlayerSP me;
    private WorldClient world;
    private NetworkManager manager;
    private MouseHelper mouse;
    /** Whether the tick that runs now started with the agent at the keys. */
    private boolean started;
    private boolean openingInventory;
    private GuiInventory ownedInventory;

    public Session(Minecraft mc, Policy policy) {
        this.mc = mc;
        this.policy = policy;
    }

    /** Whether the agent is at the keys. */
    public boolean on() {
        return bot != null;
    }

    public Bot bot() {
        return bot;
    }

    public Duel duel() {
        return duel;
    }

    /** The players about as the choice of opponent sees them, for a log. */
    public String targets() {
        return targets.describe(mc);
    }

    /** How many times it went over to another opponent in this duel. */
    public int switches() {
        return switches;
    }

    /**
     * Put the agent at the keys for a duel against the player it picks
     * ({@link Targets}): the nearest, or the one in front of it.
     * Returns a line for the player; {@link #on} says whether it worked.
     */
    public String start(Settings s) {
        stop();
        if (mc.thePlayer == null || mc.theWorld == null || mc.getNetHandler() == null) {
            return "PvP AI: no hay partida.";
        }
        targets = new Targets();
        switches = 0;
        EntityOtherPlayerMP other = targets.choose(mc, null);
        if (other == null) {
            return "PvP AI: no hay ningun rival a la vista.";
        }
        Duel d = new Duel();
        double[] arena = s.arenaSet ? new double[] {s.arenaX, s.arenaZ, s.arenaHalf} : Scout.arena(mc, other);
        d.centerX = arena[0];
        d.centerZ = arena[1];
        d.arenaHalf = arena[2];
        d.floorY = Scout.floor(mc);
        d.realHealth = true;
        d.hitHealth = Scout.hitHealth(mc.thePlayer, other, Layout.of(mc.thePlayer.inventory));
        d.pearlCooldown = (int) Math.floor(s.pearlCooldownS * 20.0 + 0.5);
        d.drinkWindowS = DRINK_WINDOW_S;
        double ping = s.ping >= 0.0 ? s.ping : Scout.ping(mc, mc.thePlayer);
        double otherPing = s.otherPing >= 0.0 ? s.otherPing : Scout.ping(mc, other);
        boolean guessed = ping < 0.0;
        ping = ping < 0.0 ? PING : ping;
        otherPing = otherPing < 0.0 ? OTHER_PING : otherPing;
        double cps = s.cpsMin + draws.nextDouble() * (s.cpsMax - s.cpsMin);

        policy.reset();
        bot = new Bot(mc, other, d, ping, otherPing, cps, new Bot.Chooser() {
            public int[] choose(float[] obs) {
                return policy.act(obs, draws);
            }
        });
        bot.block(s.block);
        bot.release();
        // The agent starts a duel with its sword in hand. If the player holds
        // something else, the sword is taken up as the wheel of the mouse would.
        int swordAt = bot.layout().real[Inv.SLOT_SWORD];
        if (bot.layout().sword && mc.thePlayer.inventory.currentItem != swordAt) {
            mc.thePlayer.inventory.currentItem = swordAt;
        }
        duel = d;
        me = mc.thePlayer;
        world = mc.theWorld;
        manager = mc.getNetHandler().getNetworkManager();
        Wire.install(mc, manager, this);
        mouse = mc.mouseHelper;
        mc.mouseHelper = new Still();
        started = false;

        String line = String.format(Locale.ROOT,
            "PvP AI prendido contra %s. Arena de %.0f x %.0f con centro en %.1f, %.1f y piso en %.1f. Golpe de %.1f de vida. Ping %.0f%s y %.0f ms. %.1f clics por segundo.",
            other.getName(), 2.0 * d.arenaHalf, 2.0 * d.arenaHalf, d.centerX, d.centerZ, d.floorY, d.hitHealth, ping, guessed ? " (supuesto)" : "", otherPing, cps);
        line += " " + kit(bot.layout());
        if (!s.block) {
            line += " Sin bloquear con la espada.";
        }
        return line;
    }

    /** What the agent found in the hotbar and the inventory, for the player to check. */
    private String kit(Layout l) {
        if (!l.sword) {
            return "Ojo: no hay espada en la barra.";
        }
        int[] bar = new int[8], rows = new int[8];
        for (int i = 0; i < 36; i++) {
            (i < 9 ? bar : rows)[Reader.item(me.inventory.mainInventory[i])]++;
        }
        int pearl = l.shown(me.inventory, Inv.SLOT_PEARL, Inv.PEARL), food = l.shown(me.inventory, Inv.SLOT_FOOD, Inv.FOOD);
        return String.format(Locale.ROOT, "Barra: espada en %d, %s, %s, %d pociones de vida y %d de Speed. Inventario: %d de vida y %d de Speed.",
            l.real[Inv.SLOT_SWORD] + 1, pearl > 0 ? "perlas en " + pearl : "sin perlas", food > 0 ? "comida en " + food : "sin comida",
            bar[Inv.POTION], bar[Inv.DRINK], rows[Inv.POTION], rows[Inv.DRINK]);
    }

    /** Take the agent off the keys. */
    public void stop() {
        if (bot == null) {
            return;
        }
        try {
            bot.release();
            Wire.remove(manager);
        } catch (RuntimeException e) {
            // A connection that is gone has nothing left to take off.
        }
        if (mc.mouseHelper instanceof Still) {
            mc.mouseHelper = mouse;
        }
        boolean closeInventory = mc.currentScreen instanceof GuiInventory
            && (ownedInventory == mc.currentScreen || openingInventory)
            && mc.thePlayer == me && mc.theWorld == world && mc.getNetHandler() != null;
        bot = null;
        started = false;
        openingInventory = false;
        ownedInventory = null;
        refill.reset();
        if (closeInventory) {
            me.closeScreen();
        }
    }

    @Override
    public void before(Packet<?> packet) {
        if (bot != null) {
            bot.before(packet);
        }
    }

    @Override
    public void after(Packet<?> packet) {
        if (bot != null) {
            bot.after(packet);
            if (packet instanceof S32PacketConfirmTransaction) {
                refill.confirmed((S32PacketConfirmTransaction) packet);
            }
        }
    }

    /** Why the agent cannot go on, or null if it can. */
    public String check() {
        if (bot == null) {
            return null;
        }
        if (mc.thePlayer != me || mc.theWorld != world || mc.getNetHandler() == null) {
            return "PvP AI apagado: cambio el mundo.";
        }
        if (me.isDead || me.getHealth() <= 0.0F) {
            return "PvP AI apagado: el jugador murio.";
        }
        if (!bot.alive() || bot.other().getHealth() <= 0.0F) {
            return "PvP AI apagado: el rival ya no esta.";
        }
        if (mc.currentScreen != null && !(mc.currentScreen instanceof GuiInventory)) {
            return "PvP AI apagado: se abrio otra pantalla.";
        }
        return null;
    }

    /**
     * At the start of a tick of the client. Returns a line for the player
     * if the agent left the keys, else null.
     */
    public String tickStart() {
        started = false;
        openingInventory = false;
        if (bot != null && mc.thePlayer == me && mc.theWorld == world && mc.getNetHandler() != null && !mc.isGamePaused()) {
            targets.tick(mc);
            EntityOtherPlayerMP next = targets.choose(mc, bot.other());
            if (next != null && next != bot.other()) {
                bot.target(next);
                switches++;
            }
        }
        String why = check();
        if (why != null) {
            stop();
            return why;
        }
        if (bot != null && !mc.isGamePaused()) {
            if (ownedInventory != mc.currentScreen) {
                ownedInventory = null;
            }
            InventoryRefill.Step step = refill.step(mc.currentScreen, me);
            switch (step) {
                case WAIT:
                case CLOSE:
                    bot.waitForInventory();
                    started = true;
                    if (step == InventoryRefill.Step.CLOSE) {
                        me.closeScreen();
                    }
                    return null;
                case REJECTED:
                case TIMED_OUT:
                    // The game takes this in stride: the client answers the
                    // refusal and the server sends the inventory again. The
                    // screen shuts and the agent plays on.
                    bot.waitForInventory();
                    started = true;
                    me.closeScreen();
                    return step == InventoryRefill.Step.REJECTED
                        ? "El servidor rechazo un clic de inventario: cierro el inventario y sigo."
                        : "No llego la confirmacion de un clic de inventario: cierro el inventario y sigo.";
                default:
                    break;
            }
            // In 1.8.9 the targeting ray reads the head yaw. The mouse
            // changes the camera yaw before vanilla updates the head.
            me.setRotationYawHead(me.rotationYaw);
            boolean wasClosed = mc.currentScreen == null;
            int inventory = bot.tickStartForSession();
            me.setRotationYawHead(me.rotationYaw);
            openingInventory = wasClosed && inventory == 1;
            refill.sent(mc.currentScreen, me.openContainer, inventory >= Inv.SHUT + Inv.WINDOW_ROWS);
            started = true;
        }
        return null;
    }

    /** At the end of a tick of the client. */
    public void tickEnd() {
        if (bot != null && started && mc.thePlayer == me && mc.theWorld == world) {
            if (openingInventory && mc.currentScreen instanceof GuiInventory) {
                ownedInventory = (GuiInventory) mc.currentScreen;
            }
            bot.tickEnd();
            // Keep the targeting ray in step with the interpolated camera.
            me.prevRotationYawHead = me.prevRotationYaw;
        }
        started = false;
        openingInventory = false;
    }
}
