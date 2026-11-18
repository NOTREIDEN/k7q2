package pvpai.client;

import io.netty.channel.Channel;
import io.netty.channel.ChannelHandlerContext;
import io.netty.channel.ChannelInboundHandlerAdapter;
import java.lang.reflect.Field;
import net.minecraft.client.Minecraft;
import net.minecraft.network.INetHandler;
import net.minecraft.network.NetworkManager;
import net.minecraft.network.Packet;
import net.minecraft.network.play.server.S08PacketPlayerPosLook;
import net.minecraft.network.play.server.S0EPacketSpawnObject;
import net.minecraft.network.play.server.S12PacketEntityVelocity;
import net.minecraft.network.play.server.S13PacketDestroyEntities;
import net.minecraft.network.play.server.S14PacketEntity;
import net.minecraft.network.play.server.S18PacketEntityTeleport;
import net.minecraft.network.play.server.S19PacketEntityStatus;
import net.minecraft.network.play.server.S32PacketConfirmTransaction;

/**
 * Sits in front of the game's own packet handler and, for the packets an
 * agent's counts go by, has them handled on the game's thread between a
 * call before and a call after. The game does the same with every packet
 * (it puts it in line for its own thread), so the order of all of them
 * stays as it was.
 */
public final class Wire extends ChannelInboundHandlerAdapter {
    /** Called on the game's thread around the game's own handling of a packet. */
    public interface Tap {
        void before(Packet<?> p);

        void after(Packet<?> p);
    }

    private static final String NAME = "pvpai";
    private static final Field CHANNEL = Names.field(NetworkManager.class, "channel", "field_150746_k");

    private final Minecraft mc;
    private final NetworkManager manager;
    private final Tap tap;

    private Wire(Minecraft mc, NetworkManager manager, Tap tap) {
        this.mc = mc;
        this.manager = manager;
        this.tap = tap;
    }

    /** Put a tap on a connection; one that is there already is taken off first. */
    public static void install(Minecraft mc, NetworkManager manager, Tap tap) {
        Channel ch = (Channel) Names.get(CHANNEL, manager);
        remove(manager);
        ch.pipeline().addBefore("packet_handler", NAME, new Wire(mc, manager, tap));
    }

    public static void remove(NetworkManager manager) {
        Channel ch = (Channel) Names.get(CHANNEL, manager);
        if (ch.pipeline().get(NAME) != null) {
            ch.pipeline().remove(NAME);
        }
    }

    static boolean wanted(Object p) {
        return p instanceof S12PacketEntityVelocity || p instanceof S19PacketEntityStatus || p instanceof S08PacketPlayerPosLook
            || p instanceof S18PacketEntityTeleport || p instanceof S0EPacketSpawnObject || p instanceof S14PacketEntity
            || p instanceof S13PacketDestroyEntities || p instanceof S32PacketConfirmTransaction;
    }

    @Override
    public void channelRead(final ChannelHandlerContext ctx, Object msg) throws Exception {
        if (!wanted(msg)) {
            ctx.fireChannelRead(msg);
            return;
        }
        final Packet<?> p = (Packet<?>) msg;
        mc.addScheduledTask(new Runnable() {
            public void run() {
                if (!ctx.channel().isOpen()) {
                    return;
                }
                tap.before(p);
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
