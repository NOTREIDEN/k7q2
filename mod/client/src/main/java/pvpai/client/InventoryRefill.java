package pvpai.client;

import java.lang.reflect.Field;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.gui.GuiScreen;
import net.minecraft.client.gui.inventory.GuiInventory;
import net.minecraft.inventory.Container;
import net.minecraft.network.play.server.S32PacketConfirmTransaction;

/**
 * Follows the clicks of a refill in the vanilla inventory screen. The
 * clicks go one a tick, as a player makes them and as in training, without
 * waiting for the server to confirm each one: the client has already moved
 * the stacks itself, and the server takes the clicks in the order they were
 * sent. Some servers never send the confirmation of a click they took, and
 * waiting for it there held the screen open for a second. A click the
 * server refuses still shuts the screen when its refusal comes.
 */
final class InventoryRefill {
    enum Step { PLAY, WAIT, CLOSE, REJECTED }

    private static final Field TRANSACTION = Names.field(Container.class, "transactionID", "field_75150_e");
    private static final int IDLE_TICKS = 8;

    private GuiScreen screen;
    /** The action number the container would give the next click, before this tick's keys. */
    private short action;
    private int windowId = -1;
    /** The action numbers of the clicks sent on this screen, first to last. */
    private short first, last;
    private boolean clicked;
    private int idleTicks;
    private boolean rejected, closeNext;

    Step step(GuiScreen current, EntityPlayerSP player) {
        if (!(current instanceof GuiInventory)) {
            reset();
            return Step.PLAY;
        }
        if (screen != current) {
            reset();
            screen = current;
            return Step.WAIT;
        }
        if (rejected) {
            return Step.REJECTED;
        }
        if (player.isSprinting()) {
            // The screen has let go of the keys and the sprint ends on the
            // next tick; the click waits for it. Sliding on, or in the air
            // after a hit, the clicks go on as in the game and in training:
            // closing for that only opened the screen again on the next
            // tick the policy asked for potions, over and over.
            return Step.WAIT;
        }
        if (closeNext) {
            // The last click went out on the tick before: shut the screen.
            closeNext = false;
            return Step.CLOSE;
        }
        if (++idleTicks > IDLE_TICKS) {
            return Step.CLOSE;
        }
        action = transaction(player.openContainer);
        return Step.PLAY;
    }

    void sent(GuiScreen current, Container container, boolean closeAfterClick) {
        if (screen == null || screen != current) {
            return;
        }
        short number = transaction(container);
        if (number == action) {
            return;
        }
        // The click of this tick took action numbers action .. number - 1.
        if (!clicked) {
            first = action;
            clicked = true;
        }
        last = (short) (number - 1);
        action = number;
        windowId = container.windowId;
        closeNext = closeAfterClick;
        idleTicks = 0;
    }

    private static short transaction(Container container) {
        return (Short) Names.get(TRANSACTION, container);
    }

    void confirmed(S32PacketConfirmTransaction packet) {
        if (clicked && !packet.func_148888_e() && windowId == packet.getWindowId()) {
            // One of this screen's clicks, counted the way the shorts wrap.
            short n = packet.getActionNumber();
            if ((short) (n - first) >= 0 && (short) (last - n) >= 0) {
                rejected = true;
            }
        }
    }

    void reset() {
        screen = null;
        idleTicks = 0;
        clicked = false;
        windowId = -1;
        rejected = false;
        closeNext = false;
    }
}
