package rig;

import com.mojang.authlib.GameProfile;
import java.io.BufferedReader;
import java.io.FileReader;
import java.io.FileWriter;
import java.io.PrintWriter;
import java.lang.reflect.Field;
import java.lang.reflect.Modifier;
import java.util.ArrayList;
import java.util.Collection;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityOtherPlayerMP;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.gui.GuiMainMenu;
import net.minecraft.client.network.NetHandlerPlayClient;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.client.settings.KeyBinding;
import net.minecraft.entity.Entity;
import net.minecraft.entity.EntityLivingBase;
import net.minecraft.entity.EntityTrackerEntry;
import net.minecraft.entity.SharedMonsterAttributes;
import net.minecraft.entity.ai.attributes.AttributeModifier;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.entity.player.EntityPlayerMP;
import net.minecraft.init.Items;
import net.minecraft.item.ItemStack;
import net.minecraft.network.EnumPacketDirection;
import net.minecraft.network.INetHandler;
import net.minecraft.network.NetHandlerPlayServer;
import net.minecraft.network.NetworkManager;
import net.minecraft.network.Packet;
import net.minecraft.network.play.client.C02PacketUseEntity;
import net.minecraft.network.play.client.C03PacketPlayer;
import net.minecraft.network.play.client.C07PacketPlayerDigging;
import net.minecraft.network.play.client.C08PacketPlayerBlockPlacement;
import net.minecraft.network.play.client.C0APacketAnimation;
import net.minecraft.network.play.client.C0BPacketEntityAction;
import net.minecraft.network.play.server.S1CPacketEntityMetadata;
import net.minecraft.network.play.server.S20PacketEntityProperties;
import net.minecraft.potion.Potion;
import net.minecraft.potion.PotionEffect;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.management.ItemInWorldManager;
import net.minecraft.entity.DataWatcher;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.util.BlockPos;
import net.minecraft.util.EnumFacing;
import net.minecraft.util.MathHelper;
import net.minecraft.util.MovingObjectPosition;
import net.minecraft.util.Vec3;
import net.minecraft.world.WorldServer;
import net.minecraft.world.WorldSettings;
import net.minecraft.world.WorldType;

/**
 * Parity rig. Drives the real client with scripted inputs and a scripted
 * second player on the integrated server, and writes down, tick by tick,
 * every packet handled and sent and the players' state on both sides.
 *
 * Hooks (one line each, added to the decompiled sources):
 *   Minecraft.runGameLoop: clientPre(this) before runTick(), clientPost(this) after
 *   MinecraftServer.tick:  serverPre(this) at the top, serverPost(this) after the world update
 *   PacketThreadUtil:      onHandle(packet, handler) just before processPacket
 *   NetHandlerPlayServer.sendPacket: onServerSend(this, packet) at the top
 *   NetHandlerPlayClient.addToSendQueue: onClientSend(packet) at the top
 */
public final class Rig {
    static final String SCRIPT = System.getProperty("rig.script");
    static final String OUT = System.getProperty("rig.out", "trace.log");
    static PrintWriter out;
    static final Object lock = new Object();

    // Script.
    static final List<int[]> inputs = new ArrayList<int[]>();
    static final Map<Integer, List<String[]>> fakeActions = new HashMap<Integer, List<String[]>>();
    static double[] start = {0.5, 4.0, 0.5, 0.0, 0.0};
    static double[] fakeStart = null;
    static int settle = 100;
    static double penLo = -1000.0, penHi = 1000.0;
    static int speedAmp = 1;
    static float sensitivity = 0.5F;
    static final List<int[]> blocks = new ArrayList<int[]>();
    static boolean unbreakable = false;
    static boolean useDown = false;

    // State.
    static int state = 0, wait = 0;
    static volatile boolean setupDone = false, recording = false, finished = false, ready = false;
    static int ctick = 0;
    static boolean inClientTick = false;
    static int stick = -1;
    static boolean serverRecording = false, inServerTick = false;
    static EntityPlayerMP real, fake;
    static FakeNet fakeNet;

    static void log(String s) {
        synchronized (lock) {
            out.println(s);
        }
    }

    static String hd(double d) { return Long.toHexString(Double.doubleToRawLongBits(d)); }
    static String hf(float f) { return Integer.toHexString(Float.floatToRawIntBits(f)); }

    static Object get(Object o, Class<?> c, String name) {
        try {
            Field f = c.getDeclaredField(name);
            f.setAccessible(true);
            return f.get(o);
        } catch (Exception e) {
            throw new RuntimeException(e);
        }
    }

    static void loadScript() throws Exception {
        out = new PrintWriter(new FileWriter(OUT));
        BufferedReader r = new BufferedReader(new FileReader(SCRIPT));
        String line;
        while ((line = r.readLine()) != null) {
            line = line.trim();
            if (line.isEmpty() || line.startsWith("#")) continue;
            String[] p = line.split("\\s+");
            if (p[0].equals("C")) {
                // forward strafe jump sprint attack dx dy [use]
                int[] in = new int[8];
                for (int i = 0; i < 8 && 1 + i < p.length; i++) in[i] = Integer.parseInt(p[1 + i]);
                inputs.add(in);
            } else if (p[0].equals("F")) {
                int t = Integer.parseInt(p[1]);
                List<String[]> l = fakeActions.get(t);
                if (l == null) fakeActions.put(t, l = new ArrayList<String[]>());
                l.add(p);
            } else if (p[0].equals("start")) {
                for (int i = 0; i < 5; i++) start[i] = Double.parseDouble(p[1 + i]);
            } else if (p[0].equals("fake")) {
                fakeStart = new double[5];
                for (int i = 0; i < 5; i++) fakeStart[i] = Double.parseDouble(p[1 + i]);
            } else if (p[0].equals("settle")) {
                settle = Integer.parseInt(p[1]);
            } else if (p[0].equals("speed")) {
                speedAmp = Integer.parseInt(p[1]);
            } else if (p[0].equals("sensitivity")) {
                sensitivity = Float.parseFloat(p[1]);
            } else if (p[0].equals("pen")) {
                penLo = Double.parseDouble(p[1]);
                penHi = Double.parseDouble(p[2]);
            } else if (p[0].equals("unbreakable")) {
                unbreakable = !p[1].equals("0");
            } else if (p[0].equals("block")) {
                blocks.add(new int[] {Integer.parseInt(p[1]), Integer.parseInt(p[2]), Integer.parseInt(p[3])});
            }
        }
        r.close();
        log("HEADER ticks=" + inputs.size() + " speed=" + speedAmp + " sensitivity=" + hf(sensitivity)
            + " fake=" + (fakeStart != null ? 1 : 0) + " blocks=" + blocks.size() + " fmt=2 wear=" + (unbreakable ? 0 : 1));
        for (int[] b : blocks) log("BLOCK " + b[0] + " " + b[1] + " " + b[2]);
    }

    // ------------------------------------------------------------------ client side

    public static void clientPre(Minecraft mc) {
        try {
            clientPre0(mc);
        } catch (Throwable t) {
            t.printStackTrace();
            fail("clientPre " + t);
        }
    }

    static void fail(String why) {
        if (out != null) {
            log("FAIL " + why);
            out.flush();
        }
        Runtime.getRuntime().halt(3);
    }

    static void clientPre0(Minecraft mc) throws Exception {
        GameSettings gs = mc.gameSettings;
        if (state == 0) {
            if (!(mc.currentScreen instanceof GuiMainMenu)) return;
            loadScript();
            gs.pauseOnLostFocus = false;
            gs.mouseSensitivity = sensitivity;
            gs.invertMouse = false;
            gs.renderDistanceChunks = 2;
            gs.limitFramerate = 30;
            gs.fancyGraphics = false;
            gs.ambientOcclusion = 0;
            gs.clouds = 0;
            gs.particleSetting = 2;
            gs.viewBobbing = false;
            gs.smoothCamera = false;
            WorldSettings ws = new WorldSettings(1L, WorldSettings.GameType.SURVIVAL, false, false, WorldType.FLAT);
            ws.setWorldName("3;minecraft:bedrock,2*minecraft:dirt,minecraft:grass;0;");
            ws.enableCommands();
            mc.launchIntegratedServer("rig" + System.currentTimeMillis(), "rig", ws);
            state = 1;
            return;
        }
        if (state == 1) {
            if (mc.thePlayer == null || mc.theWorld == null || mc.getIntegratedServer() == null) return;
            if (mc.currentScreen != null) {
                if (++wait > 200) mc.displayGuiScreen(null);
                return;
            }
            if (++wait < 60) return;
            wait = 0;
            final MinecraftServer server = mc.getIntegratedServer();
            final String name = mc.thePlayer.getName();
            server.addScheduledTask(new Runnable() {
                public void run() {
                    serverSetup(server, name);
                }
            });
            state = 2;
            return;
        }
        mc.inGameHasFocus = false;
        if (state == 2) {
            if (!setupDone) return;
            if (mc.currentScreen != null) mc.displayGuiScreen(null);
            if (++wait <= settle || !ready) return;
            if (fakeStart != null && other(mc) == null) return;
            clientInit(mc);
            recording = true;
            state = 3;
        }
        if (state == 3) {
            if (ctick >= inputs.size()) {
                log("END");
                out.flush();
                finished = true;
                state = 4;
                wait = 0;
                return;
            }
            int[] in = inputs.get(ctick);
            key(gs.keyBindForward, in[0] > 0);
            key(gs.keyBindBack, in[0] < 0);
            key(gs.keyBindLeft, in[1] > 0);
            key(gs.keyBindRight, in[1] < 0);
            key(gs.keyBindJump, in[2] != 0);
            key(gs.keyBindSprint, in[3] != 0);
            if (in[4] != 0) KeyBinding.onTick(gs.keyBindAttack.getKeyCode());
            // The right button: a press when it goes down, then held.
            boolean use = in[7] != 0;
            if (use && !useDown) KeyBinding.onTick(gs.keyBindUseItem.getKeyCode());
            key(gs.keyBindUseItem, use);
            useDown = use;
            if (in[5] != 0 || in[6] != 0) {
                float f = gs.mouseSensitivity * 0.6F + 0.2F;
                float f1 = f * f * f * 8.0F;
                mc.thePlayer.setAngles((float) in[5] * f1, (float) in[6] * f1);
            }
            log("CI " + ctick + " " + in[0] + " " + in[1] + " " + in[2] + " " + in[3] + " " + in[4] + " " + in[5] + " " + in[6] + " " + in[7]);
            inClientTick = true;
            return;
        }
        if (state == 4 && ++wait > 20) {
            out.close();
            Runtime.getRuntime().halt(0);
        }
    }

    static void key(KeyBinding k, boolean down) {
        KeyBinding.setKeyBindState(k.getKeyCode(), down);
    }

    static EntityOtherPlayerMP other(Minecraft mc) {
        for (Object o : mc.theWorld.playerEntities) {
            if (o instanceof EntityOtherPlayerMP) return (EntityOtherPlayerMP) o;
        }
        return null;
    }

    static String remote(EntityOtherPlayerMP o) {
        if (o == null) return "";
        Class<?> c = EntityOtherPlayerMP.class;
        return " rid=" + o.getEntityId() + " rx=" + hd(o.posX) + " ry=" + hd(o.posY) + " rz=" + hd(o.posZ)
            + " ryaw=" + hf(o.rotationYaw) + " rpitch=" + hf(o.rotationPitch)
            + " rsx=" + o.serverPosX + " rsy=" + o.serverPosY + " rsz=" + o.serverPosZ
            + " rinc=" + get(o, c, "otherPlayerMPPosRotationIncrements")
            + " rtx=" + hd((Double) get(o, c, "otherPlayerMPX")) + " rty=" + hd((Double) get(o, c, "otherPlayerMPY"))
            + " rtz=" + hd((Double) get(o, c, "otherPlayerMPZ")) + " rtyaw=" + hd((Double) get(o, c, "otherPlayerMPYaw"))
            + " rtpitch=" + hd((Double) get(o, c, "otherPlayerMPPitch"))
            + " rog=" + (o.onGround ? 1 : 0) + " rspr=" + (o.isSprinting() ? 1 : 0) + " rht=" + o.hurtTime
            + " rswing=" + (o.isSwingInProgress ? 1 : 0) + " rswp=" + o.swingProgressInt
            + " reat=" + (o.isEating() ? 1 : 0);
    }

    static String local(EntityPlayerSP p) {
        Class<?> c = EntityPlayerSP.class;
        return "id=" + p.getEntityId() + " x=" + hd(p.posX) + " y=" + hd(p.posY) + " z=" + hd(p.posZ)
            + " mx=" + hd(p.motionX) + " my=" + hd(p.motionY) + " mz=" + hd(p.motionZ)
            + " yaw=" + hf(p.rotationYaw) + " pitch=" + hf(p.rotationPitch) + " hyaw=" + hf(p.rotationYawHead)
            + " og=" + (p.onGround ? 1 : 0) + " spr=" + (p.isSprinting() ? 1 : 0)
            + " sss=" + (((Boolean) get(p, c, "serverSprintState")) ? 1 : 0)
            + " put=" + get(p, c, "positionUpdateTicks")
            + " lrx=" + hd((Double) get(p, c, "lastReportedPosX")) + " lry=" + hd((Double) get(p, c, "lastReportedPosY"))
            + " lrz=" + hd((Double) get(p, c, "lastReportedPosZ")) + " lryaw=" + hf((Float) get(p, c, "lastReportedYaw"))
            + " lrpitch=" + hf((Float) get(p, c, "lastReportedPitch"))
            + " stl=" + p.sprintingTicksLeft + " stt=" + get(p, c, "sprintToggleTimer")
            + " jt=" + get(p, EntityLivingBase.class, "jumpTicks") + " jmf=" + hf(p.jumpMovementFactor)
            + " attr=" + hd(p.getEntityAttribute(SharedMonsterAttributes.movementSpeed).getAttributeValue())
            + " mf=" + hf(p.moveForward) + " ms=" + hf(p.moveStrafing)
            + " mif=" + hf(p.movementInput.moveForward) + " mis=" + hf(p.movementInput.moveStrafe)
            + " mij=" + (p.movementInput.jump ? 1 : 0)
            + " ht=" + p.hurtTime + " fall=" + hf(p.fallDistance) + " food=" + p.getFoodStats().getFoodLevel()
            + " ch=" + (p.isCollidedHorizontally ? 1 : 0)
            + " use=" + (p.isUsingItem() ? 1 : 0) + " eat=" + (p.isEating() ? 1 : 0)
            + " rcd=" + get(Minecraft.getMinecraft(), Minecraft.class, "rightClickDelayTimer")
            + " lcc=" + get(Minecraft.getMinecraft(), Minecraft.class, "leftClickCounter")
            + " sd=" + wear(p);
    }

    /** Damage value of the held item, -1 with empty hands. */
    static int wear(EntityPlayer p) {
        ItemStack s = p.inventory.getCurrentItem();
        return s == null ? -1 : s.getItemDamage();
    }

    static void clientInit(Minecraft mc) {
        log("CINIT " + local(mc.thePlayer) + remote(other(mc)));
    }

    public static void clientPost(Minecraft mc) {
        if (!inClientTick) return;
        inClientTick = false;
        try {
            MovingObjectPosition m = mc.objectMouseOver;
            // 1 the other player, 2 a block, 0 nothing.
            int pick = m == null ? 0 : m.typeOfHit == MovingObjectPosition.MovingObjectType.ENTITY ? 1
                : m.typeOfHit == MovingObjectPosition.MovingObjectType.BLOCK ? 2 : 0;
            log("CT " + ctick + " " + local(mc.thePlayer) + " pick=" + pick + remote(other(mc)));
        } catch (Throwable t) {
            t.printStackTrace();
            fail("clientPost " + t);
        }
        ctick++;
    }

    public static void onClientSend(Packet p) {
        if (p == null || state != 3 || !wanted(p)) return;
        log("CS " + ctick + " " + dump(p));
    }

    /** Write down a packet that is sent without going through addToSendQueue. */
    public static Packet noted(Packet p) {
        onClientSend(p);
        return p;
    }

    // ------------------------------------------------------------------ both sides

    public static void onHandle(Packet p, INetHandler h) {
        try {
            if (h instanceof NetHandlerPlayClient) {
                if (state == 3 && wanted(p)) log("CP " + ctick + " " + dump(p));
            } else if (h instanceof NetHandlerPlayServer) {
                if (serverRecording && !finished && wanted(p)) {
                    log("SP " + stick + " " + who(((NetHandlerPlayServer) h).playerEntity) + " " + dump(p));
                }
            }
        } catch (Throwable t) {
            t.printStackTrace();
            fail("onHandle " + t);
        }
    }

    static int who(EntityPlayerMP p) {
        return p == fake ? 1 : 0;
    }

    static final String[] WANT = {
        "C03PacketPlayer", "C02PacketUseEntity", "C0APacketAnimation", "C0BPacketEntityAction", "C09PacketHeldItemChange",
        "C07PacketPlayerDigging", "C08PacketPlayerBlockPlacement", "S2FPacketSetSlot",
        "S12PacketEntityVelocity", "S19PacketEntityStatus", "S19PacketEntityHeadLook", "S14PacketEntity", "S18PacketEntityTeleport",
        "S1CPacketEntityMetadata", "S20PacketEntityProperties", "S0BPacketAnimation", "S08PacketPlayerPosLook",
        "S1DPacketEntityEffect", "S1EPacketRemoveEntityEffect", "S06PacketUpdateHealth", "S0CPacketSpawnPlayer",
        "S13PacketDestroyEntities", "S27PacketExplosion",
    };

    static boolean wanted(Packet p) {
        String n = p.getClass().getName();
        for (String w : WANT) {
            if (n.contains(w)) return true;
        }
        return false;
    }

    /** Class name, then every plain field (own and inherited) as name=value. */
    static String dump(Packet p) {
        StringBuilder sb = new StringBuilder();
        String n = p.getClass().getName();
        sb.append(n.substring(n.lastIndexOf('.') + 1));
        try {
            for (Class<?> c = p.getClass(); c != null && c != Object.class; c = c.getSuperclass()) {
                for (Field f : c.getDeclaredFields()) {
                    if (Modifier.isStatic(f.getModifiers())) continue;
                    f.setAccessible(true);
                    Object v = f.get(p);
                    sb.append(' ').append(f.getName()).append('=');
                    if (v instanceof Double) sb.append(hd((Double) v));
                    else if (v instanceof Float) sb.append(hf((Float) v));
                    else if (v instanceof Boolean) sb.append(((Boolean) v) ? 1 : 0);
                    else if (v instanceof Number) sb.append(v);
                    else if (v instanceof Enum) sb.append(((Enum<?>) v).name());
                    else if (v instanceof ItemStack) {
                        ItemStack st = (ItemStack) v;
                        sb.append(net.minecraft.item.Item.getIdFromItem(st.getItem())).append('/').append(st.getItemDamage())
                            .append('/').append(st.stackSize);
                    } else if (v instanceof BlockPos) {
                        BlockPos b = (BlockPos) v;
                        sb.append(b.getX()).append(',').append(b.getY()).append(',').append(b.getZ());
                    }
                    else if (v instanceof int[]) {
                        int[] a = (int[]) v;
                        for (int i = 0; i < a.length; i++) sb.append(i > 0 ? "," : "").append(a[i]);
                    } else if (v instanceof Collection) sb.append(list((Collection<?>) v));
                    else sb.append(v == null ? "null" : "?");
                }
            }
        } catch (Exception e) {
            throw new RuntimeException(e);
        }
        return sb.toString();
    }

    static String list(Collection<?> l) {
        StringBuilder sb = new StringBuilder("[");
        for (Object o : l) {
            if (o instanceof DataWatcher.WatchableObject) {
                DataWatcher.WatchableObject w = (DataWatcher.WatchableObject) o;
                Object v = w.getObject();
                sb.append(w.getDataValueId()).append(':').append(w.getObjectType()).append(':')
                    .append(v instanceof Float ? hf((Float) v) : v instanceof Number ? String.valueOf(v) : "?").append(';');
            } else if (o instanceof S20PacketEntityProperties.Snapshot) {
                S20PacketEntityProperties.Snapshot s = (S20PacketEntityProperties.Snapshot) o;
                sb.append(s.func_151409_a()).append(':').append(hd(s.func_151410_b()));
                for (AttributeModifier m : s.func_151408_c()) {
                    sb.append(',').append(m.getID()).append('/').append(m.getOperation()).append('/').append(hd(m.getAmount()));
                }
                sb.append(';');
            } else {
                sb.append("?;");
            }
        }
        return sb.append(']').toString();
    }

    // ------------------------------------------------------------------ server side

    static void serverSetup(MinecraftServer server, String name) {
        try {
            WorldServer w = server.worldServerForDimension(0);
            w.getGameRules().setOrCreateGameRule("doMobSpawning", "false");
            w.getGameRules().setOrCreateGameRule("doDaylightCycle", "false");
            w.setWorldTime(6000L);
            for (int[] b : blocks) {
                w.setBlockState(new net.minecraft.util.BlockPos(b[0], b[1], b[2]), net.minecraft.init.Blocks.stone.getDefaultState());
            }
            real = server.getConfigurationManager().getPlayerByUsername(name);
            prepare(real);
            real.playerNetServerHandler.setPlayerLocation(start[0], start[1], start[2], (float) start[3], (float) start[4]);
            if (fakeStart != null) {
                GameProfile gp = new GameProfile(UUID.fromString("00000000-0000-4000-8000-000000000001"), "Dummy");
                fake = new EntityPlayerMP(server, w, gp, new ItemInWorldManager(w));
                fakeNet = new FakeNet(server, fake);
                fake.setPositionAndRotation(fakeStart[0], fakeStart[1], fakeStart[2], (float) fakeStart[3], (float) fakeStart[4]);
                fake.theItemInWorldManager.setGameType(WorldSettings.GameType.SURVIVAL);
                server.getConfigurationManager().playerLoggedIn(fake);
                // As for a player that joins: it is told about changes to its own inventory.
                fake.addSelfToInternalCraftingInventory();
                prepare(fake);
                fakeNet.setPlayerLocation(fakeStart[0], fakeStart[1], fakeStart[2], (float) fakeStart[3], (float) fakeStart[4]);
                fakeNet.processPlayer(new C03PacketPlayer.C06PacketPlayerPosLook(fakeStart[0], fakeStart[1], fakeStart[2],
                    (float) fakeStart[3], (float) fakeStart[4], true));
            }
            setupDone = true;
        } catch (Throwable t) {
            t.printStackTrace();
            fail("serverSetup " + t);
        }
    }

    static void prepare(EntityPlayerMP p) {
        p.clearActivePotions();
        if (speedAmp >= 0) p.addPotionEffect(new PotionEffect(Potion.moveSpeed.id, 10000000, speedAmp, false, false));
        p.addPotionEffect(new PotionEffect(Potion.resistance.id, 10000000, 4, false, false));
        p.addPotionEffect(new PotionEffect(Potion.saturation.id, 10000000, 0, false, false));
        ItemStack sword = new ItemStack(Items.diamond_sword);
        if (unbreakable) {
            NBTTagCompound tag = new NBTTagCompound();
            tag.setBoolean("Unbreakable", true);
            sword.setTagCompound(tag);
        }
        p.inventory.mainInventory[0] = sword;
        p.inventory.currentItem = 0;
        p.setHealth(p.getMaxHealth());
    }

    static EntityTrackerEntry entry(EntityPlayerMP p) {
        WorldServer w = (WorldServer) p.worldObj;
        Object set = get(w.getEntityTracker(), net.minecraft.entity.EntityTracker.class, "trackedEntities");
        for (Object o : (java.util.Set<?>) set) {
            if (((EntityTrackerEntry) o).trackedEntity == p) return (EntityTrackerEntry) o;
        }
        return null;
    }

    static String tracker(EntityPlayerMP p) {
        EntityTrackerEntry e = entry(p);
        if (e == null) return " tr=0";
        Class<?> c = EntityTrackerEntry.class;
        return " tr=1 uc=" + e.updateCounter + " tsl=" + get(e, c, "ticksSinceLastForcedTeleport")
            + " ex=" + e.encodedPosX + " ey=" + e.encodedPosY + " ez=" + e.encodedPosZ
            + " eyaw=" + e.encodedRotationYaw + " epitch=" + e.encodedRotationPitch + " ehead=" + e.lastHeadMotion
            + " tog=" + (((Boolean) get(e, c, "onGround")) ? 1 : 0) + " tfirst=" + (((Boolean) get(e, c, "firstUpdateDone")) ? 1 : 0)
            + " tmx=" + hd(e.lastTrackedEntityMotionX) + " tmy=" + hd(e.lastTrackedEntityMotionY) + " tmz=" + hd(e.motionZ);
    }

    static String sp(EntityPlayerMP p) {
        return "who=" + who(p) + " id=" + p.getEntityId() + " x=" + hd(p.posX) + " y=" + hd(p.posY) + " z=" + hd(p.posZ)
            + " mx=" + hd(p.motionX) + " my=" + hd(p.motionY) + " mz=" + hd(p.motionZ)
            + " yaw=" + hf(p.rotationYaw) + " pitch=" + hf(p.rotationPitch)
            + " og=" + (p.onGround ? 1 : 0) + " spr=" + (p.isSprinting() ? 1 : 0)
            + " hrt=" + p.hurtResistantTime + " ht=" + p.hurtTime
            + " ld=" + hf((Float) get(p, EntityLivingBase.class, "lastDamage"))
            + " vc=" + (p.velocityChanged ? 1 : 0) + " ab=" + (p.isAirBorne ? 1 : 0)
            + " jt=" + get(p, EntityLivingBase.class, "jumpTicks") + " jmf=" + hf(p.jumpMovementFactor)
            + " attr=" + hd(p.getEntityAttribute(SharedMonsterAttributes.movementSpeed).getAttributeValue())
            + " fall=" + hf(p.fallDistance) + " hp=" + hf(p.getHealth()) + " food=" + p.getFoodStats().getFoodLevel()
            + " swing=" + (p.isSwingInProgress ? 1 : 0) + " swp=" + p.swingProgressInt
            + " use=" + (p.isUsingItem() ? 1 : 0) + " eat=" + (p.isEating() ? 1 : 0) + " blk=" + (p.isBlocking() ? 1 : 0)
            + " sd=" + wear(p)
            + " rit=" + get(p, EntityPlayerMP.class, "respawnInvulnerabilityTicks")
            + net(p.playerNetServerHandler)
            + " ccx=" + p.chunkCoordX + " ccy=" + p.chunkCoordY + " ccz=" + p.chunkCoordZ + " atc=" + (p.addedToChunk ? 1 : 0)
            + tracker(p);
    }

    static String net(NetHandlerPlayServer h) {
        Class<?> c = NetHandlerPlayServer.class;
        return " hm=" + (((Boolean) get(h, c, "hasMoved")) ? 1 : 0)
            + " lpx=" + hd((Double) get(h, c, "lastPosX")) + " lpy=" + hd((Double) get(h, c, "lastPosY"))
            + " lpz=" + hd((Double) get(h, c, "lastPosZ"))
            + " ntc=" + get(h, c, "networkTickCount") + " lmt=" + get(h, c, "field_175090_f");
    }

    public static void serverPre(MinecraftServer server) {
        try {
            if (real == null || finished) return;
            WorldServer w = server.worldServerForDimension(0);
            for (Object o : new ArrayList<Object>(w.loadedEntityList)) {
                if (!(o instanceof EntityPlayer)) ((Entity) o).setDead();
            }
            if (!recording) {
                // A client reports in every tick; do the same for the scripted player.
                if (fake != null && setupDone) fakeNet.processPlayer(new C03PacketPlayer(true));
                ready = setupDone && (fake == null || (entry(real).trackingPlayers.contains(fake) && entry(fake).trackingPlayers.contains(real)));
                return;
            }
            if (!serverRecording) {
                serverRecording = true;
                log("SINIT " + sp(real));
                if (fake != null) log("SINIT " + sp(fake));
            }
            stick++;
            inServerTick = true;
            List<String[]> acts = fakeActions.get(stick);
            if (acts != null && fake != null) {
                for (String[] a : acts) fakeAct(a);
            }
        } catch (Throwable t) {
            t.printStackTrace();
            fail("serverPre " + t);
        }
    }

    static void fakeAct(String[] a) {
        Packet p;
        String k = a[2];
        if (k.equals("POS")) {
            p = new C03PacketPlayer.C06PacketPlayerPosLook(Double.parseDouble(a[3]), Double.parseDouble(a[4]), Double.parseDouble(a[5]),
                Float.parseFloat(a[6]), Float.parseFloat(a[7]), !a[8].equals("0"));
        } else if (k.equals("FRONT")) {
            // F tick FRONT distance angle height yawNoise pitch: a spot at this distance from the
            // real player, this many degrees off its line of sight, facing it.
            double dist = Double.parseDouble(a[3]), angle = Double.parseDouble(a[4]), up = Double.parseDouble(a[5]);
            double rad = ((double) real.rotationYaw + angle) * Math.PI / 180.0;
            double x = Math.max(penLo, Math.min(penHi, real.posX - Math.sin(rad) * dist));
            double z = Math.max(penLo, Math.min(penHi, real.posZ + Math.cos(rad) * dist));
            float yaw = (float) (Math.atan2(-(real.posX - x), real.posZ - z) * 180.0 / Math.PI) + Float.parseFloat(a[6]);
            p = new C03PacketPlayer.C06PacketPlayerPosLook(x, 4.0 + up, z, yaw, Float.parseFloat(a[7]), up == 0.0);
        } else if (k.equals("GOTO")) {
            // F tick GOTO x z step height yawNoise pitch: walk towards a point, at most one step.
            double gx = Double.parseDouble(a[3]) - fake.posX, gz = Double.parseDouble(a[4]) - fake.posZ;
            double step = Double.parseDouble(a[5]), up = Double.parseDouble(a[6]);
            double len = Math.sqrt(gx * gx + gz * gz);
            double f = len > step ? step / len : 1.0;
            double x = fake.posX + gx * f, z = fake.posZ + gz * f;
            float yaw = (float) (Math.atan2(-(real.posX - x), real.posZ - z) * 180.0 / Math.PI) + Float.parseFloat(a[7]);
            p = new C03PacketPlayer.C06PacketPlayerPosLook(x, 4.0 + up, z, yaw, Float.parseFloat(a[8]), up == 0.0);
        } else if (k.equals("GROUND")) {
            p = new C03PacketPlayer(!a[3].equals("0"));
        } else if (k.equals("SPRINT")) {
            p = new C0BPacketEntityAction(fake, a[3].equals("0") ? C0BPacketEntityAction.Action.STOP_SPRINTING : C0BPacketEntityAction.Action.START_SPRINTING);
        } else if (k.equals("SWING")) {
            p = new C0APacketAnimation();
        } else if (k.equals("ATTACK")) {
            p = new C02PacketUseEntity(real, C02PacketUseEntity.Action.ATTACK);
        } else if (k.equals("USE")) {
            // Right click with the held item, as a client sends it.
            p = new C08PacketPlayerBlockPlacement(fake.inventory.getCurrentItem());
        } else if (k.equals("RELEASE")) {
            p = new C07PacketPlayerDigging(C07PacketPlayerDigging.Action.RELEASE_USE_ITEM, BlockPos.ORIGIN, EnumFacing.DOWN);
        } else {
            throw new RuntimeException("unknown fake action " + k);
        }
        log("SP " + stick + " 1 " + dump(p));
        if (p instanceof C03PacketPlayer) fakeNet.processPlayer((C03PacketPlayer) p);
        else if (p instanceof C0BPacketEntityAction) fakeNet.processEntityAction((C0BPacketEntityAction) p);
        else if (p instanceof C0APacketAnimation) fakeNet.handleAnimation((C0APacketAnimation) p);
        else if (p instanceof C08PacketPlayerBlockPlacement) fakeNet.processPlayerBlockPlacement((C08PacketPlayerBlockPlacement) p);
        else if (p instanceof C07PacketPlayerDigging) fakeNet.processPlayerDigging((C07PacketPlayerDigging) p);
        else fakeNet.processUseEntity((C02PacketUseEntity) p);
    }

    public static void serverPost(MinecraftServer server) {
        // The server ticks every real connection here; the scripted player has none.
        if (fake != null && setupDone && !finished) fakeNet.update();
        if (!inServerTick) return;
        inServerTick = false;
        try {
            log("ST " + stick + " " + sp(real));
            if (fake != null) log("ST " + stick + " " + sp(fake));
        } catch (Throwable t) {
            t.printStackTrace();
            fail("serverPost " + t);
        }
    }

    public static void onServerSend(NetHandlerPlayServer h, Packet p) {
        if (!serverRecording || finished || !wanted(p)) return;
        log("SS " + stick + " " + who(h.playerEntity) + " " + dump(p));
    }

    /** Connection of the scripted player: nothing is sent anywhere, only written down. */
    static final class FakeNet extends NetHandlerPlayServer {
        FakeNet(MinecraftServer server, EntityPlayerMP p) {
            super(server, new NetworkManager(EnumPacketDirection.SERVERBOUND), p);
        }

        public void sendPacket(Packet p) {
            if (serverRecording && !finished && wanted(p)) log("SS " + stick + " 1 " + dump(p));
        }

        public void kickPlayerFromServer(String reason) {
            fail("the scripted player was kicked: " + reason);
        }
    }
}
