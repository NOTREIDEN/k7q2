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
    /** Choices of each control: forward, strafe, jump, sprint key, attack, yaw, pitch, right button, pearl, and the rest of the hotbar. */
    public static final int[] ACT_DIMS = {3, 3, 2, 2, 2, 27, 29, 2, 2, 7};
    /** Mouse counts per tick. */
    public static final int[] YAW_BINS = {
        -377, -233, -144, -89, -55, -34, -21, -13, -8, -5, -3, -2, -1, 0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377,
    };
    public static final int[] PITCH_BINS = {
        -89, -55, -34, -21, -13, -8, -5, -3, -2, -1, 0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, -377, -233, -144, 144, 233, 377, -600, 600,
    };
    public static final int OBS_DIM = 151;
    private static final int TRAIL = 16;
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
    /** Where it is in the key presses of a pearl throw: 0 not throwing, 1 the right click comes next, 2 the sword comes next. */
    private int pearlStage;
    /** Own ticks until it may throw again, as it counts them itself. */
    private int pearlWait;
    private boolean askedPearl;
    /** The same for a potion throw, and what it asked for on its last tick. */
    private int potionStage;
    private boolean askedPotion;
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
    /** The drink (1) or the meal (2) it has its right button down for, and what it asked of the hotbar on its last tick. */
    private int consume, askedBar;
    /** Whether it was drinking or eating on its last tick, and for how many ticks in a row it has seen the other do so. */
    private boolean wasConsuming;
    private int oppUseTicks;
    /** The inventory: 0 the screen is shut, 1 open and the click it was opened for is still to come, 2 open and that click made; and the kind of potion it was opened for. */
    private int refill, refillKind = Inv.POTION;

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
        k.dx = YAW_BINS[pick(a[5], 27)];
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
    public Keys keys(View v, Duel duel, int[] action, Random rng) {
        Keys inp = decode(action);
        int bar = action.length > 9 ? action[9] : 0;
        Keys screen = refillKeys(v, bar);
        if (screen != null) {
            return screen;
        }
        consumeKeys(v, duel, bar, inp);
        potionKeys(v, bar == 1, inp);
        pearlKeys(v, action.length > 8 && action[8] != 0, inp);
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
        if (threwPearl) {
            // The server starts its wait when the throw reaches it, a
            // little later than this; two ticks more cover the difference.
            pearlWait = duel.pearlCooldown + 2;
        }
        prev = inp.copy();
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
        return v.pearls > 0 && pearlWait <= 0 && pearlStage == 0 && potionStage == 0 && consume == 0 && v.held == Inv.SLOT_SWORD;
    }

    /** Can the player throw a potion now? With the sword in hand, or on the tick after another potion. */
    private boolean potionReady(View v) {
        return v.potions != 0 && pearlStage == 0 && consume == 0 && (potionStage == 0 ? v.held == Inv.SLOT_SWORD : potionStage == 2);
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
        askedBar = Math.min(Math.max(choice, 0), 6);
        if (consume == 0) {
            int slot;
            if (choice == 2 && v.drinks != 0 && drinkDue(v, duel)) {
                slot = Integer.numberOfTrailingZeros(v.drinks);
            } else if (choice == 3 && v.steak > 0 && v.food < 20) {
                slot = Inv.SLOT_FOOD;
            } else {
                return;
            }
            if (v.held != Inv.SLOT_SWORD || pearlStage != 0 || potionStage != 0) {
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
        int want = choice == 5 ? Inv.POTION : choice == 6 ? Inv.DRINK : -1;
        Inv.Bar bar = Inv.Bar.of(v);
        int inv;
        if (refill == 0) {
            if (want < 0) {
                return null;
            }
            if (v.held != Inv.SLOT_SWORD || pearlStage != 0 || potionStage != 0 || consume != 0) {
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
        askedBar = Math.min(Math.max(choice, 0), 6);
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
     * clicks again at once.
     */
    private void potionKeys(View v, boolean want, Keys inp) {
        askedPotion = want;
        if (potionStage == 0) {
            if (want && potionReady(v)) {
                inp.slotKey = Integer.numberOfTrailingZeros(v.potions) + 1;
                inp.attack = false;
                if (prev.useItem) {
                    inp.useItem = false;
                    potionStage = 1;
                } else {
                    inp.useItem = true;
                    potionStage = 2;
                }
            }
        } else if (potionStage == 1) {
            inp.useItem = true;
            inp.attack = false;
            potionStage = 2;
        } else if (want && potionReady(v)) {
            inp.slotKey = Integer.numberOfTrailingZeros(v.potions) + 1;
            inp.attack = false;
            inp.useItem = true;
            inp.useAgain = prev.useItem;
        } else {
            inp.slotKey = Inv.SLOT_SWORD + 1;
            inp.useItem = false;
            potionStage = 0;
        }
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
     * come. Its own potion, seen for the first time: the ticks since it
     * threw it are how far behind the server's answers are. And what it
     * foresees for the potions and pearls that break on their next tick is
     * kept.
     */
    public void beforeTick(View v, Blocks blocks) {
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
     * client and of the duel. comboOwn and comboOther are the hits in a row
     * this player has landed, and the other.
     */
    public void observe(View v, Blocks blocks, Duel duel, int comboOwn, int comboOther, float[] o) {
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
        o[13] = clip(pos[0] / duel.arenaHalf, 1.5);
        o[14] = clip(pos[2] / duel.arenaHalf, 1.5);
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
        for (int i = 77; i < OBS_DIM; i++) {
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
            double[] land = Geo.pearlPreview(blocks, pos, v.yaw, v.pitch);
            double[] ll = local(yaw, land[0] - pos[0], land[2] - pos[2]);
            o[92] = clip(Math.hypot(ll[0], ll[1]) / 16.0, 4.0);
            o[93] = (float) Math.min((int) land[3], 60) / 30.0f;
            o[94] = clip((ll[0] - rf) / 8.0, 4.0);
            o[95] = clip((ll[1] - rr) / 8.0, 4.0);
        }
        // Health.
        boolean real = duel.realHealth;
        o[96] = flag(real);
        o[97] = v.health / MAX_HEALTH;
        o[98] = v.oHealth / MAX_HEALTH;
        o[99] = (v.health - v.oHealth) / MAX_HEALTH;
        o[100] = real ? duel.hitHealth / 4.0f : 0.0f;
        o[101] = Math.min((MAX_HEALTH - v.health) / POTION_HEAL, 2.5f);
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
            double[] from = {h[0], h[1], h[2]}, m = {h[3] * sp, h[4] * sp, h[5] * sp};
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
    }
}
