package pvpai.core;

import java.util.ArrayList;
import java.util.List;

/**
 * What one client holds when its packets are in and its tick is to come:
 * its own player, what the server sent it of the other player, and the
 * pearls and potions it sees in flight. Everything an agent goes by.
 */
public final class View {
    /** A pearl or a potion in flight, as this client has it. */
    public static final class Thrown {
        public long id;
        /** {@link Inv#KIND_PEARL} or {@link Inv#KIND_POTION}. */
        public int kind;
        /** Thrown by this client's own player. */
        public boolean mine;
        /** Where the client shows it and how it moves per tick. */
        public final double[] pos = new double[3], motion = new double[3];
        /** Ticks the client's copy has made, and server ticks its motion stands for. */
        public int age, air;
        /** Where the server has it. */
        public final double[] serverSide = new double[3];
    }

    // The player itself.
    public final double[] pos = new double[3], motion = new double[3];
    public boolean onGround, collidedH;
    public float fallDistance, yaw, pitch;
    public boolean sprinting;
    public int jumpTicks;
    /** The value of its movement speed attribute. */
    public double speed;
    /** Whether the server has been told it sprints. */
    public boolean serverSprint;
    public int hurtTime;
    /** The last velocity the server gave it, and ticks since. */
    public final double[] lastKb = new double[3];
    public int ticksSinceKb, ticksSinceSwing, ticksSinceAttack, ticksSinceHurt, ticksSinceMoved;
    public boolean usingItem;
    public int rightClickDelay;
    /** What the server last said about this player using an item. */
    public boolean flagUsing;
    /** The hotbar slot in hand, 0 to 8. */
    public int held;
    /** Pearls in slot 1, steaks in slot 8, and one bit per slot that holds a splash potion, a potion to drink, an empty bottle. */
    public int pearls, steak, potions, drinks, bottles;
    /** Potions of the other player seen so far, and own potions thrown that the server has not shown yet. */
    public int potionsSeen, potionsPending;
    public int food;
    public float saturation;
    /** Ticks of Speed left. */
    public int speedTicks;
    public float health;
    /** The slot of the item in use and the ticks left of it. */
    public int useSlot, useCount;
    public boolean invOpen;
    /** The three rows of the inventory: 0 empty, 1 a splash potion, 2 a potion to drink, more: that many bottles plus two. */
    public final int[] rows = new int[27];

    // The other player, as shown.
    public final double[] oPos = new double[3], oPrevPos = new double[3], oTarget = new double[3];
    /** Ticks in which it still moves towards {@link #oTarget}. */
    public int oIncrements;
    public final double[] oMin = new double[3], oMax = new double[3];
    public boolean oOnGround, oSprinting;
    public int oHurtTime;
    public boolean oSwinging;
    public int oSwingProgress;
    public float oYaw, oPitch;
    public int oTicksSinceHurt, oTicksSinceJump;
    public boolean oUsing;
    /** What it holds, an item of {@link Inv}. */
    public int oItem;
    public float oHealth;
    /** Particles of an effect show around it. */
    public boolean oFx;

    /** The crosshair is on the other player within reach. */
    public boolean onTarget;
    /** Oldest first. */
    public final List<Thrown> thrown = new ArrayList<Thrown>();

    /** Drinking or eating: using an item that is not the sword. */
    public boolean consuming() {
        return usingItem && useSlot != Inv.SLOT_SWORD;
    }

    public double[] eye() {
        return new double[] {pos[0], pos[1] + Geo.EYE_HEIGHT, pos[2]};
    }
}
