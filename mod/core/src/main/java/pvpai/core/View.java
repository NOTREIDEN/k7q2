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
        /** {@link Inv#KIND_PEARL}, {@link Inv#KIND_POTION}, a snowball, an egg or a hook ({@link Inv#KIND_HOOK}). */
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
    /** One bit per slot of the hotbar taken by something a duel has no use for. The duels of training have no such thing. */
    public int others;
    /** Potions of the other player seen so far, and own potions thrown that the server has not shown yet. */
    public int potionsSeen, potionsPending;
    public int food;
    public float saturation;
    /**
     * The tool of the kit, {@link Inv#EMPTY} for none: a fishing rod, snowballs
     * or eggs, its slot, how many (1 for a rod) and its wear.
     */
    public int tool = Inv.EMPTY, toolSlot, toolCount, toolWear;
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

    /** The numbers of {@link #list} before the pearls and potions in flight, of each of those, and after them. */
    public static final int HEAD = 101, PER_THROWN = 14, TAIL = 4;

    /**
     * The whole view as a row of numbers, in the order the simulator writes
     * its own (agent.rs, view). The slot and the ticks of an item in use
     * are told only while one is in use.
     */
    public double[] list() {
        double[] l = new double[HEAD + PER_THROWN * thrown.size() + TAIL];
        int at = 0;
        at = put(l, at, pos);
        at = put(l, at, motion);
        l[at++] = flag(onGround);
        l[at++] = flag(collidedH);
        l[at++] = fallDistance;
        l[at++] = yaw;
        l[at++] = pitch;
        l[at++] = flag(sprinting);
        l[at++] = jumpTicks;
        l[at++] = speed;
        l[at++] = flag(serverSprint);
        l[at++] = hurtTime;
        at = put(l, at, lastKb);
        l[at++] = ticksSinceKb;
        l[at++] = ticksSinceSwing;
        l[at++] = ticksSinceAttack;
        l[at++] = ticksSinceHurt;
        l[at++] = ticksSinceMoved;
        l[at++] = flag(usingItem);
        l[at++] = rightClickDelay;
        l[at++] = flag(flagUsing);
        l[at++] = held;
        l[at++] = pearls;
        l[at++] = potions;
        l[at++] = drinks;
        l[at++] = bottles;
        l[at++] = steak;
        l[at++] = potionsSeen;
        l[at++] = potionsPending;
        l[at++] = food;
        l[at++] = saturation;
        l[at++] = speedTicks;
        l[at++] = health;
        l[at++] = flag(consuming());
        l[at++] = usingItem ? useSlot : 0;
        l[at++] = usingItem ? useCount : 0;
        l[at++] = flag(invOpen);
        for (int k = 0; k < 27; k++) {
            l[at++] = rows[k];
        }
        at = put(l, at, oPos);
        at = put(l, at, oPrevPos);
        at = put(l, at, oTarget);
        l[at++] = oIncrements;
        at = put(l, at, oMin);
        at = put(l, at, oMax);
        l[at++] = flag(oOnGround);
        l[at++] = flag(oSprinting);
        l[at++] = oHurtTime;
        l[at++] = flag(oSwinging);
        l[at++] = oSwingProgress;
        l[at++] = oYaw;
        l[at++] = oPitch;
        l[at++] = oTicksSinceHurt;
        l[at++] = oTicksSinceJump;
        l[at++] = flag(oUsing);
        l[at++] = oItem;
        l[at++] = oHealth;
        l[at++] = flag(oFx);
        l[at++] = flag(onTarget);
        l[at++] = thrown.size();
        for (Thrown t : thrown) {
            l[at++] = t.id;
            l[at++] = t.kind;
            l[at++] = flag(t.mine);
            at = put(l, at, t.pos);
            at = put(l, at, t.motion);
            l[at++] = t.age;
            l[at++] = t.air;
            at = put(l, at, t.serverSide);
        }
        l[at++] = tool;
        l[at++] = toolSlot;
        l[at++] = toolCount;
        l[at++] = toolWear;
        return l;
    }

    private static double flag(boolean b) {
        return b ? 1.0 : 0.0;
    }

    private static int put(double[] l, int at, double[] v) {
        System.arraycopy(v, 0, l, at, 3);
        return at + 3;
    }
}
