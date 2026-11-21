package chestai.client;

import io.netty.channel.Channel;
import io.netty.channel.ChannelHandlerContext;
import io.netty.channel.ChannelInboundHandlerAdapter;
import java.lang.reflect.Field;
import net.minecraft.client.Minecraft;
import net.minecraft.network.INetHandler;
import net.minecraft.network.NetworkManager;
import net.minecraft.network.Packet;
import net.minecraft.network.play.server.S32PacketConfirmTransaction;

/**
 * The PvP AI's tap (pvpai.client.Wire), for the one packet a refill goes by: the server's answer
 * to an inventory click. It sits in front of the game's own packet handler and has the packet
 * handled on the game's thread followed by a call (the game does the same with every packet, so
 * the order of all of them stays as it was).
 */
final class Wire extends ChannelInboundHandlerAdapter {
    interface Tap {
        void after(S32PacketConfirmTransaction p);
    }

    private static final String NAME = "chestai";
    private static final Field CHANNEL = Names.field(NetworkManager.class, "channel", "field_150746_k");

    private final Minecraft mc;
    private final NetworkManager manager;
    private final Tap tap;

    private Wire(Minecraft mc, NetworkManager manager, Tap tap) {
        this.mc = mc;
        this.manager = manager;
        this.tap = tap;
    }

    /** Put a tap on a connection; one that is there already is taken off first. False if it cannot be. */
    static boolean install(Minecraft mc, NetworkManager manager, Tap tap) {
        Channel ch = channel(manager);
        if (ch == null || ch.pipeline().get("packet_handler") == null) {
            return false;
        }
        remove(manager);
        ch.pipeline().addBefore("packet_handler", NAME, new Wire(mc, manager, tap));
        return true;
    }

    static void remove(NetworkManager manager) {
        Channel ch = channel(manager);
        if (ch != null && ch.pipeline().get(NAME) != null) {
            ch.pipeline().remove(NAME);
        }
    }

    private static Channel channel(NetworkManager manager) {
        if (CHANNEL == null || manager == null) {
            return null;
        }
        try {
            return (Channel) CHANNEL.get(manager);
        } catch (IllegalAccessException e) {
            return null;
        }
    }

    @Override
    public void channelRead(final ChannelHandlerContext ctx, Object msg) throws Exception {
        if (!(msg instanceof S32PacketConfirmTransaction)) {
            ctx.fireChannelRead(msg);
            return;
        }
        final S32PacketConfirmTransaction p = (S32PacketConfirmTransaction) msg;
        mc.addScheduledTask(new Runnable() {
            public void run() {
                if (!ctx.channel().isOpen()) {
                    return;
                }
                handle(p, manager.getNetHandler());
                tap.after(p);
            }
        });
    }

    @SuppressWarnings("unchecked")
    private static <T extends INetHandler> void handle(Packet<T> p, INetHandler h) {
        p.processPacket((T) h);
    }
}
