package pvpai.client;

import java.lang.reflect.Field;
import java.util.concurrent.TimeUnit;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.gui.GuiScreen;
import net.minecraft.client.gui.inventory.GuiInventory;
import net.minecraft.inventory.Container;
import net.minecraft.network.play.server.S32PacketConfirmTransaction;

/** Follows the vanilla inventory click until the server confirms it. */
final class InventoryRefill {
    enum Step { PLAY, WAIT, CLOSE, REJECTED, TIMED_OUT }

    private static final Field TRANSACTION = Names.field(Container.class, "transactionID", "field_75150_e");
    private static final long CONFIRM_TIMEOUT = TimeUnit.SECONDS.toNanos(1);
    private static final int IDLE_TICKS = 8;

    private GuiScreen screen;
    private short action;
    private int windowId;
    private int idleTicks;
    private long sentAt;
    private boolean pending, accepted, rejected, closeAfterClick;

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
        if (pending) {
            return System.nanoTime() - sentAt >= CONFIRM_TIMEOUT ? Step.TIMED_OUT : Step.WAIT;
        }
        if (!player.onGround || Math.abs(player.motionX) >= 0.005 || Math.abs(player.motionZ) >= 0.005) {
            return Step.CLOSE;
        }
        if (accepted) {
            // Give the client a tick to apply the inventory changes before deciding what to move next.
            accepted = false;
            return closeAfterClick ? Step.CLOSE : Step.WAIT;
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
        action = number;
        windowId = container.windowId;
        sentAt = System.nanoTime();
        pending = true;
        this.closeAfterClick = closeAfterClick;
        idleTicks = 0;
    }

    private static short transaction(Container container) {
        return (Short) Names.get(TRANSACTION, container);
    }

    void confirmed(S32PacketConfirmTransaction packet) {
        if (pending && windowId == packet.getWindowId() && action == packet.getActionNumber()) {
            pending = false;
            accepted = packet.func_148888_e();
            rejected = !accepted;
        }
    }

    void reset() {
        screen = null;
        idleTicks = 0;
        pending = false;
        accepted = false;
        rejected = false;
        closeAfterClick = false;
    }
}
