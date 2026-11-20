package chestai.core;

import java.util.ArrayList;
import java.util.List;

/**
 * What the game client sees in one tick, in the terms of the simulator:
 * everything the observation is built from but the client's memory
 * ({@link Memory}) and the blocks ({@link Grid}).
 */
public final class Frame {
    /** Mob kinds, as the simulator numbers them. */
    public static final int ZOMBIE = 0, HORSEMAN = 1, GOLEM = 2, RIDER = 3;
    /** Chest states, as the simulator numbers them. */
    public static final int COOLDOWN = 0, READY = 1, ACTIVE = 2, CLAIMABLE = 3;
    /** Projectile kinds. */
    public static final int ARROW = 0, SNOWBALL = 1, PEARL = 2, POTION = 3;

    public static final class Mob {
        public int id, kind;
        public double x, y, z, mx, my, mz;
        public float hp, maxHp;
        public int hurtTime;
        public boolean mounted;
        public double hw, h;
    }

    public static final class Proj {
        public int kind;
        /** -1 the player's own, 0 or more a mob's (or anyone else's). */
        public int owner;
        public double x, y, z, mx, my, mz;
    }

    /** A hologram seen this tick over chest {@code chest} (an index of {@link Memory#chests}). */
    public static final class Holo {
        public int chest, state, secs;
    }

    public long tick;
    public double x, y, z, mx, my, mz;
    public float yaw, pitch;
    public boolean onGround, collidedH, sprinting;
    /** Air left (300 full), the head under water, in lava, on fire, the head in a block. */
    public int air = 300;
    public boolean headInWater, inLava, burning, inWall;
    public float hp, absorption;
    /** The food bar (of 20) and its saturation. */
    public int food = 20;
    public float saturation = 5.0f;
    public int regenLeft, slowLeft, blindLeft;
    public int gapples, pearls, potions, arrows, pearlCdLeft;
    /** The agent's slot (0 sword, 1 bow, 2 golden apple, 3 pearl, 4 potion), whether it holds the right button on an item and for how many ticks, and its click allowance. */
    public int slot;
    public boolean using;
    public int useTicks;
    public double tokens;
    public int speedAmp;
    public double pingMs, cps, punch, leashR;
    public boolean extreme;
    /** What the crosshair is on: a mob's id or -1, a chest's index or -1. */
    public int pickMob = -1, pickChest = -1;
    public final List<double[]> rivals = new ArrayList<double[]>();
    public final List<Mob> mobs = new ArrayList<Mob>();
    public final List<Holo> holos = new ArrayList<Holo>();
    public final List<Proj> projs = new ArrayList<Proj>();
}
