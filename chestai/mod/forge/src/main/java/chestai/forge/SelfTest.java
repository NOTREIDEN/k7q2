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
    private static final int TICKS = 900, PATIENCE = 20 * 300;

    private final ChestAi mod;
    private int state, waited, ticks, swings;
    private double fromX, fromZ, far;
    private int potsAt, applesAt, cx, cy, cz;
    private float lowest = 99f;
    private boolean hurt;

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
            // The chest behind a wall: the way there goes round it.
            int x = (int) Math.floor(me.posX) + 14, y = (int) Math.floor(me.posY), z = (int) Math.floor(me.posZ) + 2;
            cx = x;
            cy = y;
            cz = z;
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
            int wx = (int) Math.floor(me.posX) + 5, wz = (int) Math.floor(me.posZ);
            run(s, "fill " + wx + " " + y + " " + (wz - 5) + " " + (wx + 1) + " " + (y + 2) + " " + (wz + 5) + " stone");
            run(s, "setblock " + x + " " + y + " " + z + " chest");
            run(s, "summon ArmorStand " + (x + 0.5) + " " + (y + 1.2) + " " + (z + 0.5) + " {CustomName:\"§cKill the mobs!\",CustomNameVisible:1,Invisible:1,NoGravity:1}");
            for (int i = 0; i < 4; i++) {
                run(s, "summon Zombie " + (x + 2 + i) + " " + y + " " + (z - 2 + i) + " {Attributes:[{Name:generic.maxHealth,Base:30},{Name:generic.followRange,Base:6}],Health:30}");
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
            EntityPlayerSP me = mc.thePlayer;
            potsAt = count(me, "potion");
            applesAt = count(me, "golden_apple");
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
            if (ticks >= 300 && ticks < 700 && !hurt && me.getHealth() >= 16.0F) {
                // Low on health all at once, with no apple's absorption left: a potion has to come.
                MinecraftServer s = mc.getIntegratedServer();
                run(s, "effect " + me.getName() + " clear");
                run(s, "effect " + me.getName() + " 1 600 2");
                run(s, "effect " + me.getName() + " 7 1 1");
                hurt = true;
            }
            if (ticks > 300) {
                lowest = Math.min(lowest, me.getHealth());
            }
            if (++ticks < TICKS) {
                return;
            }
            int chests = mod.session().chestsKnown(), mobs = mod.session().mostMobs();
            int pots = potsAt - count(me, "potion"), apples = applesAt - count(me, "golden_apple");
            double toChest = Math.hypot(me.posX - (cx + 0.5), me.posZ - (cz + 0.5));
            int alive = 0;
            for (Object o : mc.getIntegratedServer().worldServerForDimension(0).loadedEntityList) {
                if (o instanceof net.minecraft.entity.monster.EntityZombie && ((net.minecraft.entity.EntityLivingBase) o).isEntityAlive()) {
                    alive++;
                }
            }
            mod.flip(mc);
            boolean ok = !mod.on() && chests >= 1 && mobs >= 3 && far > 6.0 && swings > 0 && (pots >= 1 || lowest > 10.0F) && apples <= 3 && me.getHealth() > 0;
            end(mc, String.format(Locale.ROOT, "%s: %d ticks with the agent at the keys, %d chest(s) known, up to %d mobs seen, got %.1f blocks away from the start (%.1f from the chest behind the wall at the end), swung %d times, %d of 4 zombies left, %d potion(s) and %d apple(s) used, lowest health %.1f",
                ok ? "ok" : "FAILED", ticks, chests, mobs, far, toChest, swings, alive, pots, apples, lowest));
        }
    }

    private static int count(EntityPlayerSP me, String item) {
        int n = 0;
        for (net.minecraft.item.ItemStack st : me.inventory.mainInventory) {
            if (st != null && net.minecraft.item.Item.itemRegistry.getNameForObject(st.getItem()).toString().equals("minecraft:" + item)) {
                n += st.stackSize;
            }
        }
        return n;
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
