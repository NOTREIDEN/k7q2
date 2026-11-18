package pvpai.core;

import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.Map;

/**
 * The counts an agent goes by that the game does not keep itself: how long
 * ago its player was knocked, hurt, put somewhere, how long ago it swung
 * and hit, how many potions it has seen the other throw, and of the pearls
 * and potions in flight which are its own and where the server has them.
 * Kept from the packets the client is handed and from what its own tick
 * did, and written into a {@link View}. A copy of what the simulator's
 * client keeps (sim/src/client.rs), checked against it.
 */
public final strictfp class Book {
    /** Half of the step positions are sent in. */
    private static final double HALF_STEP = 1.0 / 64.0;

    private static final class Flying {
        int kind;
        boolean mine;
        /** Client ticks since it appeared, and server ticks its motion stands for. */
        int age, air;
        /** Where the server's is ahead of the copy, and the first step of its flight in the 32nds positions come in. */
        final double[] behind = {HALF_STEP, HALF_STEP, HALF_STEP}, first = new double[3];
        /** Its place as the server sends it, in 32nds, where the copy was put last, and the motion last told. */
        final int[] serverPos = new int[3];
        final double[] start = new double[3], motion = new double[3];
        boolean told;
    }

    private final double[] lastKb = new double[3];
    private int sinceKb = 1000, sinceHurt = 1000, sinceSwing = 1000, sinceAttack = 1000, sinceMoved = 1000, sinceThrow = 1000;
    /** Where its eyes were at its last throw and where that throw started on the server. */
    private double[] throwEye, throwFrom;
    /** Its own pearls and potions the server has not shown yet, and potions of the other seen so far. */
    private int pearlsPending, potionsPending, potionsSeen;
    private int otherSinceHurt = 1000, otherSinceJump = 1000;
    /** By number, oldest first. */
    private final Map<Long, Flying> flying = new LinkedHashMap<Long, Flying>();

    /** At the start of a duel, with where the player stands. */
    public Book(double[] feet) {
        throwEye = feet.clone();
        throwFrom = feet.clone();
    }

    // Packets, each at the moment the client handles it.

    /** A velocity for its own player, in 8000ths of a block per tick. */
    public void velocity(int vx, int vy, int vz) {
        lastKb[0] = (double) vx / 8000.0;
        lastKb[1] = (double) vy / 8000.0;
        lastKb[2] = (double) vz / 8000.0;
        sinceKb = 0;
    }

    /** The hurt animation of its own player or of the other (entity status 2). */
    public void hurt(boolean own) {
        if (own) {
            sinceHurt = 0;
        } else {
            otherSinceHurt = 0;
        }
    }

    /**
     * Another player is the opponent from now on, with this much left of
     * its hurt animation (10 just hit, 0 none): what was counted of the
     * one before is not this one's.
     */
    public void newOpponent(int hurtTime) {
        otherSinceHurt = hurtTime > 0 ? Math.max(0, 10 - hurtTime) : 1000;
        otherSinceJump = 1000;
        potionsSeen = 0;
    }

    /** The server put its own player somewhere. */
    public void moved() {
        sinceMoved = 0;
    }

    /**
     * The server put the other player somewhere (a teleport packet): where
     * the copy was heading before, and the new place in 32nds of a block.
     * More than three blocks at once is a pearl.
     */
    public void otherTeleported(double[] targetBefore, int px, int py, int pz) {
        double[] d = {(double) px / 32.0, (double) py / 32.0, (double) pz / 32.0};
        if (Geo.distSq(targetBefore, d) > 9.0) {
            otherSinceJump = 0;
        }
    }

    /** A pearl or a potion appeared, at this place in 32nds of a block. */
    public void spawned(long id, int kind, int px, int py, int pz) {
        Flying e = new Flying();
        e.kind = kind;
        e.serverPos[0] = px;
        e.serverPos[1] = py;
        e.serverPos[2] = pz;
        for (int a = 0; a < 3; a++) {
            e.start[a] = (double) e.serverPos[a] / 32.0;
        }
        int pending = kind == Inv.KIND_PEARL ? pearlsPending : potionsPending;
        e.mine = pending > 0 && Geo.distSq(e.start, throwEye) < 1.0;
        if (e.mine) {
            if (kind == Inv.KIND_PEARL) {
                pearlsPending--;
            } else {
                potionsPending--;
            }
            // Where its own throw started is known exactly; of the rest
            // only up to what the 32nds cut off.
            boolean fits = true;
            double[] cut = new double[3];
            for (int a = 0; a < 3; a++) {
                cut[a] = throwFrom[a] - e.start[a];
                fits &= cut[a] >= 0.0 && cut[a] <= 2.0 * HALF_STEP;
            }
            if (fits) {
                System.arraycopy(cut, 0, e.behind, 0, 3);
            }
        } else if (kind == Inv.KIND_POTION) {
            potionsSeen = Math.min(potionsSeen + 1, 255);
        }
        flying.put(id, e);
    }

    /** The velocity of a pearl or a potion, in 8000ths of a block per tick. */
    public void thrownVelocity(long id, int vx, int vy, int vz) {
        Flying e = flying.get(id);
        if (e == null) {
            return;
        }
        if (e.age == 0 && e.air == 0 && e.told) {
            // Told twice before it moved here: the server's has made the
            // step in between.
            e.air = 1;
            for (int a = 0; a < 3; a++) {
                double from = e.start[a] + e.behind[a], was = (double) e.serverPos[a] / 32.0;
                e.first[a] = Math.floor((from + e.motion[a]) * 32.0) / 32.0 - was;
                e.behind[a] += e.motion[a];
            }
        }
        e.motion[0] = (double) vx / 8000.0;
        e.motion[1] = (double) vy / 8000.0;
        e.motion[2] = (double) vz / 8000.0;
        e.told = true;
    }

    /**
     * A pearl or a potion moved (a relative move, by so many 32nds; a look
     * alone is a move by nothing), and where the client's copy is once the
     * packet is handled.
     */
    public void thrownMoved(long id, int dx, int dy, int dz, double[] posAfter) {
        Flying e = flying.get(id);
        if (e == null) {
            return;
        }
        e.serverPos[0] += dx;
        e.serverPos[1] += dy;
        e.serverPos[2] += dz;
        for (int a = 0; a < 3; a++) {
            e.behind[a] = (double) e.serverPos[a] / 32.0 + e.first[a] + HALF_STEP - posAfter[a];
            e.start[a] = posAfter[a];
        }
    }

    /**
     * A pearl or a potion was put somewhere (a teleport packet), in 32nds,
     * and where the client's copy is once the packet is handled.
     */
    public void thrownTeleported(long id, int px, int py, int pz, double[] posAfter) {
        Flying e = flying.get(id);
        if (e == null) {
            return;
        }
        e.serverPos[0] = px;
        e.serverPos[1] = py;
        e.serverPos[2] = pz;
        for (int a = 0; a < 3; a++) {
            e.first[a] = 0.0;
            e.behind[a] = HALF_STEP;
            e.start[a] = posAfter[a];
        }
    }

    /** The server says an entity is gone. */
    public void destroyed(long id) {
        flying.remove(id);
    }

    /**
     * After each tick of the client: whether it swung, whether it hit the
     * other player, whether it threw a pearl or a potion, and, as they were
     * before the tick, where its eyes were and the place and the view the
     * server had last been told (the throw starts from those).
     */
    public void ticked(boolean swung, boolean attacked, boolean threwPearl, boolean threwPotion, double[] eyeBefore, double[] saidPos, float saidYaw, float saidPitch) {
        if (swung) {
            sinceSwing = 0;
        }
        if (attacked) {
            sinceAttack = 0;
        }
        if (threwPearl || threwPotion) {
            sinceThrow = 0;
            throwEye = eyeBefore.clone();
            double[] h = Geo.throwHeading(Inv.KIND_POTION, saidPos, saidYaw, saidPitch);
            throwFrom = new double[] {h[0], h[1], h[2]};
            if (threwPearl) {
                pearlsPending = Math.min(pearlsPending + 1, 255);
            }
            if (threwPotion) {
                potionsPending = Math.min(potionsPending + 1, 255);
            }
        }
        sinceKb = Math.min(sinceKb + 1, 1000);
        sinceHurt = Math.min(sinceHurt + 1, 1000);
        sinceSwing = Math.min(sinceSwing + 1, 1000);
        sinceAttack = Math.min(sinceAttack + 1, 1000);
        sinceMoved = Math.min(sinceMoved + 1, 1000);
        if (sinceThrow < 1000) {
            sinceThrow++;
            if (sinceThrow > 60) {
                // A throw the server refused never shows up.
                pearlsPending = 0;
                potionsPending = 0;
            }
        }
        otherSinceHurt = Math.min(otherSinceHurt + 1, 1000);
        otherSinceJump = Math.min(otherSinceJump + 1, 1000);
        for (Flying e : flying.values()) {
            e.age++;
            e.air++;
        }
    }

    /**
     * Write the counts into a view that already holds what the client
     * itself knows, the pearls and potions it has in its world among it
     * (number, kind, place and motion). One the client no longer has is
     * forgotten here too.
     */
    public void fill(View v) {
        System.arraycopy(lastKb, 0, v.lastKb, 0, 3);
        v.ticksSinceKb = sinceKb;
        v.ticksSinceHurt = sinceHurt;
        v.ticksSinceSwing = sinceSwing;
        v.ticksSinceAttack = sinceAttack;
        v.ticksSinceMoved = sinceMoved;
        v.potionsSeen = potionsSeen;
        v.potionsPending = potionsPending;
        v.oTicksSinceHurt = otherSinceHurt;
        v.oTicksSinceJump = otherSinceJump;
        for (Iterator<Long> it = flying.keySet().iterator(); it.hasNext();) {
            long id = it.next();
            boolean there = false;
            for (View.Thrown t : v.thrown) {
                there |= t.id == id;
            }
            if (!there) {
                it.remove();
            }
        }
        for (Iterator<View.Thrown> it = v.thrown.iterator(); it.hasNext();) {
            View.Thrown t = it.next();
            Flying e = flying.get(t.id);
            if (e == null) {
                // Not seen appearing: nothing is known of it.
                it.remove();
                continue;
            }
            t.mine = e.mine;
            t.age = e.age;
            t.air = e.air;
            for (int a = 0; a < 3; a++) {
                t.serverSide[a] = t.pos[a] + e.behind[a];
            }
        }
    }
}
