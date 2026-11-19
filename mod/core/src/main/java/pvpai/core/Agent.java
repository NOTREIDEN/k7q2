package pvpai.core;

import java.util.Random;

/**
 * What plays a player from outside the game: the part between the policy
 * and a client. It turns what the policy chose into the keys and buttons of
 * one client tick, remembers from tick to tick the little it needs for
 * that, and writes down what the policy is shown. A copy, line for line,
 * of the agent the policy was trained with (sim/src/agent.rs), and checked
 * against it number by number.
 *
 * Each tick: {@link #beforeTick} when the packets are in, {@link #observe}
 * for the policy, {@link #keys} for what it chose, and {@link #ticked}
 * once the client has made its tick with them.
 */
public final strictfp class Agent {
    /**
     * The pitches a potion thrown with a turn can be thrown at: choice k of
     * the last control is {@code 90 - 2 k} degrees.
     */
    public static final int THROW_PITCHES = 36;
    /**
     * Choices of each control: forward, strafe, jump, sprint key, attack,
     * yaw, pitch, right button, pearl, the rest of the hotbar, a flick of
     * the mouse, the pitch of a potion thrown with a turn, and the tool of
     * a kit that has one (nothing, use it, take the sword back).
     */
    public static final int[] ACT_DIMS = {3, 3, 2, 2, 2, 31, 29, 2, 2, 13, 6, THROW_PITCHES, 3};
    /**
     * The first of the choices of the hotbar control that throw a potion
     * with a turn of the view, and the mouse counts of each one's turn:
     * half a turn, a quarter to the right, a quarter to the left. The three
     * choices after those make the same turns for two potions: on tick
     * three the next potion's number key and a right click, the view kept
     * where the first one went.
     */
    public static final int BAR_TURN = 7;
    static final int[] BAR_TURNS = {1200, 600, -600};
    /** Degrees of one mouse count, and the ticks after the turn of a potion thrown with a turn that observation 152 counts down. */
    private static final float COUNT_DEG = 0.15f;
    private static final int TURN_SHOWN = 8;

    /** The pitch of choice k of the last control. */
    static float throwPitch(int k) {
        return 90.0f - 2.0f * (float) Math.min(Math.max(k, 0), THROW_PITCHES - 1);
    }
    /** Mouse counts a flick adds to the yaw of its tick: an eighth of a turn, a quarter either way, or half a turn. */
    public static final int[] FLICK_BINS = {0, -300, 300, -600, 600, 1200};
    /** Mouse counts per tick; the four large ones at the end are a quarter turn and a half turn each way. */
    public static final int[] YAW_BINS = {
        -377, -233, -144, -89, -55, -34, -21, -13, -8, -5, -3, -2, -1, 0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, -600, 600,
        -1200, 1200,
    };
    public static final int[] PITCH_BINS = {
        -89, -55, -34, -21, -13, -8, -5, -3, -2, -1, 0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, -377, -233, -144, 144, 233, 377, -600, 600,
    };
    /** The ground around a player, shown as a grid of columns along its view: blocks forward (negative: behind) and to the right. */
    public static final double[] GRID_FWD = {-2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0};
    public static final double[] GRID_RIGHT = {-3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0};
    /** Where the grid starts in an observation. */
    public static final int GRID_AT = 154;
    /** What a column with no ground near enough under the feet shows. */
    public static final double NO_GROUND = -6.0;
    /** Where the previews of a pearl thrown with the view turned first start in an observation, and the turns (degrees of yaw). */
    public static final int PEARL_AT = GRID_AT + GRID_FWD.length * GRID_RIGHT.length;
    public static final float[] PEARL_TURNS = {45.0f, -45.0f, 90.0f, -90.0f};
    /** Where what the last hit and the last second took of its health is in an observation, and how many of its own ticks that second is. */
    public static final int HURT_AT = PEARL_AT + 3 * PEARL_TURNS.length;
    public static final int HURT_WINDOW = 20;
    /**
     * Where the cobwebs start in an observation: whether its own box is in
     * one, whether the opponent's is, then the grid of {@link #webGrid}.
     */
    public static final int WEB_AT = HURT_AT + 2;
    /** Where the tool, the hooks and the snowballs and eggs start in an observation, and how many numbers they take. */
    public static final int TOOL_AT = WEB_AT + 2 + GRID_FWD.length * GRID_RIGHT.length;
    public static final int TOOL_OBS = 25;
    public static final int OBS_DIM = TOOL_AT + TOOL_OBS;
    private static final int TRAIL = 16;
    /**
     * A hurt of the other player counts as a hit of this one's for this
     * many ticks after a hit of its own went out: the server's answer takes
     * a ping.
     */
    private static final int HIT_WINDOW = 20;
    private static final float MAX_HEALTH = 20.0f, POTION_HEAL = 8.0f;

    /** What a potion will do: where it breaks, in how many ticks, and the health it gives this player and the other. */
    private static final class Fate {
        final double[] at;
        final int ticks;
        final float[] heal;

        Fate(double[] at, int ticks, float[] heal) {
            this.at = at;
            this.ticks = ticks;
            this.heal = heal;
        }
    }

    /** Where the two players are on the server on a potion's tick t from now, see {@link #potionFeet}. */
    private static final class Feet {
        double[] there, here;
        double[] seen, own;
        double[][] trail;
        int back, on;
        boolean moving;
        double[] at(int j, int t) {
            if (j == 1) {
                double n = (double) (on + t);
                return new double[] {there[0] + seen[0] * n, there[1], there[2] + seen[1] * n};
            }
            int k = back - t;
            if (k >= 0) {
                return trail[Math.min(k, TRAIL - 1)];
            } else if (moving) {
                return new double[] {here[0] - own[0] * (double) k, here[1], here[2] - own[1] * (double) k};
            }
            return here;
        }
    }

    /** The ping of its own player and of the other, in milliseconds. */
    private final double pingOwn, pingOther;
    /** Its click rate and what is left of its budget. */
    private final double cps;
    private double clickTokens;
    /** Degrees a mouse count turns the view, over 0.15. */
    private final float mouseF1;
    /** The keys and buttons of its last tick. */
    private Keys prev = new Keys();
    /** The slot the tool was last seen in, -1 for none: snowballs or eggs used up in hand leave it empty, and the sword is taken back. */
    private int toolSlot = -1;
    /** Where it is in the key presses of a pearl throw: 0 not throwing, 1 the right click comes next, 2 the sword comes next. */
    private int pearlStage;
    /** Own ticks until it may throw again, as it counts them itself. */
    private int pearlWait;
    private boolean askedPearl;
    /** The same for a potion throw, and what it asked for on its last tick. */
    private int potionStage;
    private boolean askedPotion;
    /** A potion thrown with a turn: ticks since the turn, the tick of the turn being 1, and 0 when there is none or it is {@link #TURN_SHOWN} ago. */
    private int turned;
    /** Whether the potion thrown with the turn under way has a second one after it, on the tick after its click. */
    private boolean twice;
    /** Own ticks since its last potion throw. */
    private int sinceThrow = 1000;
    /**
     * Where it stood after each of its last ticks, newest first, and the
     * ticks from a click of its own to the first it sees of what the server
     * made of it: counted on its last potion, guessed from its ping before.
     */
    private final double[][] trail = new double[TRAIL][];
    private int lag;
    /**
     * The opponent's potion (0) and its own (1) that break on their next
     * tick: which one, and what it does. The same for pearls: which one and
     * where it lands.
     */
    private final long[] fateId = {-1, -1};
    private final Fate[] fate = new Fate[2];
    private final long[] pearlFateId = {-1, -1};
    private final double[][] pearlFate = new double[2][];
    /** The opponent's pearl it last saw appear, and its own ticks since. */
    private long oppPearl = -1;
    /**
     * Its health on its last tick (below zero before the first), what the
     * last fall of it took, and what each of its last {@link #HURT_WINDOW}
     * ticks took, oldest at hurtAt.
     */
    private float hpLast = -1.0f, hpDrop;
    private final float[] hpLost = new float[HURT_WINDOW];
    private int hurtAt;
    private int oppPearlAge = 100000;
    /** The drink (1) or the meal (2) it has its right button down for, and what it asked of the hotbar on its last tick. */
    private int consume, askedBar;
    /** Whether it was drinking or eating on its last tick, and for how many ticks in a row it has seen the other do so. */
    private boolean wasConsuming;
    private int oppUseTicks;
    /** The inventory: 0 the screen is shut, 1 open and the click it was opened for is still to come, 2 open and that click made; and the kind of potion it was opened for. */
    private int refill, refillKind = Inv.POTION;
    /**
     * The hits in a row it has landed and taken, as its own client can
     * tell: the other player hurt soon after a hit of its own, and itself
     * hurt other than by a pearl of its own coming down.
     */
    private int comboOwn, comboOther;
    /**
     * Where its player stood after its last tick and the one before: a
     * throw on the tick to come goes out with the movement between the
     * two, on a server that adds it.
     */
    private double[] said, saidBefore;
    /**
     * What it has seen of that: -1 not yet, 0 its potions fly as the view
     * throws them, 1 they take on its movement. Each potion of its own
     * thrown while moving tells ({@link #CARRY_MOVED}).
     */
    private int carry = -1;
    /**
     * The last potion click: how fast the view throws it across the ground
     * and the movement it goes out with (null with none), and the ticks since.
     */
    private double[] probe;
    private int probeAge;

    /** The least movement across the ground in a tick with which a potion of its own tells whether the server adds it. */
    private static final double CARRY_MOVED = 0.08;

    /**
     * An agent at the start of a duel: the two pings in milliseconds, its
     * click rate, the mouse factor of its sensitivity
     * ({@code f * f * f * 8} with {@code f = sensitivity * 0.6 + 0.2}), and
     * where its player stands.
     */
    public Agent(double pingOwn, double pingOther, double cps, float mouseF1, double[] feet) {
        this.pingOwn = pingOwn;
        this.pingOther = pingOther;
        this.cps = cps;
        this.mouseF1 = mouseF1;
        clickTokens = 1.0 + Math.min(cps / 20.0, 1.0);
        for (int i = 0; i < TRAIL; i++) {
            trail[i] = feet.clone();
        }
        lag = (int) Math.floor(pingOwn / 50.0) + 1;
        said = feet.clone();
        saidBefore = feet.clone();
    }

    /** Have it take the server to add its movement to its throws, or not, as if a potion of its own had told it (for checks). */
    public void knowsCarry(boolean c) {
        carry = c ? 1 : 0;
    }

    /** The movement across the ground a throw on the tick to come goes out with, if the server is known to add it. */
    private double[] carried() {
        return carry == 1 ? new double[] {said[0] - saidBefore[0], said[2] - saidBefore[2]} : new double[] {0.0, 0.0};
    }

    /**
     * A potion click on this tick: the throw the server will make of it
     * from the view it has, and the movement it goes out with, for the
     * potion to tell when it shows whether the server adds that.
     */
    private void probeThrow(View v) {
        double[] h = Geo.throwHeading(Inv.KIND_POTION, v.pos, v.yaw, v.pitch);
        double sp = Geo.speed(Inv.KIND_POTION);
        probe = new double[] {h[3] * sp, h[5] * sp, said[0] - saidBefore[0], said[2] - saidBefore[2]};
        probeAge = 0;
    }

    private static int pick(int v, int n) {
        return Math.min(Math.max(v, 0), n - 1);
    }

    private static final int[] AXIS = {0, 1, -1};

    /** The plain keys of an action. */
    static Keys decode(int[] a) {
        Keys k = new Keys();
        k.forward = AXIS[pick(a[0], 3)];
        k.strafe = AXIS[pick(a[1], 3)];
        k.jump = a[2] != 0;
        k.sprintKey = a[3] != 0;
        k.attack = a[4] != 0;
        k.dx = YAW_BINS[pick(a[5], 31)] + (a.length > 10 ? FLICK_BINS[pick(a[10], 6)] : 0);
        k.dy = PITCH_BINS[pick(a[6], 29)];
        k.useItem = a.length > 7 && a[7] != 0;
        return k;
    }

    /**
     * The keys and buttons of the tick to come for what the policy chose,
     * one number per control. An open inventory takes the whole tick;
     * otherwise a drink or a meal, a potion and a pearl are laid over the
     * plain keys in that order, and the click rate comes last.
     */
    public Keys keys(View v, Blocks blocks, Duel duel, int[] action, Random rng) {
        Keys inp = decode(action);
        int bar = action.length > 9 ? action[9] : 0;
        if (turned > 0) {
            turned++;
            if (turned == 2) {
                // The click of a potion thrown with a turn: nothing else of the hotbar.
                bar = 0;
            } else if (turned == 3 && twice) {
                // The second of two: the next potion, the view kept still.
                bar = 1;
                inp.dx = 0;
                inp.dy = 0;
            } else if (turned > TURN_SHOWN) {
                turned = 0;
            }
            if (turned >= 3) {
                twice = false;
            }
        }
        int turns = BAR_TURNS.length;
        int turn = bar >= BAR_TURN ? BAR_TURNS[Math.min(bar - BAR_TURN, 2 * turns - 1) % turns] : 0;
        boolean two = bar >= BAR_TURN + turns;
        Keys screen = refillKeys(v, bar);
        if (screen != null) {
            return screen;
        }
        consumeKeys(v, duel, bar, inp);
        float pitch = throwPitch(action.length > 11 ? action[11] : 0);
        potionKeys(v, bar == 1 || turn != 0, turn, pitch, inp);
        if (turn != 0 && turned == 1) {
            twice = two;
        }
        pearlKeys(v, action.length > 8 && action[8] != 0, inp);
        toolKeys(v, action.length > 12 ? action[12] : 0, inp);
        limitClicks(inp, rng);
        return inp;
    }

    /**
     * Called after the client has made its tick with these keys: where the
     * player now stands, whether it is drinking or eating, whether the
     * other is seen using a drink or food, and whether the tick threw a
     * pearl or a potion.
     */
    public void ticked(Keys inp, Duel duel, double[] feet, boolean consuming, boolean oppConsuming, boolean threwPearl, boolean threwPotion) {
        if (threwPotion) {
            sinceThrow = 0;
        }
        System.arraycopy(trail, 0, trail, 1, TRAIL - 1);
        trail[0] = feet.clone();
        if (sinceThrow < 1000) {
            sinceThrow++;
        }
        wasConsuming = consuming;
        oppUseTicks = oppConsuming ? oppUseTicks + 1 : 0;
        if (oppPearlAge < 100000) {
            oppPearlAge++;
        }
        if (threwPearl) {
            // The server starts its wait when the throw reaches it, a
            // little later than this; two ticks more cover the difference.
            pearlWait = duel.pearlCooldown + 2;
        }
        prev = inp.copy();
    }

    /**
     * Another player is the opponent from now on: what it counted of the
     * one before (its hits in a row on it, how long it was seen drinking,
     * its last pearl) is not this one's.
     */
    public void newOpponent() {
        comboOwn = 0;
        oppUseTicks = 0;
        oppPearlAge = 100000;
    }

    /** Whether it is in the middle of a throw, a drink, a meal or the inventory, whose keys must not be changed. */
    public boolean busy() {
        return potionStage != 0 || pearlStage != 0 || turned > 0 || consume != 0 || refill != 0;
    }

    /** Time passes while the inventory waits for a server reply. */
    public void inventoryWaitTick() {
        if (pearlWait > 0) {
            pearlWait--;
        }
    }

    /** Clicks the player can still make on its next tick. */
    private double clicksAvailable() {
        double r = Math.min(cps / 20.0, 1.0);
        return Math.min(clickTokens + r, 1.0 + r);
    }

    /** Apply the click rate: drop a click the budget does not cover, or add the second click of a tick for a rate above twenty. */
    private void limitClicks(Keys inp, Random rng) {
        double c = cps;
        if (c >= 20.0) {
            if (inp.attack && c > 20.0 && rng.nextDouble() < (c - 20.0) / 20.0) {
                inp.extraClicks = 1;
            }
            return;
        }
        double t = clicksAvailable();
        if (inp.attack && t >= 1.0 - 1e-9) {
            clickTokens = t - 1.0;
        } else {
            inp.attack = false;
            clickTokens = t;
        }
    }

    /** Can the player throw a pearl now, as far as it can tell itself? */
    private boolean pearlReady(View v) {
        return v.pearls > 0 && pearlWait <= 0 && pearlStage == 0 && potionStage == 0 && consume == 0 && handFree(v);
    }

    /** The sword in hand, or the tool of the kit: a pearl, a potion, a drink or a meal can be started from either. */
    private static boolean handFree(View v) {
        return v.held == Inv.SLOT_SWORD || (v.tool != Inv.EMPTY && v.held == v.toolSlot);
    }

    /**
     * The key presses of the tool: choice 1 uses it, 2 takes the sword
     * back. With the sword in hand and nothing else under way, 1 presses
     * the number key of the tool with a right click; with the tool in
     * hand, the button is let go and pressed again between the ticks, a
     * click out of the budget of the left ones, or with none left stays
     * down. 2 presses the number key of the sword. Snowballs or eggs used
     * up in hand leave the slot empty, and the sword is taken back.
     */
    private void toolKeys(View v, int choice, Keys inp) {
        if (potionStage != 0 || pearlStage != 0 || consume != 0 || refill != 0 || inp.slotKey != 0) {
            return;
        }
        if (v.tool == Inv.EMPTY) {
            if (toolSlot >= 0 && v.held == toolSlot) {
                inp.slotKey = Inv.SLOT_SWORD + 1;
                inp.useItem = false;
                toolSlot = -1;
            }
            return;
        }
        toolSlot = v.toolSlot;
        boolean holding = v.held == v.toolSlot;
        if (choice == 1 && !holding && v.held == Inv.SLOT_SWORD) {
            inp.slotKey = v.toolSlot + 1;
            inp.useItem = true;
            inp.useAgain = prev.useItem;
            inp.attack = false;
        } else if (choice == 1 && holding) {
            inp.useItem = true;
            if (prev.useItem) {
                inp.useAgain = takeClick();
            }
            if (inp.useAgain || !prev.useItem) {
                inp.attack = false;
            }
        } else if (choice == 2 && holding) {
            inp.slotKey = Inv.SLOT_SWORD + 1;
            inp.useItem = false;
        }
    }

    /** Can the player throw a potion now? With the sword in hand, or on the tick after another potion. */
    private boolean potionReady(View v) {
        return v.potions != 0 && pearlStage == 0 && consume == 0 && (potionStage == 0 ? handFree(v) : potionStage == 2);
    }

    /** Is the player without Speed or in the last seconds of it? */
    private boolean drinkDue(View v, Duel duel) {
        return duel.drinkWindowS < 0.0 || v.speedTicks <= duel.drinkWindow();
    }

    /**
     * The key presses of a drink or a meal. choice is what was asked of the
     * rest of the hotbar: 2 to drink (taken only when due), 3 to eat, 4 to
     * stop. Asked with the sword in hand, the number key of the potion of
     * Speed or of the steak is pressed with a right click, and the button
     * stays down on the ticks that follow until the client is no longer
     * using the item or the player stops; then the button is let go and
     * the number key of the sword pressed.
     */
    private void consumeKeys(View v, Duel duel, int choice, Keys inp) {
        askedBar = Math.min(Math.max(choice, 0), ACT_DIMS[9] - 1);
        if (consume == 0) {
            int slot;
            if (choice == 2 && v.drinks != 0 && drinkDue(v, duel)) {
                slot = Integer.numberOfTrailingZeros(v.drinks);
            } else if (choice == 3 && v.steak > 0 && v.food < 20) {
                slot = Inv.SLOT_FOOD;
            } else {
                return;
            }
            if (!handFree(v) || pearlStage != 0 || potionStage != 0) {
                return;
            }
            inp.slotKey = slot + 1;
            inp.attack = false;
            inp.useItem = true;
            inp.useAgain = prev.useItem;
            consume = choice - 1;
            wasConsuming = false;
        } else if ((wasConsuming && !v.consuming()) || choice == 4 || v.held == Inv.SLOT_SWORD) {
            inp.slotKey = Inv.SLOT_SWORD + 1;
            inp.useItem = false;
            consume = 0;
        } else {
            inp.useItem = true;
            inp.attack = false;
            // The click that did not start a use (a full stomach, as the
            // server knows before the client): give up.
            if (!v.consuming() && !wasConsuming && prev.slotKey == 0) {
                inp.slotKey = Inv.SLOT_SWORD + 1;
                inp.useItem = false;
                consume = 0;
            }
        }
    }

    /** One click of the mouse on a slot of the inventory, out of the same budget as the clicks with the sword. */
    private boolean takeClick() {
        if (cps >= 20.0) {
            return true;
        }
        double t = clicksAvailable();
        boolean can = t >= 1.0 - 1e-9;
        clickTokens = can ? t - 1.0 : t;
        return can;
    }

    /**
     * The inventory: choice 5 asks for a healing potion from its rows and 6
     * for a potion of Speed. Asked with the screen shut, the sword in hand
     * and nothing else under way, the inventory key is pressed. On the next
     * tick the potion is shift-clicked, whatever is asked then (an empty
     * bottle first when the hotbar is full). From then on every tick that
     * asks for a potion clicks another one down, and a tick that asks for
     * anything else shuts the screen. Returns the keys of a tick that
     * belongs to the screen, or null.
     */
    private Keys refillKeys(View v, int choice) {
        if (refill != 0 && !v.invOpen) {
            refill = 0;
            return null;
        }
        int want = choice == 5 ? Inv.POTION : choice == 6 ? Inv.DRINK : -1;
        Inv.Bar bar = Inv.Bar.of(v);
        int inv;
        if (refill == 0) {
            if (want < 0) {
                return null;
            }
            if (!handFree(v) || pearlStage != 0 || potionStage != 0 || consume != 0) {
                return null;
            }
            if (Inv.refillClick(bar, v.rows, want) < 0) {
                return null;
            }
            refill = 1;
            refillKind = want;
            clickTokens = clicksAvailable();
            inv = 1;
        } else {
            int kind = refill == 1 ? refillKind : want;
            int slot = kind < 0 ? -1 : Inv.refillClick(bar, v.rows, kind);
            if (slot < 0) {
                refill = 0;
                clickTokens = clicksAvailable();
                inv = 0;
            } else if (takeClick()) {
                int[] r = v.rows.clone();
                Inv.shiftClick(bar, r, slot);
                boolean more = Inv.refillClick(bar, r, Inv.POTION) >= 0 || Inv.refillClick(bar, r, Inv.DRINK) >= 0;
                if (!more) {
                    refill = 0;
                    inv = slot + Inv.SHUT;
                } else {
                    // A bottle put away is not yet the potion.
                    if (slot < Inv.WINDOW_BAR) {
                        refill = 2;
                    }
                    inv = slot;
                }
            } else {
                inv = 1;
            }
        }
        askedBar = Math.min(Math.max(choice, 0), ACT_DIMS[9] - 1);
        askedPotion = false;
        askedPearl = false;
        if (pearlWait > 0) {
            pearlWait--;
        }
        Keys k = new Keys();
        k.inv = inv;
        return k;
    }

    /**
     * The key presses of a potion throw: the number key of the first slot
     * that holds a potion and a right click, which throws along the view
     * the server already has, then the number key of the sword. Asked for
     * again on the tick after a throw, it takes the next potion in hand and
     * clicks again at once. Thrown with a turn, the first tick turns the
     * view and sets its pitch to the one asked for, and the click is made
     * on the second, along the view the server has by then.
     */
    private void potionKeys(View v, boolean want, int turn, float pitch, Keys inp) {
        askedPotion = want;
        if (potionStage == 0) {
            if (want && potionReady(v)) {
                inp.slotKey = Integer.numberOfTrailingZeros(v.potions) + 1;
                inp.attack = false;
                if (turn != 0) {
                    inp.dx += turn;
                    inp.dy = counts(pitch - v.pitch);
                    potionStage = 1;
                    turned = 1;
                } else if (prev.useItem) {
                    inp.useItem = false;
                    potionStage = 1;
                } else {
                    inp.useItem = true;
                    potionStage = 2;
                    probeThrow(v);
                }
            }
        } else if (potionStage == 1) {
            inp.useItem = true;
            inp.attack = false;
            potionStage = 2;
            probeThrow(v);
        } else if (want && potionReady(v)) {
            inp.slotKey = Integer.numberOfTrailingZeros(v.potions) + 1;
            inp.attack = false;
            inp.useItem = true;
            inp.useAgain = prev.useItem;
            probeThrow(v);
        } else {
            inp.slotKey = Inv.SLOT_SWORD + 1;
            inp.useItem = false;
            potionStage = 0;
        }
    }

    /** The mouse counts of a turn by deg degrees, to the nearest. */
    private static int counts(float deg) {
        return (int) Math.floor(deg / COUNT_DEG + 0.5f);
    }

    /**
     * The key presses of a pearl throw. Tick one: the number key of the
     * pearls and a right click. Tick two: the number key of the sword. If
     * the right button was down, it is let go on tick one and the click
     * comes a tick later.
     */
    private void pearlKeys(View v, boolean want, Keys inp) {
        if (pearlWait > 0) {
            pearlWait--;
        }
        askedPearl = want;
        if (pearlStage == 0) {
            if (want && pearlReady(v)) {
                inp.slotKey = Inv.SLOT_PEARL + 1;
                inp.attack = false;
                if (prev.useItem) {
                    inp.useItem = false;
                    pearlStage = 1;
                } else {
                    inp.useItem = true;
                    pearlStage = 2;
                }
            }
        } else if (pearlStage == 1) {
            inp.useItem = true;
            inp.attack = false;
            pearlStage = 2;
        } else {
            inp.slotKey = Inv.SLOT_SWORD + 1;
            inp.useItem = false;
            pearlStage = 0;
        }
    }

    /**
     * Where the two players are on the server while a potion flies there,
     * as this client can tell: its own player (0) and the other (1) on the
     * potion's tick t from now. Of the other it has what the server last
     * sent and takes it to go on as it does, on ticks further on the
     * potion's next tick. Of itself it knows more than the server: back is
     * how many ticks ago it stood where the server has it on the potion's
     * next tick; from now on it keeps moving as it does, or stops.
     */
    private Feet potionFeet(View v, int back, int on, boolean moving) {
        Feet f = new Feet();
        double sx = v.oPos[0] - v.oPrevPos[0], sz = v.oPos[2] - v.oPrevPos[2];
        f.seen = Math.hypot(sx, sz) < 1.5 ? new double[] {sx, sz} : new double[] {0.0, 0.0};
        double fr = v.onGround ? 0.546 : 0.91;
        f.own = new double[] {v.motion[0] / fr, v.motion[2] / fr};
        f.there = v.oIncrements > 0 ? v.oTarget : v.oPos;
        f.here = v.pos;
        f.trail = trail;
        f.back = back;
        f.on = on;
        f.moving = moving;
        return f;
    }

    /**
     * What a potion at pos moving by m per tick and age ticks old will do.
     * thrower is the one who threw it (0 this player, 1 the other), whom it
     * passes through for its first ticks.
     */
    private static Fate potionOutcome(Blocks blocks, double[] from, double[] motion, int age, int thrower, Feet feet) {
        double g = Geo.gravity(Inv.KIND_POTION);
        double[] pos = from.clone(), m = motion.clone();
        for (int t = 0; t < 60; t++) {
            age++;
            double[] next = {pos[0] + m[0], pos[1] + m[1], pos[2] + m[2]};
            double[] end = next;
            boolean broke = false;
            double[] h = blocks.rayTrace(pos, next);
            if (h != null) {
                end = h;
                broke = true;
            }
            int direct = -1;
            double best = 0.0;
            for (int j = 0; j < 2; j++) {
                if (j == thrower && age < 5) {
                    continue;
                }
                double[] f = feet.at(j, t);
                double[] min = {f[0] - 0.6, f[1] - 0.3, f[2] - 0.6}, max = {f[0] + 0.6, f[1] + 2.1, f[2] + 0.6};
                double[] hit = Geo.intercept(min, max, pos, end);
                if (hit != null) {
                    double d = Geo.distSq(pos, hit);
                    if (direct < 0 || d < best) {
                        direct = j;
                        best = d;
                    }
                }
            }
            if (broke || direct >= 0) {
                float[] offered = {0.0f, 0.0f};
                for (int j = 0; j < 2; j++) {
                    double[] f = feet.at(j, t);
                    if (f[1] + 1.8 >= pos[1] - 2.0 && f[1] <= pos[1] + 2.25) {
                        offered[j] = Geo.splashHeal(pos, f, direct == j);
                    }
                }
                return new Fate(pos, t, offered);
            }
            pos = next;
            for (int a = 0; a < 3; a++) {
                m[a] *= (double) 0.99f;
            }
            m[1] -= g;
        }
        return new Fate(pos, 60, new float[] {0.0f, 0.0f});
    }

    /**
     * {@link #potionOutcome} for a potion this client sees in flight. What
     * was foreseen for the tick before it broke is kept, because the copy
     * flies on until the server says it is gone.
     */
    private Fate potionSeen(View v, Blocks blocks, View.Thrown e) {
        int i = e.mine ? 1 : 0;
        if (fate[i] != null && fateId[i] == e.id) {
            return new Fate(fate[i].at, 0, fate[i].heal);
        }
        Feet feet = potionFeet(v, lag + e.age - 1 - e.air, e.air - e.age + 1, true);
        return potionOutcome(blocks, e.serverSide, e.motion, e.air, e.mine ? 0 : 1, feet);
    }

    /** Where a pearl this client sees in flight lands and in how many ticks, kept from the tick before it lands as for a potion. */
    private double[] pearlSeen(Blocks blocks, View.Thrown e) {
        int i = e.mine ? 1 : 0;
        if (pearlFate[i] != null && pearlFateId[i] == e.id) {
            double[] at = pearlFate[i];
            return new double[] {at[0], at[1], at[2], 0};
        }
        return Geo.flight(blocks, e.kind, e.serverSide, e.motion);
    }

    private static View.Thrown newest(View v, boolean mine, int kind) {
        for (int i = v.thrown.size() - 1; i >= 0; i--) {
            View.Thrown e = v.thrown.get(i);
            if (e.mine == mine && e.kind == kind) {
                return e;
            }
        }
        return null;
    }

    /**
     * Called when the packets of this client are in and its tick is to
     * come. A hurt that came in since its last tick goes to the hits in a
     * row. Its own potion, seen for the first time: the ticks since it
     * threw it are how far behind the server's answers are. And what it
     * foresees for the potions and pearls that break on their next tick is
     * kept.
     */
    public void beforeTick(View v, Blocks blocks) {
        float h = v.health;
        float d = hpLast >= 0.0f ? Math.max(hpLast - h, 0.0f) : 0.0f;
        hpLast = h;
        if (d > 0.0f) {
            hpDrop = d;
        }
        hpLost[hurtAt] = d;
        hurtAt = (hurtAt + 1) % HURT_WINDOW;
        saidBefore = said;
        said = v.pos.clone();
        if (probe != null) {
            // Its potion as the server has it after its first tick: the
            // throw and what was added to it, slowed by the air once.
            View.Thrown seen = null;
            for (int i = v.thrown.size() - 1; i >= 0; i--) {
                View.Thrown t = v.thrown.get(i);
                if (t.mine && t.kind == Inv.KIND_POTION && t.age == 0) {
                    seen = t;
                    break;
                }
            }
            if (seen != null) {
                double n = probe[2] * probe[2] + probe[3] * probe[3];
                if (n > CARRY_MOVED * CARRY_MOVED) {
                    double r0 = seen.motion[0] / 0.99 - probe[0], r1 = seen.motion[2] / 0.99 - probe[1];
                    carry = (r0 * probe[2] + r1 * probe[3]) / n > 0.5 ? 1 : 0;
                }
                probe = null;
            } else if (probeAge >= 20) {
                probe = null;
            } else {
                probeAge++;
            }
        }
        if (v.oTicksSinceHurt == 0 && v.ticksSinceAttack <= HIT_WINDOW) {
            comboOwn++;
            comboOther = 0;
        }
        if (v.ticksSinceHurt == 0 && v.ticksSinceMoved != 0) {
            comboOwn = 0;
            comboOther++;
        }
        if (v.potionsPending == 0 && sinceThrow < TRAIL) {
            for (View.Thrown e : v.thrown) {
                if (e.mine && e.kind == Inv.KIND_POTION && e.age == 0) {
                    lag = Math.max(sinceThrow, 1);
                    break;
                }
            }
        }
        for (int i = 0; i < 2; i++) {
            boolean mine = i == 1;
            Fate f = null;
            long fid = -1;
            View.Thrown e = newest(v, mine, Inv.KIND_POTION);
            if (e != null) {
                Fate r = potionSeen(v, blocks, e);
                if (r.ticks == 0) {
                    f = r;
                    fid = e.id;
                }
            }
            double[] p = null;
            long pid = -1;
            e = newest(v, mine, Inv.KIND_PEARL);
            if (e != null) {
                double[] r = pearlSeen(blocks, e);
                if (r[3] == 0) {
                    p = new double[] {r[0], r[1], r[2]};
                    pid = e.id;
                }
            }
            fate[i] = f;
            fateId[i] = fid;
            pearlFate[i] = p;
            pearlFateId[i] = pid;
        }
        View.Thrown theirs = newest(v, false, Inv.KIND_PEARL);
        if (theirs != null && theirs.id != oppPearl) {
            oppPearl = theirs.id;
            oppPearlAge = 0;
        }
    }

    private static float clip(double v, double lim) {
        return (float) Geo.clamp(v, -lim, lim);
    }

    private static float flag(boolean b) {
        return b ? 1.0f : 0.0f;
    }

    /** x and z as seen along a yaw: forward and to the right. */
    private static double[] local(float yaw, double x, double z) {
        double r = (double) yaw * (Math.PI / 180.0);
        double s = Math.sin(r), c = Math.cos(r);
        return new double[] {-s * x + c * z, -c * x - s * z};
    }

    /** Yaw error, pitch error (degrees) and horizontal distance from an eye with this view to the nearest height of a box. */
    private static double[] aimError(double[] eye, float yaw, float pitch, double[] min, double[] max) {
        double cx = (min[0] + max[0]) * 0.5, cz = (min[2] + max[2]) * 0.5;
        double ty = Geo.clamp(eye[1], min[1] + 0.2, max[1] - 0.2);
        double[] to = Geo.anglesTo(eye, new double[] {cx, ty, cz});
        double dx = cx - eye[0], dz = cz - eye[2];
        double h = Math.sqrt(dx * dx + dz * dz);
        if (h < 1e-6) {
            return new double[] {0.0, 0.0, h};
        }
        return new double[] {Geo.wrapDeg(to[0] - (double) yaw), to[1] - (double) pitch, h};
    }

    /**
     * What the policy is shown: what the agent can know from its own
     * client and of the duel.
     */
    public void observe(View v, Blocks blocks, Duel duel, float[] o) {
        float yaw = v.yaw;
        double[] pos = v.pos;
        double[] eye = v.eye();
        double[] l = local(yaw, v.motion[0], v.motion[2]);
        o[0] = clip(l[0] / 0.5, 3.0);
        o[1] = clip(l[1] / 0.5, 3.0);
        o[2] = clip(v.motion[1] / 0.5, 3.0);
        o[3] = flag(v.onGround);
        o[4] = flag(v.collidedH);
        o[5] = flag(v.sprinting);
        o[6] = flag(v.serverSprint);
        o[7] = (float) v.hurtTime / 10.0f;
        o[8] = (float) v.jumpTicks / 10.0f;
        o[9] = Math.min(v.fallDistance / 3.0f, 2.0f);
        o[10] = v.pitch / 90.0f;
        double yr = (double) yaw * (Math.PI / 180.0);
        o[11] = (float) Math.sin(yr);
        o[12] = (float) Math.cos(yr);
        o[13] = clip((pos[0] - duel.centerX) / duel.arenaHalf, 1.5);
        o[14] = clip((pos[2] - duel.centerZ) / duel.arenaHalf, 1.5);
        o[15] = clip((pos[1] - duel.floorY) / 3.0, 2.0);
        o[16] = (float) v.speed / 0.2f;
        double[] k = local(yaw, v.lastKb[0], v.lastKb[2]);
        boolean kbOn = v.ticksSinceKb < 40;
        o[17] = kbOn ? clip(k[0] / 0.5, 3.0) : 0.0f;
        o[18] = kbOn ? clip(k[1] / 0.5, 3.0) : 0.0f;
        o[19] = kbOn ? clip(v.lastKb[1] / 0.5, 3.0) : 0.0f;
        o[20] = (float) Math.min(v.ticksSinceKb, 40) / 40.0f;
        o[21] = (float) Math.min(v.ticksSinceSwing, 20) / 20.0f;
        o[22] = (float) Math.min(v.ticksSinceAttack, 20) / 20.0f;

        double[] rel = {v.oPos[0] - pos[0], v.oPos[1] - pos[1], v.oPos[2] - pos[2]};
        double[] r = local(yaw, rel[0], rel[2]);
        double rf = r[0], rr = r[1];
        o[23] = clip(rf / 8.0, 4.0);
        o[24] = clip(rr / 8.0, 4.0);
        o[25] = clip(rel[1] / 3.0, 2.0);
        double[] aim = aimError(eye, yaw, v.pitch, v.oMin, v.oMax);
        o[26] = clip(aim[2] / 8.0, 4.0);
        o[27] = (float) (aim[0] / 180.0);
        o[28] = clip(aim[1] / 90.0, 2.0);
        // Fine-grained copies of the aim error for when it is small.
        o[29] = clip(aim[0] / 15.0, 1.0);
        o[30] = clip(aim[1] / 15.0, 1.0);
        double[] tgt = {v.oTarget[0] - v.oPos[0], v.oTarget[1] - v.oPos[1], v.oTarget[2] - v.oPos[2]};
        double[] t = local(yaw, tgt[0], tgt[2]);
        boolean inc = v.oIncrements > 0;
        o[31] = inc ? clip(t[0], 2.0) : 0.0f;
        o[32] = inc ? clip(t[1], 2.0) : 0.0f;
        o[33] = inc ? clip(tgt[1], 2.0) : 0.0f;
        o[34] = (float) v.oIncrements / 3.0f;
        double[] vel = {v.oPos[0] - v.oPrevPos[0], v.oPos[1] - v.oPrevPos[1], v.oPos[2] - v.oPrevPos[2]};
        double[] ov = local(yaw, vel[0], vel[2]);
        o[35] = clip(ov[0] / 0.5, 3.0);
        o[36] = clip(ov[1] / 0.5, 3.0);
        o[37] = clip(vel[1] / 0.5, 3.0);
        o[38] = flag(v.oOnGround);
        o[39] = flag(v.oSprinting);
        o[40] = (float) v.oHurtTime / 10.0f;
        o[41] = v.oSwinging ? ((float) Math.max(v.oSwingProgress, 0) + 1.0f) / 6.0f : 0.0f;
        double[] reye = {v.oPos[0], v.oPos[1] + Geo.EYE_HEIGHT, v.oPos[2]};
        double ry = Geo.anglesTo(reye, eye)[0];
        o[42] = (float) (Geo.wrapDeg(ry - (double) v.oYaw) / 180.0);
        o[43] = v.oPitch / 90.0f;
        o[44] = flag(v.onTarget);
        double[] near = {
            Geo.clamp(eye[0], v.oMin[0] - 0.1, v.oMax[0] + 0.1), Geo.clamp(eye[1], v.oMin[1] - 0.1, v.oMax[1] + 0.1), Geo.clamp(eye[2], v.oMin[2] - 0.1, v.oMax[2] + 0.1),
        };
        double reach = Geo.dist(eye, near);
        o[45] = clip(reach / 6.0, 3.0);
        o[46] = flag(reach <= 3.0);
        o[47] = clip((reach - 3.0) / 1.0, 1.0);
        o[48] = (float) Math.min(v.oTicksSinceHurt, 20) / 20.0f;
        o[49] = (float) Math.min(v.ticksSinceHurt, 20) / 20.0f;
        o[50] = (float) (pingOwn / 200.0);
        o[51] = (float) (pingOther / 200.0);
        double dpc = (double) mouseF1 * 0.15;
        o[52] = (float) prev.forward;
        o[53] = (float) prev.strafe;
        o[54] = flag(prev.jump);
        o[55] = flag(prev.sprintKey);
        o[56] = flag(prev.attack);
        o[57] = clip((double) prev.dx * dpc / 30.0, 2.0);
        o[58] = clip((double) prev.dy * dpc / 15.0, 1.0);
        o[59] = Math.min((float) comboOwn / 10.0f, 2.0f) - Math.min((float) comboOther / 10.0f, 2.0f);
        double avail = clicksAvailable();
        o[60] = flag(avail >= 1.0 - 1e-9);
        o[61] = (float) (avail / 2.0);
        o[62] = (float) (cps / 20.0);
        // Blocking: whether this client is blocking, how long until a held
        // button clicks again, whether the opponent is shown blocking, the
        // button on the last tick, and what the server last said about
        // this player's own block.
        o[63] = flag(v.usingItem);
        o[64] = (float) v.rightClickDelay / 4.0f;
        o[65] = flag(v.oUsing);
        o[66] = flag(prev.useItem);
        o[67] = flag(v.flagUsing);
        // Ender pearls.
        float cd = (float) Math.max(duel.pearlCooldown, 1);
        o[68] = flag(v.held == Inv.SLOT_SWORD);
        o[69] = (float) v.pearls / 16.0f;
        o[70] = flag(pearlReady(v));
        o[71] = Math.min((float) Math.max(pearlWait, 0) / cd, 1.0f);
        o[72] = (float) pearlStage / 2.0f;
        o[73] = flag(askedPearl);
        o[74] = flag(v.oItem == Inv.PEARL);
        o[75] = (float) Math.min(v.ticksSinceMoved, 20) / 20.0f;
        o[76] = (float) Math.min(v.oTicksSinceJump, 20) / 20.0f;
        for (int i = 77; i < TOOL_AT; i++) {
            o[i] = 0.0f;
        }
        // The newest pearl of its own in flight: where it will put this
        // player, in how many ticks, and how far from the opponent that is.
        View.Thrown e = newest(v, true, Inv.KIND_PEARL);
        if (e != null) {
            double[] land = pearlSeen(blocks, e);
            double[] ll = local(yaw, land[0] - pos[0], land[2] - pos[2]);
            o[77] = 1.0f;
            o[78] = clip(ll[0] / 8.0, 4.0);
            o[79] = clip(ll[1] / 8.0, 4.0);
            o[80] = clip((land[1] - pos[1]) / 3.0, 2.0);
            o[81] = (float) Math.min((int) land[3], 40) / 20.0f;
            o[82] = clip(Geo.dist(new double[] {land[0], land[1], land[2]}, v.oPos) / 8.0, 4.0);
        }
        // The newest pearl of the opponent: where it is, where it will put
        // the opponent, in how many ticks, and how far the view has to turn
        // to face that spot.
        e = newest(v, false, Inv.KIND_PEARL);
        if (e != null) {
            double[] land = pearlSeen(blocks, e);
            double[] pl = local(yaw, e.pos[0] - pos[0], e.pos[2] - pos[2]);
            double[] ll = local(yaw, land[0] - pos[0], land[2] - pos[2]);
            o[83] = 1.0f;
            o[84] = clip(pl[0] / 8.0, 4.0);
            o[85] = clip(pl[1] / 8.0, 4.0);
            o[86] = clip((e.pos[1] - pos[1]) / 3.0, 2.0);
            o[87] = clip(ll[0] / 8.0, 4.0);
            o[88] = clip(ll[1] / 8.0, 4.0);
            o[89] = clip((land[1] - pos[1]) / 3.0, 2.0);
            o[90] = (float) Math.min((int) land[3], 40) / 20.0f;
            double ly = Geo.anglesTo(eye, new double[] {land[0], land[1] + 1.0, land[2]})[0];
            o[91] = (float) (Geo.wrapDeg(ly - (double) yaw) / 180.0);
        }
        // A pearl thrown on this tick: how far it would carry this player,
        // in how many ticks, and where that is seen from the opponent.
        if (v.pearls > 0) {
            double[] land;
            if (carry == 1) {
                double[] h = Geo.throwHeading(Inv.KIND_PEARL, pos, v.yaw, v.pitch);
                double sp = Geo.speed(Inv.KIND_PEARL);
                double[] add = carried();
                land = Geo.flight(blocks, Inv.KIND_PEARL, new double[] {h[0], h[1], h[2]}, new double[] {h[3] * sp + add[0], h[4] * sp, h[5] * sp + add[1]});
            } else {
                land = Geo.pearlPreview(blocks, pos, v.yaw, v.pitch);
            }
            double[] ll = local(yaw, land[0] - pos[0], land[2] - pos[2]);
            o[92] = clip(Math.hypot(ll[0], ll[1]) / 16.0, 4.0);
            o[93] = (float) Math.min((int) land[3], 60) / 30.0f;
            o[94] = clip((ll[0] - rf) / 8.0, 4.0);
            o[95] = clip((ll[1] - rr) / 8.0, 4.0);
            // The same for a view turned first, to either side: where a
            // pearl would put this player as seen from the opponent, and in
            // how many ticks.
            for (int q = 0; q < PEARL_TURNS.length; q++) {
                float turned = v.yaw + PEARL_TURNS[q];
                double[] at;
                if (carry == 1) {
                    double[] h = Geo.throwHeading(Inv.KIND_PEARL, pos, turned, v.pitch);
                    double sp = Geo.speed(Inv.KIND_PEARL);
                    double[] add = carried();
                    at = Geo.flight(blocks, Inv.KIND_PEARL, new double[] {h[0], h[1], h[2]}, new double[] {h[3] * sp + add[0], h[4] * sp, h[5] * sp + add[1]});
                } else {
                    at = Geo.pearlPreview(blocks, pos, turned, v.pitch);
                }
                double[] lt = local(yaw, at[0] - pos[0], at[2] - pos[2]);
                int i = PEARL_AT + 3 * q;
                o[i] = clip((lt[0] - rf) / 8.0, 4.0);
                o[i + 1] = clip((lt[1] - rr) / 8.0, 4.0);
                o[i + 2] = (float) Math.min((int) at[3], 60) / 30.0f;
            }
        }
        // Health.
        boolean real = duel.realHealth;
        o[96] = flag(real);
        o[97] = v.health / MAX_HEALTH;
        o[98] = v.oHealth / MAX_HEALTH;
        o[99] = (v.health - v.oHealth) / MAX_HEALTH;
        o[100] = real ? duel.hitHealth / 4.0f : 0.0f;
        o[101] = Math.min((MAX_HEALTH - v.health) / POTION_HEAL, 2.5f);
        // How hard it is being hit: what the last fall of its health took
        // and what its last second took, in hearts over five.
        float lost = 0.0f;
        for (int w = 0; w < HURT_WINDOW; w++) {
            lost += hpLost[(hurtAt + w) % HURT_WINDOW];
        }
        o[HURT_AT] = Math.min(hpDrop / 10.0f, 2.0f);
        o[HURT_AT + 1] = Math.min(lost / 10.0f, 2.0f);
        // Potions.
        o[102] = (float) Integer.bitCount(v.potions) / 7.0f;
        o[103] = flag(potionReady(v));
        o[104] = (float) potionStage / 2.0f;
        o[105] = flag(askedPotion);
        o[106] = flag(v.oItem == Inv.POTION);
        o[107] = (float) Math.min(v.potionsSeen, 14) / 7.0f;
        // Its own newest potion in flight, then the opponent's: where it
        // will break, in how many ticks, and the health it will then give
        // this player and the opponent.
        for (int who = 0; who < 2; who++) {
            e = newest(v, who == 0, Inv.KIND_POTION);
            if (e == null) {
                continue;
            }
            Fate f = potionSeen(v, blocks, e);
            double[] ll = local(yaw, f.at[0] - pos[0], f.at[2] - pos[2]);
            int at = 108 + 7 * who;
            o[at] = 1.0f;
            o[at + 1] = clip(ll[0] / 4.0, 4.0);
            o[at + 2] = clip(ll[1] / 4.0, 4.0);
            o[at + 3] = clip((f.at[1] - pos[1]) / 3.0, 2.0);
            o[at + 4] = (float) Math.min(f.ticks, 40) / 20.0f;
            o[at + 5] = f.heal[0] / POTION_HEAL;
            o[at + 6] = f.heal[1] / POTION_HEAL;
        }
        // A potion thrown on this tick: how far from here it would break,
        // in how many ticks, and the health it would give this player if it
        // keeps moving as it does, if it stops, and the opponent.
        if (v.potions != 0) {
            double[] h = Geo.throwHeading(Inv.KIND_POTION, pos, v.yaw, v.pitch);
            double sp = Geo.speed(Inv.KIND_POTION);
            double[] add = carried();
            double[] from = {h[0], h[1], h[2]}, m = {h[3] * sp + add[0], h[4] * sp, h[5] * sp + add[1]};
            Fate f = potionOutcome(blocks, from, m, 0, 0, potionFeet(v, -1, lag + 1, true));
            Fate still = potionOutcome(blocks, from, m, 0, 0, potionFeet(v, -1, lag + 1, false));
            double[] ll = local(yaw, f.at[0] - pos[0], f.at[2] - pos[2]);
            o[122] = clip(ll[0] / 4.0, 4.0);
            o[123] = clip(ll[1] / 4.0, 4.0);
            o[124] = (float) Math.min(f.ticks, 40) / 20.0f;
            o[125] = f.heal[0] / POTION_HEAL;
            o[126] = still.heal[0] / POTION_HEAL;
            o[127] = f.heal[1] / POTION_HEAL;
        }
        // Speed that runs out, drinks and food.
        boolean speed = v.speedTicks > 0;
        o[128] = flag(speed);
        o[129] = speed ? (float) Math.min(v.speedTicks, 1800) / 1800.0f : 0.0f;
        o[130] = (float) Integer.bitCount(v.drinks) / 2.0f;
        o[131] = flag(v.steak > 0);
        o[132] = (float) v.food / 20.0f;
        o[133] = v.saturation / 20.0f;
        o[134] = flag(v.food > 6);
        Inv.Bar bar = Inv.Bar.of(v);
        int using = v.consuming() ? bar.item(v.useSlot) : Inv.EMPTY;
        o[135] = flag(using == Inv.DRINK);
        o[136] = flag(using == Inv.FOOD);
        o[137] = v.consuming() ? Math.max(0.0f, Math.min(1.5f, (float) (Inv.USE_TICKS - v.useCount) / (float) Inv.USE_TICKS)) : 0.0f;
        o[138] = (float) consume / 2.0f;
        o[139] = (float) askedBar / 4.0f;
        o[140] = flag(v.oFx);
        o[141] = flag(v.oUsing && v.oItem == Inv.DRINK);
        o[142] = flag(v.oUsing && v.oItem == Inv.FOOD);
        o[143] = (float) Math.min(oppUseTicks, 48) / (float) Inv.USE_TICKS;
        // The inventory.
        o[144] = flag(v.invOpen);
        o[145] = Math.min((float) Inv.count(v.rows, Inv.POTION) / 8.0f, 3.0f);
        o[146] = Math.min((float) Inv.count(v.rows, Inv.DRINK) / 2.0f, 3.0f);
        int free = 0;
        for (int s = 1; s < 9; s++) {
            if (bar.item(s) == Inv.EMPTY) {
                free++;
            }
        }
        o[147] = (float) free / 8.0f;
        o[148] = (float) Integer.bitCount(v.bottles) / 2.0f;
        o[149] = flag(refill == 1);
        // The Speed it has left out of the last seconds in which a drink is
        // taken: 1 with more than that, when a drink would not be.
        o[150] = duel.drinkWindowS < 0.0 ? flag(speed) : Math.min((float) Math.max(v.speedTicks, 0) / (float) Math.max(duel.drinkWindow(), 1), 1.0f);
        // How much of the server's wait between pearls the opponent still
        // has in front of it, counted from when its last pearl was seen.
        o[151] = Math.min((float) Math.max(duel.pearlCooldown - oppPearlAge, 0) / cd, 1.0f);
        // A potion thrown with a turn: the ticks left of the first TURN_SHOWN after the turn.
        o[152] = turned > 0 ? (float) (TURN_SHOWN + 1 - turned) / (float) TURN_SHOWN : 0.0f;
        // Whether its throws take on its movement: not seen yet, no, yes.
        o[153] = new float[] {0.5f, 0.0f, 1.0f}[carry + 1];
        // The ground around it: steps, walls, pits and drops.
        groundGrid(blocks, pos, yaw, o, GRID_AT);
        // Cobwebs: in one, the opponent in one, and those around it.
        o[WEB_AT] = flag(inWeb(blocks, pos));
        o[WEB_AT + 1] = flag(inWeb(blocks, v.oPos));
        webGrid(blocks, pos, yaw, o, WEB_AT + 2);
        toolObs(v, o, TOOL_AT);
    }

    /** {@code o[at + k] = } a place seen from the player: forward, to the right, up. */
    private static void rel(View v, double[] p, float[] o, int at) {
        double[] l = local(v.yaw, p[0] - v.pos[0], p[2] - v.pos[2]);
        o[at] = clip(l[0] / 8.0, 4.0);
        o[at + 1] = clip(l[1] / 8.0, 4.0);
        o[at + 2] = clip((p[1] - v.pos[1]) / 3.0, 2.0);
    }

    /**
     * The tool of its kit, the hooks out and the snowballs and eggs in
     * flight. A hook is where the server last said it is.
     */
    private static void toolObs(View v, float[] o, int at) {
        for (int i = 0; i < TOOL_OBS; i++) {
            o[at + i] = 0.0f;
        }
        if (v.tool == Inv.ROD) {
            o[at] = 1.0f;
            o[at + 3] = 1.0f - (float) v.toolWear / (float) Inv.ROD_MAX_WEAR;
        } else if (v.tool == Inv.SNOWBALL || v.tool == Inv.EGG) {
            o[at + (v.tool == Inv.SNOWBALL ? 1 : 2)] = 1.0f;
            o[at + 3] = Math.min((float) v.toolCount / 16.0f, 1.0f);
        }
        o[at + 4] = flag(v.tool != Inv.EMPTY && v.held == v.toolSlot);
        View.Thrown e = newest(v, true, Inv.KIND_HOOK);
        if (e != null) {
            o[at + 5] = 1.0f;
            o[at + 6] = (float) Math.min(e.age, 40) / 20.0f;
            rel(v, e.serverSide, o, at + 7);
            o[at + 10] = clip(Geo.dist(e.serverSide, v.oPos) / 4.0, 4.0);
        }
        o[at + 11] = flag(v.oItem == Inv.ROD);
        o[at + 12] = flag(v.oItem == Inv.SNOWBALL || v.oItem == Inv.EGG);
        e = newest(v, false, Inv.KIND_HOOK);
        if (e != null) {
            o[at + 13] = 1.0f;
            rel(v, e.serverSide, o, at + 14);
        }
        // The opponent's snowball or egg nearest to this player, and how it moves.
        View.Thrown near = null;
        double least = 0.0;
        for (View.Thrown t : v.thrown) {
            if (t.mine || (t.kind != Inv.KIND_SNOWBALL && t.kind != Inv.KIND_EGG)) {
                continue;
            }
            double d = Geo.distSq(t.pos, v.pos);
            if (near == null || d < least) {
                near = t;
                least = d;
            }
        }
        if (near != null) {
            o[at + 17] = 1.0f;
            rel(v, near.pos, o, at + 18);
            double[] m = local(v.yaw, near.motion[0], near.motion[2]);
            o[at + 21] = clip(m[0] / 2.0, 2.0);
            o[at + 22] = clip(m[1] / 2.0, 2.0);
            o[at + 23] = clip(near.motion[1] / 2.0, 2.0);
        }
        boolean ball = false;
        for (View.Thrown t : v.thrown) {
            ball |= t.mine && (t.kind == Inv.KIND_SNOWBALL || t.kind == Inv.KIND_EGG);
        }
        o[at + 24] = flag(ball);
    }

    /**
     * {@code Entity.doBlockCollisions} for cobwebs: whether a block whose
     * cell the box of a player standing at {@code feet} reaches into by more
     * than 0.001 is a cobweb.
     */
    public static boolean inWeb(Blocks blocks, double[] feet) {
        double hw = (double) 0.3f, h = (double) 1.8f;
        int x0 = Geo.floor(feet[0] - hw + 0.001), x1 = Geo.floor(feet[0] + hw - 0.001);
        int y0 = Geo.floor(feet[1] + 0.001), y1 = Geo.floor(feet[1] + h - 0.001);
        int z0 = Geo.floor(feet[2] - hw + 0.001), z1 = Geo.floor(feet[2] + hw - 0.001);
        for (int x = x0; x <= x1; x++) {
            for (int y = y0; y <= y1; y++) {
                for (int z = z0; z <= z1; z++) {
                    if (blocks.web(x, y, z)) {
                        return true;
                    }
                }
            }
        }
        return false;
    }

    /** The cobwebs in the column at (x, z) for feet at {@code feet}: a half for one at the level of the feet, a half for one at the head. */
    public static double webs(Blocks blocks, int x, int z, double feet) {
        int fy = Geo.floor(feet);
        return ((blocks.web(x, fy, z) ? 1.0 : 0.0) + (blocks.web(x, fy + 1, z) ? 1.0 : 0.0)) / 2.0;
    }

    /** The grid of {@link #webs} around a player at {@code pos} looking along {@code yaw}, laid out as {@link #groundGrid}. */
    public static void webGrid(Blocks blocks, double[] pos, float yaw, float[] o, int at) {
        double r = (double) yaw * (Math.PI / 180.0);
        double s = Math.sin(r), c = Math.cos(r);
        for (int i = 0; i < GRID_FWD.length; i++) {
            for (int j = 0; j < GRID_RIGHT.length; j++) {
                double f = GRID_FWD[i], rt = GRID_RIGHT[j];
                double x = pos[0] - s * f - c * rt;
                double z = pos[2] + c * f - s * rt;
                o[at + i * GRID_RIGHT.length + j] = (float) webs(blocks, Geo.floor(x), Geo.floor(z), pos[1]);
            }
        }
    }

    /**
     * Where the ground is in the column of blocks at (x, z) for feet at
     * height {@code feet}: the top of the highest block from the level of
     * the head down to five under the feet, relative to the feet;
     * {@link #NO_GROUND} for none (a deep pit, the void). 1 is a block to
     * jump onto, 0.5 a slab to walk onto, 2 a wall.
     */
    public static double ground(Blocks blocks, int x, int z, double feet) {
        int fy = Geo.floor(feet);
        for (int y = fy + 1; y >= fy - 5; y--) {
            double h = blocks.height(x, y, z);
            if (h > 0.0) {
                return Math.min((double) y + h - feet, 2.0);
            }
        }
        return NO_GROUND;
    }

    /** The grid of {@link #ground} around a player at {@code pos} looking along {@code yaw}, divided by 3, forward row by forward row. */
    public static void groundGrid(Blocks blocks, double[] pos, float yaw, float[] o, int at) {
        double r = (double) yaw * (Math.PI / 180.0);
        double s = Math.sin(r), c = Math.cos(r);
        for (int i = 0; i < GRID_FWD.length; i++) {
            for (int j = 0; j < GRID_RIGHT.length; j++) {
                double f = GRID_FWD[i], rt = GRID_RIGHT[j];
                // Forward is (-sin, cos), right is (-cos, -sin).
                double x = pos[0] - s * f - c * rt;
                double z = pos[2] + c * f - s * rt;
                double g = ground(blocks, Geo.floor(x), Geo.floor(z), pos[1]);
                o[at + i * GRID_RIGHT.length + j] = (float) (g / 3.0);
            }
        }
    }
}
