package pvpai.core;

/** What an agent is told about the duel it plays. */
public final class Duel {
    /** Half the side of the arena, which is centred on the origin, and the height of its floor. */
    public double arenaHalf, floorY;
    /** Whether hits cost health, and the health a plain hit costs with the armour and swords of this duel. */
    public boolean realHealth;
    public float hitHealth;
    /** Server ticks a player waits between two pearls. */
    public int pearlCooldown;
    /** The last seconds of Speed in which a drink is taken (negative: at any time). */
    public double drinkWindowS;

    /** {@link #drinkWindowS} in ticks. */
    public int drinkWindow() {
        double x = drinkWindowS * 20.0;
        return (int) (x < 0 ? -Math.floor(-x + 0.5) : Math.floor(x + 0.5));
    }
}
