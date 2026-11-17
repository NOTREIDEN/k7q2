package pvpai.forge;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.OutputStream;
import java.io.PrintWriter;
import java.text.SimpleDateFormat;
import java.util.Date;
import java.util.Locale;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.gui.GuiDisconnected;
import net.minecraft.client.gui.GuiGameOver;
import net.minecraft.client.gui.GuiMainMenu;
import net.minecraft.client.gui.inventory.GuiInventory;
import net.minecraft.client.multiplayer.GuiConnecting;
import net.minecraft.item.ItemEnderPearl;
import net.minecraft.item.ItemFood;
import net.minecraft.item.ItemPotion;
import net.minecraft.item.ItemStack;
import net.minecraft.item.ItemSword;

/**
 * For a test server, which has no player either: with
 * -Dpvpai.servertest=HOST:PORT the game joins that server by itself and,
 * whenever another player is there and the hotbar has a sword, hands the
 * keys to the policy for a duel. A death is followed by a respawn and the
 * next duel. After -Dpvpai.seconds (90) with the agent at the keys it
 * writes what it did into servertest.txt and the game shuts down. What the
 * server and its plugins make of a client played this way is read off the
 * server's side; ticks.txt has the state of the player on each tick with the
 * agent at the keys, by the clock, to set beside what the server said.
 */
final class ServerTest {
    private static final int PATIENCE = 20 * 600;

    private final PvpAi mod;
    private final String host;
    private final int port, ticksWanted;
    private int state, waited, ticks, duels, swings, deaths, settled;
    /** What the agent used up and how often it went to its inventory, and the counts and the screen of the tick before. */
    private int potions, meals, pearls, visits;
    private int[] had;
    private boolean wasOpen;
    private PrintWriter trace;
    private final SimpleDateFormat clock = new SimpleDateFormat("HH:mm:ss.SSS", Locale.ROOT);

    ServerTest(PvpAi mod, String address) {
        this.mod = mod;
        int colon = address.lastIndexOf(':');
        this.host = colon < 0 ? address : address.substring(0, colon);
        this.port = colon < 0 ? 25565 : Integer.parseInt(address.substring(colon + 1));
        this.ticksWanted = 20 * Integer.getInteger("pvpai.seconds", 90);
    }

    /** At the start of each tick of the client, before the mod's own part. */
    void step(Minecraft mc) throws IOException {
        if (state == 3) {
            return;
        }
        if (++waited > PATIENCE) {
            end(mc, "FAILED: still in state " + state + " after " + waited + " ticks, screen " + mc.currentScreen);
            return;
        }
        if (state == 0) {
            if (!(mc.currentScreen instanceof GuiMainMenu)) {
                return;
            }
            mc.gameSettings.pauseOnLostFocus = false;
            mc.gameSettings.renderDistanceChunks = 4;
            mc.gameSettings.limitFramerate = 30;
            mc.displayGuiScreen(new GuiConnecting(new GuiMainMenu(), mc, host, port));
            state = 1;
            return;
        }
        if (mc.currentScreen instanceof GuiDisconnected) {
            end(mc, "FAILED: not on the server any more after " + ticks + " ticks with the agent at the keys");
            return;
        }
        EntityPlayerSP me = mc.thePlayer;
        if (me == null || mc.theWorld == null) {
            return;
        }
        if (state == 1) {
            if (mc.currentScreen instanceof GuiGameOver || me.getHealth() <= 0.0F) {
                me.respawnPlayer();
                mc.displayGuiScreen(null);
                settled = 0;
                return;
            }
            boolean sword = false;
            for (int i = 0; i < 9; i++) {
                ItemStack s = me.inventory.mainInventory[i];
                sword |= s != null && s.getItem() instanceof ItemSword;
            }
            // A second or two on the ground with the kit and somebody to fight.
            if (mc.currentScreen != null || !sword || !me.onGround || mc.theWorld.playerEntities.size() < 2) {
                settled = 0;
                return;
            }
            if (++settled < 40) {
                return;
            }
            mod.flip(mc);
            if (mod.on()) {
                duels++;
                state = 2;
                had = null;
                wasOpen = false;
            }
            settled = 0;
            return;
        }
        if (!mod.on()) {
            // The duel is over: one of the two died or left.
            deaths += me.getHealth() <= 0.0F ? 1 : 0;
            state = 1;
            settled = 0;
        } else {
            swings += me.isSwingInProgress && me.swingProgressInt == 0 ? 1 : 0;
            ticks++;
            trace(mc, me);
            int[] has = new int[3];
            for (ItemStack s : me.inventory.mainInventory) {
                if (s != null) {
                    int kind = s.getItem() instanceof ItemPotion ? 0 : s.getItem() instanceof ItemFood ? 1 : s.getItem() instanceof ItemEnderPearl ? 2 : -1;
                    if (kind >= 0) {
                        has[kind] += s.stackSize;
                    }
                }
            }
            // One less than a tick ago was used; a death or a new kit changes more than that.
            if (had != null && me.getHealth() > 0.0F) {
                potions += had[0] - has[0] == 1 ? 1 : 0;
                meals += had[1] - has[1] == 1 ? 1 : 0;
                pearls += had[2] - has[2] == 1 ? 1 : 0;
            }
            had = has;
            boolean open = mc.currentScreen instanceof GuiInventory;
            visits += open && !wasOpen ? 1 : 0;
            wasOpen = open;
        }
        if (ticks >= ticksWanted) {
            if (mod.on()) {
                mod.flip(mc);
            }
            end(mc, String.format(Locale.ROOT, "%s: %d ticks with the agent at the keys in %d duels, swung %d times, died %d times, used %d potions, %d meals and %d pearls, went to its inventory %d times",
                swings > 0 ? "ok" : "FAILED", ticks, duels, swings, deaths, potions, meals, pearls, visits));
        }
    }

    /** The player as it is at the start of this tick, after the last one. */
    private void trace(Minecraft mc, EntityPlayerSP me) throws IOException {
        if (trace == null) {
            trace = new PrintWriter(new File(mc.mcDataDir, "ticks.txt"), "UTF-8");
        }
        ItemStack held = me.inventory.getCurrentItem(), using = me.getItemInUse();
        trace.println(String.format(Locale.ROOT,
            "%s using %s %d held %d %s sprint %b fwd %.2f strafe %.2f ground %b speed %.3f hurt %d useKey %b screen %s health %.1f",
            clock.format(new Date()), using == null ? "-" : using.getUnlocalizedName(), me.getItemInUseCount(),
            me.inventory.currentItem, held == null ? "-" : held.getUnlocalizedName(), me.isSprinting(),
            me.movementInput.moveForward, me.movementInput.moveStrafe, me.onGround, Math.hypot(me.motionX, me.motionZ),
            me.hurtTime, mc.gameSettings.keyBindUseItem.isKeyDown(),
            mc.currentScreen == null ? "-" : mc.currentScreen.getClass().getSimpleName(), me.getHealth()));
    }

    private void end(Minecraft mc, String line) throws IOException {
        state = 3;
        if (trace != null) {
            trace.close();
        }
        System.out.println("SERVERTEST " + line);
        OutputStream out = new FileOutputStream(new File(mc.mcDataDir, "servertest.txt"));
        try {
            out.write((line + "\n").getBytes("UTF-8"));
        } finally {
            out.close();
        }
        mc.shutdown();
    }
}
