package chestai.client;

import java.io.File;
import java.io.IOException;
import net.minecraft.client.gui.GuiButton;
import net.minecraft.client.gui.GuiScreen;

/**
 * The screen of the SilentAim settings (the U key): a row for each option,
 * - and + for a number, a button that goes to the next value for the rest;
 * each change saved at once, and read again by the farmer when the screen is shut.
 */
public final class GuiSilentAim extends GuiScreen {
    private static final int DONE = 200;
    private static final int ROW_HEIGHT = 21;

    private final SilentAimProfile profile;
    private int left, top;
    /** Why the last change could not be saved, or null. */
    private String error;

    public GuiSilentAim(File dir) throws IOException {
        profile = new SilentAimProfile(dir);
    }

    @Override
    public void initGui() {
        buttonList.clear();
        left = width / 2 - 143;
        top = Math.max(5, (height - (SilentAimProfile.Option.values().length * ROW_HEIGHT + 61)) / 2);
        // Two ids for each option: the even one goes down, the odd one up.
        for (SilentAimProfile.Option option : SilentAimProfile.Option.values()) {
            int id = option.ordinal() * 2;
            int y = rowY(option) - 5;
            if (option.step != 0) {
                buttonList.add(new GuiButton(id, left + 198, y, 22, 18, "-"));
                buttonList.add(new GuiButton(id + 1, left + 262, y, 22, 18, "+"));
            } else {
                buttonList.add(new GuiButton(id, left + 198, y, 86, 18, label(option)));
            }
        }
        buttonList.add(new GuiButton(DONE, width / 2 - 54, rowY(null), 108, 20, "Listo"));
    }

    /** Where the option's row is drawn; null for the row under the last one. */
    private int rowY(SilentAimProfile.Option option) {
        return top + 26 + (option == null ? SilentAimProfile.Option.values().length : option.ordinal()) * ROW_HEIGHT;
    }

    private String label(SilentAimProfile.Option option) {
        return option.display(profile.get(option));
    }

    @Override
    protected void actionPerformed(GuiButton button) {
        if (button.id == DONE) {
            mc.displayGuiScreen(null);
            return;
        }
        SilentAimProfile.Option option = SilentAimProfile.Option.values()[button.id / 2];
        try {
            profile.change(option, button.id % 2 == 0 ? -1 : 1);
            if (option.step == 0) {
                button.displayString = label(option);
            }
            error = null;
        } catch (IOException e) {
            error = "No se pudo guardar el ajuste.";
            e.printStackTrace();
        }
    }

    @Override
    public void drawScreen(int mouseX, int mouseY, float partial) {
        drawDefaultBackground();
        drawCenteredString(fontRendererObj, "Chest AI · SilentAim", width / 2, top, 0xFFFFFF);
        for (SilentAimProfile.Option option : SilentAimProfile.Option.values()) {
            int y = rowY(option);
            drawString(fontRendererObj, option.label, left, y, 0xEEEEEE);
            if (option.step != 0) {
                drawCenteredString(fontRendererObj, label(option), left + 241, y, 0xFFFFFF);
            }
        }
        if (error != null) {
            drawCenteredString(fontRendererObj, error, width / 2, rowY(null) + 25, 0xFF7676);
        }
        super.drawScreen(mouseX, mouseY, partial);
    }
}
