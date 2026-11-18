package chestai.forge;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.OutputStream;
import java.util.Locale;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.gui.GuiMainMenu;
import net.minecraft.command.ICommandManager;
import net.minecraft.server.MinecraftServer;
import net.minecraft.world.EnumDifficulty;
import net.minecraft.world.WorldSettings;
import net.minecraft.world.WorldType;

/**
 * For the build, which has no player: with -Dchestai.selftest=true the
 * game makes a flat world, gives the player the kit, puts a farm chest
 * with a hologram and some zombies around it, hands the keys to the policy
 * for some seconds and writes into selftest.txt whether it ran, found the
 * chest and the mobs, moved and swung. Then the game shuts down.
 */
final class SelfTest {
    private static final int TICKS = 600, PATIENCE = 20 * 300;

    private final ChestAi mod;
    private int state, waited, ticks, swings;
    private double fromX, fromZ, far;

    SelfTest(ChestAi mod) {
        this.mod = mod;
    }

    private static void run(MinecraftServer s, String cmd) {
        ICommandManager m = s.getCommandManager();
        int r = m.executeCommand(s, cmd);
        System.out.println("SELFTEST /" + cmd + " -> " + r);
    }

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
            WorldSettings ws = new WorldSettings(1L, WorldSettings.GameType.SURVIVAL, false, false, WorldType.FLAT);
            ws.enableCommands();
            ws.setWorldName("3;minecraft:bedrock,2*minecraft:dirt,minecraft:grass;1;");
            mc.launchIntegratedServer("selftest" + System.currentTimeMillis(), "selftest", ws);
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
            MinecraftServer s = mc.getIntegratedServer();
            s.setDifficultyForAllWorlds(EnumDifficulty.EASY);
            String n = me.getName();
            int x = (int) Math.floor(me.posX) + 8, y = (int) Math.floor(me.posY), z = (int) Math.floor(me.posZ) + 3;
            run(s, "gamerule doMobSpawning false");
            run(s, "gamerule doDaylightCycle false");
            run(s, "time set 6000");
            run(s, "give " + n + " diamond_sword 1 0 {ench:[{id:16,lvl:2}]}");
            run(s, "give " + n + " bow 1 0 {ench:[{id:49,lvl:2},{id:51,lvl:1}]}");
            run(s, "give " + n + " golden_apple 16");
            run(s, "give " + n + " ender_pearl 8");
            run(s, "give " + n + " potion 3 16421");
            run(s, "give " + n + " arrow 64");
            run(s, "give " + n + " diamond_chestplate");
            run(s, "effect " + n + " 1 600 2");
            run(s, "setblock " + x + " " + y + " " + z + " chest");
            run(s, "summon ArmorStand " + (x + 0.5) + " " + (y + 1.2) + " " + (z + 0.5) + " {CustomName:\"§cKill the mobs!\",CustomNameVisible:1,Invisible:1,NoGravity:1}");
            for (int i = 0; i < 4; i++) {
                run(s, "summon Zombie " + (x + 2 + i) + " " + y + " " + (z - 2 + i) + " {Attributes:[{Name:generic.maxHealth,Base:30}],Health:30}");
            }
            run(s, "summon SnowMan " + (x - 3) + " " + y + " " + (z + 3));
            fromX = me.posX;
            fromZ = me.posZ;
            state = 4;
        } else if (state == 4) {
            // A tick for the commands' entities to reach the client.
            if (++ticks < 40) {
                return;
            }
            ticks = 0;
            mod.flip(mc);
            if (!mod.on()) {
                end(mc, "FAILED: the agent did not take the keys");
                return;
            }
            state = 2;
        } else {
            EntityPlayerSP me = mc.thePlayer;
            if (!mod.on() || me == null) {
                end(mc, "FAILED: the agent left the keys after " + ticks + " ticks");
                return;
            }
            swings += me.isSwingInProgress && me.swingProgressInt == 0 ? 1 : 0;
            far = Math.max(far, Math.hypot(me.posX - fromX, me.posZ - fromZ));
            if (++ticks < TICKS) {
                return;
            }
            int chests = mod.session().chestsKnown(), mobs = mod.session().mostMobs();
            mod.flip(mc);
            boolean ok = !mod.on() && chests >= 1 && mobs >= 3 && (far > 1.0 || swings > 0);
            end(mc, String.format(Locale.ROOT, "%s: %d ticks with the agent at the keys, %d chest(s) known, up to %d mobs seen, got %.1f blocks away from the start, swung %d times",
                ok ? "ok" : "FAILED", ticks, chests, mobs, far, swings));
        }
    }

    private void end(Minecraft mc, String line) throws IOException {
        state = 3;
        System.out.println("SELFTEST " + line);
        OutputStream out = new FileOutputStream(new File(mc.mcDataDir, "selftest.txt"));
        try {
            out.write((line + "\n").getBytes("UTF-8"));
        } finally {
            out.close();
        }
        mc.shutdown();
    }
}
