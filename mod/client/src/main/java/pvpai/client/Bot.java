package pvpai.client;

import java.util.Random;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityOtherPlayerMP;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.entity.Entity;
import net.minecraft.network.Packet;
import net.minecraft.network.play.server.S08PacketPlayerPosLook;
import net.minecraft.network.play.server.S0EPacketSpawnObject;
import net.minecraft.network.play.server.S12PacketEntityVelocity;
import net.minecraft.network.play.server.S13PacketDestroyEntities;
import net.minecraft.network.play.server.S14PacketEntity;
import net.minecraft.network.play.server.S18PacketEntityTeleport;
import net.minecraft.network.play.server.S19PacketEntityStatus;
import pvpai.core.Agent;
import pvpai.core.Blocks;
import pvpai.core.Book;
import pvpai.core.Duel;
import pvpai.core.Inv;
import pvpai.core.Keys;
import pvpai.core.View;

/**
 * An agent at the keys of the game client for one duel. The game calls in
 * three places: around each packet it handles ({@link Wire}), at the start
 * of its tick, when the packets are in and the keys have not been read yet,
 * and at the end of its tick.
 */
public final class Bot implements Wire.Tap {
    /** What picks an action for what the agent is shown: a policy. */
    public interface Chooser {
        int[] choose(float[] obs);
    }

    private final Minecraft mc;
    private final EntityOtherPlayerMP other;
    private final Duel duel;
    private final Chooser chooser;
    private final Agent agent;
    private final Book book;
    private final Hands hands;
    private final Layout layout;
    private final Blocks blocks;
    /** Whether the agent may hold the right button with its sword in hand. */
    private boolean block = true;
    /** What the keys of the tick that runs turned the view by, in degrees. */
    private float turnedYaw, turnedPitch;
    private final Random rng = new Random();
    private final float[] obs = new float[Agent.OBS_DIM];

    /** The click delay as the last tick left it. */
    private int clickDelay;
    private View view;
    private int[] action;
    private Keys pressed;
    // As they were before the tick, for what is told after it.
    private double[] eyeBefore, saidBefore;
    private float saidYawBefore, saidPitchBefore;
    private int ticksBefore;

    /**
     * For the duel that starts now against this player. With a chooser the
     * agent plays; without one it only keeps its counts, and the keys are
     * handed in.
     */
    public Bot(Minecraft mc, EntityOtherPlayerMP other, Duel duel, double pingOwn, double pingOther, double cps, Chooser chooser) {
        this.mc = mc;
        this.other = other;
        this.duel = duel;
        this.chooser = chooser;
        double[] feet = Reader.pos(mc.thePlayer);
        this.agent = chooser == null ? null : new Agent(pingOwn, pingOther, cps, Hands.mouseFactor(mc.gameSettings), feet);
        this.book = new Book(feet);
        this.layout = Layout.of(mc.thePlayer.inventory);
        this.hands = new Hands(mc, layout);
        this.blocks = Reader.blocks(mc);
        int d = Reader.clickDelay(mc);
        this.clickDelay = d > 0 ? d + 1 : 0;
    }

    public EntityOtherPlayerMP other() {
        return other;
    }

    Layout layout() {
        return layout;
    }

    /** Whether the agent may block with its sword: a player may want to see it fight without. */
    public void block(boolean allowed) {
        block = allowed;
    }

    /** Whether both players are still there. */
    public boolean alive() {
        return mc.thePlayer != null && mc.theWorld != null && !other.isDead && mc.theWorld.getEntityByID(other.getEntityId()) == other;
    }

    /** The view, what the policy was shown and what it chose at the start of the tick that runs or ran last. */
    public View view() {
        return view;
    }

    public float[] shown() {
        return obs;
    }

    public int[] action() {
        return action;
    }

    // Packets.

    public void before(Packet<?> p) {
        EntityPlayerSP me = mc.thePlayer;
        if (me == null || mc.theWorld == null) {
            return;
        }
        if (p instanceof S12PacketEntityVelocity) {
            S12PacketEntityVelocity v = (S12PacketEntityVelocity) p;
            if (v.getEntityID() == me.getEntityId()) {
                book.velocity(v.getMotionX(), v.getMotionY(), v.getMotionZ());
            } else {
                book.thrownVelocity(v.getEntityID(), v.getMotionX(), v.getMotionY(), v.getMotionZ());
            }
        } else if (p instanceof S19PacketEntityStatus) {
            S19PacketEntityStatus s = (S19PacketEntityStatus) p;
            if (s.getOpCode() == 2) {
                Entity e = s.getEntity(mc.theWorld);
                if (e == me) {
                    book.hurt(true);
                } else if (e == other) {
                    book.hurt(false);
                }
            }
        } else if (p instanceof S08PacketPlayerPosLook) {
            book.moved();
        } else if (p instanceof S18PacketEntityTeleport) {
            S18PacketEntityTeleport t = (S18PacketEntityTeleport) p;
            if (t.getEntityId() == other.getEntityId()) {
                book.otherTeleported(Reader.target(other), t.getX(), t.getY(), t.getZ());
            }
        } else if (p instanceof S0EPacketSpawnObject) {
            S0EPacketSpawnObject s = (S0EPacketSpawnObject) p;
            if (s.getType() == 65 || s.getType() == 73) {
                book.spawned(s.getEntityID(), s.getType() == 65 ? Inv.KIND_PEARL : Inv.KIND_POTION, s.getX(), s.getY(), s.getZ());
            }
        } else if (p instanceof S13PacketDestroyEntities) {
            for (int id : ((S13PacketDestroyEntities) p).getEntityIDs()) {
                book.destroyed(id);
            }
        }
    }

    public void after(Packet<?> p) {
        if (mc.theWorld == null) {
            return;
        }
        if (p instanceof S14PacketEntity) {
            S14PacketEntity m = (S14PacketEntity) p;
            Entity e = m.getEntity(mc.theWorld);
            if (e != null && Reader.thrown(e)) {
                book.thrownMoved(e.getEntityId(), m.func_149062_c(), m.func_149061_d(), m.func_149064_e(), Reader.pos(e));
            }
        } else if (p instanceof S18PacketEntityTeleport) {
            S18PacketEntityTeleport t = (S18PacketEntityTeleport) p;
            Entity e = mc.theWorld.getEntityByID(t.getEntityId());
            if (e != null && Reader.thrown(e)) {
                book.thrownTeleported(e.getEntityId(), t.getX(), t.getY(), t.getZ(), Reader.pos(e));
            }
        }
    }

    // The tick.

    /** At the start of a tick: {@link #look} and then {@link #press}. */
    public Keys tickStart(Keys handed) {
        Keys k = look(handed);
        press();
        return k;
    }

    /**
     * Read the client and settle the keys of this tick: the agent's, or
     * the ones handed in if it only keeps its counts. Returns them.
     */
    public Keys look(Keys handed) {
        EntityPlayerSP me = mc.thePlayer;
        view = Reader.read(mc, other, clickDelay, layout);
        book.fill(view);
        eyeBefore = view.eye();
        saidBefore = Reader.said(me);
        saidYawBefore = Reader.saidYaw(me);
        saidPitchBefore = Reader.saidPitch(me);
        ticksBefore = me.ticksExisted;
        Keys k = handed;
        if (agent != null) {
            agent.beforeTick(view, blocks);
            agent.observe(view, blocks, duel, obs);
            action = chooser.choose(obs);
            if (!block && action.length > 7) {
                action[7] = 0;
            }
            k = agent.keys(view, duel, action, rng);
        }
        pressed = k.copy();
        return k;
    }

    /** Press the keys {@link #look} settled. */
    public void press() {
        EntityPlayerSP me = mc.thePlayer;
        float yaw = me.rotationYaw, pitch = me.rotationPitch;
        hands.press(pressed);
        turnedYaw = me.rotationYaw - yaw;
        turnedPitch = me.rotationPitch - pitch;
    }

    /** At the end of the tick: what it did goes to the counts. */
    public void tickEnd() {
        EntityPlayerSP me = mc.thePlayer;
        // A tick that belongs to the inventory screen reads no key.
        boolean screen = view.invOpen || pressed.inv != 0;
        boolean swung = !screen && pressed.attack && !view.usingItem;
        boolean attacked = swung && me.getLastAttackerTime() == ticksBefore;
        int pearls = 0, potions = 0;
        for (int r = 0; r < 9; r++) {
            int kind = Reader.item(me.inventory.mainInventory[r]);
            int i = layout.agent[r];
            if (kind == Inv.POTION) {
                potions |= 1 << i;
            } else if (kind == Inv.PEARL && i == Inv.SLOT_PEARL) {
                pearls = me.inventory.mainInventory[r].stackSize;
            }
        }
        boolean threwPearl = !screen && pearls < view.pearls;
        boolean threwPotion = !screen && (view.potions & ~potions) != 0;
        book.ticked(swung, attacked, threwPearl, threwPotion, eyeBefore, saidBefore, saidYawBefore, saidPitchBefore);
        if (agent != null) {
            boolean consuming = me.isUsingItem() && layout.agent[me.inventory.currentItem] != Inv.SLOT_SWORD;
            int holds = Reader.shown(other.getHeldItem());
            boolean otherConsuming = other.isEating() && (holds == Inv.DRINK || holds == Inv.FOOD);
            agent.ticked(pressed, duel, Reader.pos(me), consuming, otherConsuming, threwPearl, threwPotion);
        }
        clickDelay = Reader.clickDelay(mc);
        // The game draws the view between where it was a tick ago and where
        // it is. The mouse of a hand turns it a little on every frame; the
        // agent's turns it once a tick, before the game reads it, which on
        // a screen is twenty jumps a second. So the turn of this tick is
        // left to be drawn over the frames until the next one: what the
        // game goes by, the view itself, is where the keys put it.
        if (turnedYaw != 0.0F || turnedPitch != 0.0F) {
            me.prevRotationYaw = me.rotationYaw - turnedYaw;
            me.prevRotationPitch = me.rotationPitch - turnedPitch;
        }
    }

    /** Lets every key go, when the agent leaves the keys. */
    public void release() {
        hands.release();
    }
}
