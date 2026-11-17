package pvpai.forge;

import com.mojang.authlib.GameProfile;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.OutputStream;
import java.util.Locale;
import java.util.UUID;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityOtherPlayerMP;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.gui.GuiMainMenu;
import net.minecraft.world.EnumDifficulty;
import net.minecraft.world.WorldSettings;
import net.minecraft.world.WorldType;

/**
 * For the build, which has no player: with -Dpvpai.selftest=true the game
 * makes a flat world by itself, puts a player that stands still in it,
 * hands the keys to the policy for some seconds and writes into
 * selftest.txt whether it walked up to that player and swung at it. Then
 * the game shuts down.
 */
final class SelfTest {
    private static final int TICKS = 200, PATIENCE = 20 * 300;

    private final PvpAi mod;
    private int state, waited, ticks, swings;
    private double fromX, fromZ;
    private EntityOtherPlayerMP dummy;

    SelfTest(PvpAi mod) {
        this.mod = mod;
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
            mc.gameSettings.difficulty = EnumDifficulty.PEACEFUL;
            mc.gameSettings.renderDistanceChunks = 4;
            mc.gameSettings.limitFramerate = 30;
            WorldSettings ws = new WorldSettings(1L, WorldSettings.GameType.SURVIVAL, false, false, WorldType.FLAT);
            ws.setWorldName("3;minecraft:bedrock,2*minecraft:dirt,minecraft:grass;0;");
            mc.launchIntegratedServer("selftest" + System.currentTimeMillis(), "selftest", ws);
            state = 1;
        } else if (state == 1) {
            EntityPlayerSP me = mc.thePlayer;
            if (me == null || mc.theWorld == null || me.ticksExisted < 100 || !me.onGround) {
                return;
            }
            if (mc.currentScreen != null) {
                mc.displayGuiScreen(null);
                return;
            }
            dummy = new EntityOtherPlayerMP(mc.theWorld, new GameProfile(UUID.fromString("00000000-0000-4000-8000-000000000001"), "Dummy"));
            dummy.setPositionAndRotation(me.posX + 7.0, me.posY, me.posZ + 2.0, 90.0F, 0.0F);
            mc.theWorld.addEntityToWorld(-7, dummy);
            fromX = me.posX;
            fromZ = me.posZ;
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
            if (++ticks < TICKS) {
                return;
            }
            double walked = Math.hypot(me.posX - fromX, me.posZ - fromZ), left = me.getDistanceToEntity(dummy);
            mod.flip(mc);
            boolean ok = !mod.on() && walked > 2.0 && left < 5.0 && swings > 0;
            end(mc, String.format(Locale.ROOT, "%s: %d ticks with the agent at the keys, walked %.1f blocks, ended %.1f from the other player, swung %d times",
                ok ? "ok" : "FAILED", ticks, walked, left, swings));
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
