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
import net.minecraft.util.MathHelper;
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
    private EntityOtherPlayerMP other;
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
    /** Ticks left in which the view turns to a new opponent the way a hand would, in place of the policy's turn. */
    private int glide;
    private static final int GLIDE_TICKS = 12;
    /** Degrees to the new opponent at which the policy takes the view back. */
    private static final float GLIDE_DONE = 8.0F;
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
    /** What writes the duel down, if anything: it only reads. */
    private Recorder recorder;

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
        this.agent = chooser == null ? null : new Agent(pingOwn, pingOther, cps, Hands.TRAINED_FACTOR, feet);
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

    /**
     * Another player is the opponent from now on: what was counted of the
     * one before is let go, the hit is reckoned for its armour, and the
     * view turns to it over a few ticks.
     */
    void target(EntityOtherPlayerMP o) {
        other = o;
        book.newOpponent(o.hurtTime);
        if (agent != null) {
            agent.newOpponent();
        }
        duel.hitHealth = Scout.hitHealth(mc.thePlayer, o, layout);
        glide = GLIDE_TICKS;
        if (recorder != null) {
            recorder.target(o);
        }
    }

    /** Have the duel written down by this recorder from now on. */
    void record(Recorder r) {
        recorder = r;
    }

    /**
     * The turn of this tick toward a new opponent, eased: about half of
     * what is left, at least two degrees and at most thirty-five, until the
     * crosshair is near it.
     */
    private void glide(EntityPlayerSP me) {
        double dx = other.posX - me.posX, dz = other.posZ - me.posZ;
        double dy = other.posY + other.getEyeHeight() * 0.8 - (me.posY + me.getEyeHeight());
        float yaw = (float) (Math.atan2(dz, dx) * 180.0 / Math.PI) - 90.0F;
        float pitch = (float) -(Math.atan2(dy, Math.sqrt(dx * dx + dz * dz)) * 180.0 / Math.PI);
        float ey = MathHelper.wrapAngleTo180_float(yaw - me.rotationYaw), ep = pitch - me.rotationPitch;
        if (Math.abs(ey) < GLIDE_DONE && Math.abs(ep) < GLIDE_DONE) {
            glide = 0;
            return;
        }
        float deg = 0.15F * Hands.TRAINED_FACTOR;
        pressed.dx = Math.round(ease(ey) / deg);
        pressed.dy = Math.round(ease(ep) / deg);
    }

    private static float ease(float e) {
        float a = Math.abs(e);
        return Math.signum(e) * Math.min(Math.max(a * 0.45F, Math.min(a, 2.0F)), 35.0F);
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
        if (recorder != null) {
            recorder.before(p);
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
            int kind = Reader.kindOfSpawn(s.getType());
            if (kind >= 0) {
                // A hook's packet carries the number of its angler.
                book.spawned(s.getEntityID(), kind, kind == Inv.KIND_HOOK && s.func_149009_m() == me.getEntityId(), s.getX(), s.getY(), s.getZ());
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
        if (recorder != null) {
            recorder.after(p);
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

    /** Applies the tick, leaving an inventory close until its last click is confirmed. */
    int tickStartForSession() {
        EntityPlayerSP player = mc.thePlayer;
        int oldSword = layout.real[Inv.SLOT_SWORD];
        layout.refresh(player.inventory);
        look(null);
        int requestedInventory = pressed.inv;
        if (requestedInventory >= Inv.SHUT + Inv.WINDOW_ROWS) {
            pressed.inv -= Inv.SHUT;
        }
        if (requestedInventory == 0 && !view.invOpen && layout.sword
                && player.inventory.currentItem == oldSword && layout.real[Inv.SLOT_SWORD] != oldSword) {
            pressed.slotKey = Inv.SLOT_SWORD + 1;
            pressed.attack = false;
            pressed.useItem = false;
        }
        if (glide > 0) {
            glide--;
            if (requestedInventory == 0 && !view.invOpen && agent != null && !agent.busy()) {
                glide(player);
            }
        }
        press(other);
        return requestedInventory;
    }

    /** Holds the inputs without stopping the agent's game-tick counters. */
    void waitForInventory() {
        hands.release();
        pressed = new Keys();
        pressed.inv = 1;
        turnedYaw = turnedPitch = 0.0F;
        if (agent != null) {
            agent.inventoryWaitTick();
        }
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
            k = agent.keys(view, blocks, duel, action, rng);
        }
        pressed = k.copy();
        return k;
    }

    /** Press the keys {@link #look} settled. */
    public void press() {
        press(null);
    }

    private void press(Entity opponent) {
        EntityPlayerSP me = mc.thePlayer;
        float yaw = me.rotationYaw, pitch = me.rotationPitch;
        hands.press(pressed, opponent);
        turnedYaw = me.rotationYaw - yaw;
        turnedPitch = me.rotationPitch - pitch;
    }

    /** At the end of the tick: what it did goes to the counts. */
    public void tickEnd() {
        EntityPlayerSP me = mc.thePlayer;
        // A tick that belongs to the inventory screen reads no key.
        boolean screen = pressed.inv != 0 || view.invOpen;
        boolean attacked = !screen && pressed.attack && !view.usingItem && me.getLastAttackerTime() == ticksBefore;
        // A right click this tick leaves the click delay at 4; with a rod
        // in hand it cast or reeled in, and ItemFishingRod swings the arm
        // either way.
        boolean usedRod = !screen && Reader.clickDelay(mc) == 4 && Reader.item(me.inventory.getCurrentItem()) == Inv.ROD;
        boolean swung = !screen && (pressed.attack && !view.usingItem || usedRod);
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
        // A snowball or an egg thrown: the client takes it off its stack at the click.
        boolean threwBall = false;
        if (!screen && (view.tool == Inv.SNOWBALL || view.tool == Inv.EGG)) {
            net.minecraft.item.ItemStack t = me.inventory.mainInventory[layout.real[view.toolSlot]];
            int left = Reader.item(t) == view.tool ? t.stackSize : 0;
            threwBall = left < view.toolCount;
        }
        book.ticked(swung, attacked, threwPearl, threwPotion, threwBall, eyeBefore, saidBefore, saidYawBefore, saidPitchBefore);
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
        if (recorder != null) {
            recorder.tick(view, action, attacked, swung, threwPearl, threwPotion, screen, other);
        }
    }

    /** Lets every key go, when the agent leaves the keys. */
    public void release() {
        hands.release();
    }
}
