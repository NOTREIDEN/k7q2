package chestai.core;

/**
 * Turns the agent's choices of one tick into keys, the way the simulated
 * client does in {@code Game.client_tick}: the slot rules (a pearl or a
 * potion is thrown with its number key and a right click, and the slot
 * before is taken back on the next tick), the click allowance of at most
 * {@code cps} clicks a second, and the right button on a chest, the bow
 * or a golden apple. It keeps the state the observation reads (slot,
 * using, use ticks, allowance).
 */
public final class Pilot {
    public static final float MAX_DYAW = 10f, MAX_DPITCH = 7f;
    public static final float[] YAW_BINS = {-60f, -30f, -15f, -8f, -4f, -2f, -0.7f, 0f, 0.7f, 2f, 4f, 8f, 15f, 30f, 60f};
    public static final float[] PITCH_BINS = {-30f, -15f, -7f, -3f, -1f, 0f, 1f, 3f, 7f, 15f, 30f};
    public static final int SWORD = 0, BOW = 1, GAPPLE = 2, PEARL = 3, POTION = 4;
    public static final int EAT_TICKS = 32;

    /** The keys of one tick. */
    public static final class Keys {
        /** 1 forward, -1 back; 1 left, -1 right. */
        public int forward, strafe;
        public boolean jump, sprint;
        /** Degrees to turn the view by. */
        public float dyaw, dpitch;
        /** The agent's slot to hold this tick (the hand moves before the clicks). */
        public int slot;
        /** Throw what the slot holds: a right click on it. */
        public boolean throwIt;
        /** A left click. */
        public boolean attack;
        /** A right click on the chest the crosshair is on. */
        public boolean claim;
        /** The right button held on the bow or a golden apple. */
        public boolean useHeld;
    }

    public int slot = SWORD;
    public boolean using;
    public int useTicks;
    public double tokens;
    private int backTo = -1;
    public final double cps;

    public Pilot(double cps) {
        this.cps = cps;
    }

    /**
     * One tick. {@code a} are the agent's choices (forward, strafe, jump,
     * sprint, yaw, pitch, attack, use, slot); the rest is what the player
     * has and what the crosshair is on.
     */
    public Keys step(int[] a, int pearls, int pearlCdLeft, int potions, int arrows, int gapples, boolean pickMob, boolean pickChest) {
        Keys k = new Keys();
        k.forward = a[0] - 1;
        k.strafe = 1 - a[1];
        k.jump = a[2] == 1;
        k.sprint = a[3] == 1;
        // The view turns at most MAX_DYAW / MAX_DPITCH degrees a tick, as in the simulator.
        k.dyaw = Math.max(-MAX_DYAW, Math.min(MAX_DYAW, YAW_BINS[a[4]]));
        k.dpitch = Math.max(-MAX_DPITCH, Math.min(MAX_DPITCH, PITCH_BINS[a[5]]));
        boolean attack = a[6] == 1, use = a[7] == 1;
        int want = a[8] == 0 ? -1 : a[8] - 1;
        if (backTo >= 0) {
            slot = backTo;
            backTo = -1;
        }
        int thrown = -1;
        if (want >= 0 && want != slot && !using) {
            if (want >= PEARL) {
                boolean have = want == PEARL ? pearls > 0 && pearlCdLeft == 0 : potions > 0;
                if (have) {
                    thrown = want;
                    backTo = slot;
                    slot = want;
                }
            } else {
                slot = want;
            }
        }
        k.slot = slot;
        k.throwIt = thrown >= 0;
        tokens = Math.min(tokens + cps / 20.0, 1.0);
        if (attack && !using && tokens >= 1.0 && thrown < 0) {
            tokens -= 1.0;
            k.attack = true;
        }
        if (use && thrown < 0) {
            if (!using) {
                if (pickChest && !pickMob) {
                    k.claim = true;
                } else if ((slot == BOW && arrows > 0) || (slot == GAPPLE && gapples > 0)) {
                    using = true;
                    useTicks = 0;
                }
            }
        } else if (using) {
            using = false;
        }
        if (using) {
            useTicks++;
            if (slot == GAPPLE && useTicks >= EAT_TICKS) {
                using = false;
                // The game finishes the apple itself; the button stays down only if the agent asks again.
                k.useHeld = true;
                return k;
            }
        }
        k.useHeld = using;
        return k;
    }
}
