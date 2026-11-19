package pvpai.client;

import java.util.Locale;

/**
 * The numbers of one duel the way the evaluations of the simulator count
 * them, from what a {@link Recorder} made out of the client: hits and
 * the unanswered runs of them both ways, how much of each potion went
 * into health, the pearls and where they landed, and whether the player
 * died with potions left. Knows nothing of the game.
 */
final class Tally {
    /** Ticks after a hit within which the same player's next hit goes on with its run, for the second count of runs. */
    static final int GAP = 24;
    /** The health a splash potion of Instant Health II gives at full strength. */
    static final double POTION_HEAL = 8.0;
    /** How near the other player a pearl has to land to be told front, side or behind. */
    static final double NEAR_LAND = 6.0;

    int ticks;
    /** Hits by this player (0) and by the other (1). */
    final int[] hits = new int[2];
    /** Runs of hits by each player that the other did not answer: all of them, of three or more, the longest, and hits third or later in one. */
    final int[] runs = new int[2], runs3 = new int[2], longest = new int[2], third = new int[2];
    /** The same with a run also over after {@link #GAP} ticks without a hit. */
    final int[] gruns = new int[2], gruns3 = new int[2], glongest = new int[2];
    private int who = -1, len, gwho = -1, glen, glast;

    /** Own healing potions: thrown, thrown with fewer than eight points missing, and the points that could not fit counting the ones on the way. */
    int potions, early;
    double unfit;
    /** Of those that broke: the health they gave, what they did not offer (broke away), what they offered beyond the health missing. */
    int broke, capped, mixed;
    double given, far, over;
    /** Pearls thrown, landed, and where: near the other player in front, to a side, behind, or further away; facing its sword. */
    int pearls, landings, front, side, back, away, faced;
    /** Healing potions left when the duel ended, and whether this player died with some. */
    int left = -1;

    /** A hit by this player (0) or the other (1), on this tick. */
    void hit(int tick, int by) {
        hits[by]++;
        if (who == by) {
            len++;
        } else {
            closeRun();
            who = by;
            len = 1;
        }
        if (len >= 3) {
            third[by]++;
        }
        if (gwho == by && tick - glast <= GAP) {
            glen++;
        } else {
            closeGapRun();
            gwho = by;
            glen = 1;
        }
        glast = tick;
    }

    private void closeRun() {
        if (who >= 0) {
            runs[who]++;
            runs3[who] += len >= 3 ? 1 : 0;
            longest[who] = Math.max(longest[who], len);
        }
        who = -1;
        len = 0;
    }

    private void closeGapRun() {
        if (gwho >= 0) {
            gruns[gwho]++;
            gruns3[gwho] += glen >= 3 ? 1 : 0;
            glongest[gwho] = Math.max(glongest[gwho], glen);
        }
        gwho = -1;
        glen = 0;
    }

    /**
     * A healing potion of its own thrown with this much health of this
     * maximum, with so many others of its own still on the way. Returns
     * the points of it that cannot fit.
     */
    double thrown(double health, double max, int onWay) {
        potions++;
        double missing = max - health;
        if (missing < POTION_HEAL) {
            early++;
        }
        double room = missing - POTION_HEAL * onWay;
        double u = Math.min(Math.max(POTION_HEAL - Math.max(room, 0.0), 0.0), POTION_HEAL);
        unfit += u;
        return u;
    }

    /**
     * One of its own potions broke: the health it gave, the health that
     * was missing when it broke, and the points it offered as far as the
     * distance from the feet tells. Returns {offered, far, over} as
     * counted: when the health did not reach the maximum the potion
     * offered what it gave; when it did, the distance says how much more.
     */
    double[] splash(double healed, double missing, int offeredGuess, boolean mixedWithHit) {
        broke++;
        mixed += mixedWithHit ? 1 : 0;
        double h = Math.max(healed, 0.0);
        double offered;
        if (h < missing - 0.01) {
            offered = Math.min(h, POTION_HEAL);
        } else {
            capped++;
            offered = Math.min(Math.max(h, offeredGuess), POTION_HEAL);
        }
        double f = POTION_HEAL - offered, o = offered - Math.min(h, offered);
        given += Math.min(h, POTION_HEAL);
        far += f;
        over += o;
        return new double[] {offered, f, o};
    }

    /** A pearl of its own landed: front, side, back or away, and whether into the other's facing. */
    void landed(String where, boolean intoFacing) {
        landings++;
        if (where.equals("front")) {
            front++;
        } else if (where.equals("side")) {
            side++;
        } else if (where.equals("back")) {
            back++;
        } else {
            away++;
        }
        faced += intoFacing ? 1 : 0;
    }

    /**
     * Where a pearl landed seen from the other player, as the simulator
     * tells it (env.rs, collect_landings): within {@link #NEAR_LAND} of it,
     * in front (within 60 degrees of the line from it to where the pearl
     * was thrown from), to a side (60 to 120) or behind; else away.
     */
    static String where(double landX, double landZ, double fromX, double fromZ, double otherX, double otherZ) {
        double ax = landX - otherX, az = landZ - otherZ;
        double d = Math.hypot(ax, az);
        if (d > NEAR_LAND) {
            return "away";
        }
        double line = angle(ax, az, fromX - otherX, fromZ - otherZ);
        return line < 60.0 ? "front" : line <= 120.0 ? "side" : "back";
    }

    /** Whether it landed within {@link #NEAR_LAND} and 60 degrees of the way the other player faces. */
    static boolean intoFacing(double landX, double landZ, double otherX, double otherZ, float otherYaw) {
        double ax = landX - otherX, az = landZ - otherZ;
        double yaw = Math.toRadians(otherYaw);
        return Math.hypot(ax, az) <= NEAR_LAND && angle(ax, az, -Math.sin(yaw), Math.cos(yaw)) < 60.0;
    }

    private static double angle(double ax, double az, double vx, double vz) {
        double n = Math.hypot(vx, vz), d = Math.hypot(ax, az);
        if (n < 1e-9 || d < 1e-9) {
            return 0.0;
        }
        return Math.toDegrees(Math.acos(Math.min(Math.max((ax * vx + az * vz) / (n * d), -1.0), 1.0)));
    }

    /** The heal a potion breaking this far from the feet offers (EntityPotion.onImpact, Potion.affectEntity). */
    static int offered(double distance) {
        return distance < 4.0 ? (int) ((1.0 - distance / 4.0) * POTION_HEAL + 0.5) : 0;
    }

    /** The duel is over: the runs under way count as they are. */
    void end() {
        closeRun();
        closeGapRun();
    }

    private static String pct(double a, double b) {
        return b > 0 ? String.format(Locale.ROOT, "%.1f", 100.0 * a / b) : "-";
    }

    private static String per(double a, double minutes) {
        return minutes > 0 ? String.format(Locale.ROOT, "%.1f", a / minutes) : "-";
    }

    /** The line that sums the duel up: key=value pairs, see real_duels.py. */
    String summary(String opponent, String result, String end, boolean died) {
        double minutes = ticks / 1200.0;
        StringBuilder b = new StringBuilder("PVPAI-DUEL");
        kv(b, "vs", opponent);
        kv(b, "result", result);
        kv(b, "end", end);
        kv(b, "s", String.format(Locale.ROOT, "%.1f", ticks / 20.0));
        kv(b, "hits", hits[0]);
        kv(b, "taken", hits[1]);
        kv(b, "hpm", per(hits[0], minutes));
        kv(b, "tpm", per(hits[1], minutes));
        // Runs taken, then landed.
        kv(b, "runs_t", runs[1]);
        kv(b, "c3_t", runs3[1]);
        kv(b, "max_t", longest[1]);
        kv(b, "h3_t", third[1]);
        kv(b, "gruns_t", gruns[1]);
        kv(b, "g3_t", gruns3[1]);
        kv(b, "gmax_t", glongest[1]);
        kv(b, "runs_l", runs[0]);
        kv(b, "c3_l", runs3[0]);
        kv(b, "max_l", longest[0]);
        kv(b, "h3_l", third[0]);
        kv(b, "gruns_l", gruns[0]);
        kv(b, "g3_l", gruns3[0]);
        kv(b, "gmax_l", glongest[0]);
        kv(b, "pots", potions);
        kv(b, "early", early);
        kv(b, "unfit", String.format(Locale.ROOT, "%.1f", unfit));
        kv(b, "broke", broke);
        kv(b, "healed", String.format(Locale.ROOT, "%.2f", given));
        kv(b, "acc", pct(given, POTION_HEAL * broke));
        kv(b, "far", pct(far, POTION_HEAL * broke));
        kv(b, "over", pct(over, POTION_HEAL * broke));
        kv(b, "capped", capped);
        kv(b, "mixed", mixed);
        kv(b, "pearls", pearls);
        kv(b, "landed", landings);
        kv(b, "front", front);
        kv(b, "side", side);
        kv(b, "back", back);
        kv(b, "away", away);
        kv(b, "faced", faced);
        kv(b, "left", left);
        kv(b, "drop", died && left > 0 ? 1 : 0);
        return b.toString();
    }

    private static void kv(StringBuilder b, String k, Object v) {
        b.append(' ').append(k).append('=').append(v);
    }
}
