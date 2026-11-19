package pvpai.forge;

import java.io.BufferedInputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.util.Locale;
import java.util.Random;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.inventory.GuiInventory;
import net.minecraft.client.settings.KeyBinding;
import net.minecraft.util.ChatComponentText;
import net.minecraftforge.client.event.GuiScreenEvent;
import net.minecraftforge.client.event.MouseEvent;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.fml.client.registry.ClientRegistry;
import net.minecraftforge.fml.common.FMLCommonHandler;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.common.event.FMLInitializationEvent;
import net.minecraftforge.fml.common.event.FMLPreInitializationEvent;
import net.minecraftforge.fml.common.eventhandler.SubscribeEvent;
import net.minecraftforge.fml.common.gameevent.TickEvent;
import org.lwjgl.input.Keyboard;
import org.lwjgl.input.Mouse;
import pvpai.client.Session;
import pvpai.client.Settings;
import pvpai.core.Policy;

/**
 * The mod as Forge loads it: a key that hands the keys of the game to the
 * trained policy for a duel against the nearest player and takes them back.
 * The policy comes inside the jar; a file policy.bin in the folder
 * config/pvpai is used in its place, and ajustes.txt there holds what the
 * player sets.
 */
@Mod(modid = "pvpai", name = "PvP AI", version = "1.0", clientSideOnly = true, acceptedMinecraftVersions = "[1.8.9]")
public final class PvpAi {
    private final KeyBinding key = new KeyBinding("PvP AI: prender y apagar", Keyboard.KEY_K, "PvP AI");
    private File dir = new File("config", "pvpai");
    private Session session;
    private boolean keyWasDown;
    private final SelfTest selfTest = Boolean.getBoolean("pvpai.selftest") ? new SelfTest(this) : null;
    private final ServerTest serverTest = System.getProperty("pvpai.servertest") != null ? new ServerTest(this, System.getProperty("pvpai.servertest")) : null;

    @Mod.EventHandler
    public void preInit(FMLPreInitializationEvent e) {
        dir = new File(e.getModConfigurationDirectory(), "pvpai");
    }

    @Mod.EventHandler
    public void init(FMLInitializationEvent e) {
        ClientRegistry.registerKeyBinding(key);
        warmUp();
        // One bus or two, depending on the Forge build; a second time on the same one does nothing.
        MinecraftForge.EVENT_BUS.register(this);
        FMLCommonHandler.instance().bus().register(this);
    }

    /**
     * Runs a copy of the policy a few hundred times on a thread of its own
     * while the game loads, so that the code it runs is compiled before the
     * first duel: cold, its first ticks take 20 to 30 ms instead of half of one.
     */
    private void warmUp() {
        Thread t = new Thread(new Runnable() {
            @Override
            public void run() {
                try {
                    Policy p = policy();
                    float[] obs = new float[p.obsDim];
                    Random rng = new Random(1);
                    for (int i = 0; i < 400; i++) {
                        obs[i % obs.length] = (i % 7) * 0.1f;
                        p.act(obs, rng);
                    }
                } catch (Throwable ignored) {
                    // Only the first ticks are slower without it.
                }
            }
        }, "pvpai-warmup");
        t.setDaemon(true);
        t.setPriority(Thread.MIN_PRIORITY);
        t.start();
    }

    /** The policy: the player's own file if there is one, else the one in the jar. */
    private Policy policy() throws IOException {
        File own = new File(dir, "policy.bin");
        InputStream in = own.isFile() ? new FileInputStream(own) : PvpAi.class.getResourceAsStream("/pvpai/policy.bin");
        if (in == null) {
            throw new IOException("no policy in the jar");
        }
        try {
            return Policy.load(new BufferedInputStream(in));
        } finally {
            in.close();
        }
    }

    private static void say(Minecraft mc, String line) {
        if (line != null && mc.ingameGUI != null) {
            mc.ingameGUI.getChatGUI().printChatMessage(new ChatComponentText(line));
        }
    }

    boolean on() {
        return session != null && session.on();
    }

    @SubscribeEvent
    public void mouse(MouseEvent event) {
        if (filterMouse(event.button, event.dwheel, event.buttonstate)) {
            event.setCanceled(true);
        }
    }

    @SubscribeEvent
    public void inventoryMouse(GuiScreenEvent.MouseInputEvent.Pre event) {
        if (event.gui instanceof GuiInventory && filterMouse(Mouse.getEventButton(), Mouse.getEventDWheel(), Mouse.getEventButtonState())) {
            event.setCanceled(true);
        }
    }

    private boolean filterMouse(int button, int wheel, boolean down) {
        if (!on() || (button < 0 && wheel == 0)) {
            return false;
        }
        if (button >= 0 && button - 100 == key.getKeyCode() && down) {
            session.stop();
            say(Minecraft.getMinecraft(), "PvP AI apagado.");
        }
        return true;
    }

    /** Whether the key was pressed since the last tick. */
    private boolean pressed(Minecraft mc) {
        boolean pressed = false;
        while (key.isPressed()) {
            pressed = true;
        }
        // With the inventory open, where the agent goes for its potions, the game reads no keys.
        int code = key.getKeyCode();
        boolean down = code > 0 && code < Keyboard.KEYBOARD_SIZE && Keyboard.isKeyDown(code);
        if (down && !keyWasDown && on() && mc.currentScreen instanceof GuiInventory) {
            pressed = true;
        }
        keyWasDown = down;
        return pressed;
    }

    /** Hand the keys over, or take them back. */
    void flip(Minecraft mc) throws IOException {
        if (on()) {
            session.stop();
            say(mc, "PvP AI apagado.");
            return;
        }
        Policy policy = policy();
        session = new Session(mc, policy);
        String line = session.start(Settings.load(new File(dir, "ajustes.txt")));
        if (session.on()) {
            line += String.format(Locale.ROOT, " Entrenado con %d millones de muestras.", policy.samples / 1000000L);
        }
        say(mc, line);
    }

    @SubscribeEvent
    public void tick(TickEvent.ClientTickEvent e) {
        Minecraft mc = Minecraft.getMinecraft();
        try {
            if (e.phase == TickEvent.Phase.START) {
                if (selfTest != null) {
                    selfTest.step(mc);
                }
                if (serverTest != null) {
                    serverTest.step(mc);
                }
                if (pressed(mc)) {
                    flip(mc);
                }
                if (session != null) {
                    say(mc, session.tickStart());
                }
            } else if (session != null) {
                session.tickEnd();
            }
        } catch (Throwable t) {
            // Whatever goes wrong, the player gets the keys back and the game goes on.
            t.printStackTrace();
            try {
                if (session != null) {
                    session.stop();
                }
            } catch (Throwable again) {
                again.printStackTrace();
            }
            session = null;
            say(mc, "PvP AI apagado por un error: " + t);
        }
    }
}
