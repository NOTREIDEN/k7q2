package pvpai.forge;

import java.io.BufferedInputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.util.Locale;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.inventory.GuiInventory;
import net.minecraft.client.settings.KeyBinding;
import net.minecraft.util.ChatComponentText;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.fml.client.registry.ClientRegistry;
import net.minecraftforge.fml.common.FMLCommonHandler;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.common.event.FMLInitializationEvent;
import net.minecraftforge.fml.common.event.FMLPreInitializationEvent;
import net.minecraftforge.fml.common.eventhandler.SubscribeEvent;
import net.minecraftforge.fml.common.gameevent.TickEvent;
import org.lwjgl.input.Keyboard;
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

    @Mod.EventHandler
    public void preInit(FMLPreInitializationEvent e) {
        dir = new File(e.getModConfigurationDirectory(), "pvpai");
    }

    @Mod.EventHandler
    public void init(FMLInitializationEvent e) {
        ClientRegistry.registerKeyBinding(key);
        // One bus or two, depending on the Forge build; a second time on the same one does nothing.
        MinecraftForge.EVENT_BUS.register(this);
        FMLCommonHandler.instance().bus().register(this);
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
