package pvpai.plugin;

/**
 * The knockback of a server, in one of the two ways the simulator knows
 * (sim/src/server.rs): the game's own with other numbers (KbProfile), or
 * built from the hit alone, as server plugins set it (KbPlugin).
 */
public final class Profile {
    /** The game's own, untouched. */
    public static final int VANILLA = 0;
    /** The game's own way with the numbers below. */
    public static final int GAME = 1;
    /** Built from the hit alone. */
    public static final int HIT = 2;

    public String name = "";
    public int kind = VANILLA;
    /** Ticks in which a player that was hit cannot be hit again are half of this (the game: 20). */
    public int hitDelay = 20;

    // The game's own way: the victim's motion is divided by friction, then
    // pushed away and up; a sprinting attacker or a Knockback sword adds
    // the extra along the attacker's view, once per level.
    public double friction = 2.0;
    public double horizontal = 0.4000000059604645;
    public double vertical = 0.4000000059604645;
    public double verticalLimit = 0.4000000059604645;
    public float extraHorizontal = 0.5F;
    public double extraVertical = 0.1;

    // From the hit alone: one pair of numbers for a victim on the ground
    // and one for a victim in the air.
    public double hGround = 0.4, hAir = 0.4, hSprint = 0.5;
    public double vGround = 0.36075, vAir = 0.24775, vSprint = 0.1;
    /**
     * true: the sprint part needs a sprint the server still has (a hit
     * ends it, as in the game). false: it goes by what the client last
     * said, so every hit of a sprinting attacker carries it.
     */
    public boolean stopSprint = true;
    /**
     * Once the victim is higher than this above the last ground it stood
     * on, the upward part is vAfterLimit until it lands. Less than zero:
     * no limit.
     */
    public double yLimit = 0.675;
    public double vAfterLimit = 0.0;
}
