package chestai.core;

import java.util.ArrayList;
import java.util.List;

/**
 * What the client remembers between ticks: the farm chests it knows of
 * and the last thing each one's hologram said, and how long since the
 * player was last hurt. As {@code Env.update_known} and the
 * {@code since_hurt} count of the simulator.
 */
public final class Memory {
    public static final class Known {
        public final int x, y, z;
        public boolean seen;
        public long tick;
        public int state, secs;
        public long activeSeenAt;

        public Known(int x, int y, int z) {
            this.x = x;
            this.y = y;
            this.z = z;
        }

        public double cx() {
            return x + 0.5;
        }

        public double cy() {
            return y + 0.5;
        }

        public double cz() {
            return z + 0.5;
        }
    }

    public final List<Known> chests = new ArrayList<Known>();
    public int sinceHurt = 100;
    private float lastHp = Float.NaN;

    /** The index of the chest at this block, added if new. */
    public int chest(int x, int y, int z) {
        for (int i = 0; i < chests.size(); i++) {
            Known k = chests.get(i);
            if (k.x == x && k.y == y && k.z == z) {
                return i;
            }
        }
        chests.add(new Known(x, y, z));
        return chests.size() - 1;
    }

    /** Take in what this tick showed. The first frame only sets the health to compare with. */
    public void update(Frame f, boolean first) {
        for (Frame.Holo h : f.holos) {
            Known k = chests.get(h.chest);
            if (h.state == Frame.ACTIVE && (!k.seen || k.state != h.state)) {
                k.activeSeenAt = f.tick;
            }
            k.seen = true;
            k.tick = f.tick;
            k.state = h.state;
            k.secs = h.secs;
        }
        float hp = f.hp + f.absorption;
        if (!first) {
            if (hp < lastHp) {
                sinceHurt = 0;
            } else {
                sinceHurt = Math.min(sinceHurt + 1, 1000);
            }
        }
        lastHp = hp;
    }

    /** Seconds to the next wave as the client works it out. */
    public double estSecs(int ci, long now) {
        Known k = chests.get(ci);
        if (!k.seen) {
            return 999.0;
        }
        if (k.state == Frame.COOLDOWN) {
            return Math.max(k.secs - (now - k.tick) / 20.0, 0.0);
        }
        return 0.0;
    }
}
