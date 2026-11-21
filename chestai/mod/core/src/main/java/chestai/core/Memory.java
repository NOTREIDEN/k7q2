package chestai.core;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Iterator;
import java.util.List;
import java.util.Map;
import java.util.Set;

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
        /** Ticks spent trying to claim it since it was last not claimable; left aside until skipUntil when that runs long. */
        public int claimTicks;
        public long skipUntil = -1;
        /** When its hologram was last read; what it says since then is a guess from its mobs. */
        public long holoTick = -100000;
        public boolean guessed;
        /** Its wave's mobs still alive (by their spawning next to it), and when the first came. */
        public int waveLeft;
        public long waveAt = -1;
        /** The longest wait to the next wave its hologram has said, for a guess when none shows. */
        public int cdSecs;
        /** Waits at it for a wave on a guess that brought none: each makes the next guess twice as long. */
        public int idleWaits;

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
    /** When a farm hologram was last read anywhere: whether the server shows them at all. */
    public long holoWorkAt = -100000;
    /** Each mob seen (entity id) and when last; each wave mob and the chest it came from. */
    private final Map<Integer, Long> mobSeen = new HashMap<Integer, Long>();
    private final Map<Integer, Known> waveMob = new HashMap<Integer, Known>();

    /** No hologram read for its chest this long: its state is taken from its mobs. */
    public static final int HOLO_STALE = 100;
    /** A mob first seen this close to the player and to a chest spawned there: that chest's wave. */
    private static final double SPAWN_SEEN_R = 32.0, SPAWN_R = 10.0;

    /** Whether this mob is of a wave of a known chest (it came out next to it). */
    public boolean waveMob(int id) {
        return waveMob.containsKey(id);
    }

    /** Whether this mob is of the wave of this chest. */
    public boolean waveMobOf(int id, Known k) {
        return waveMob.get(id) == k;
    }

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

    /** The chest within {@code r} blocks (and 3 up or down) of this block, the nearest, or -1. */
    public int near(int x, int y, int z, double r) {
        int best = -1;
        double bd = r * r;
        for (int i = 0; i < chests.size(); i++) {
            Known k = chests.get(i);
            double dx = k.x - x, dz = k.z - z;
            double d = dx * dx + dz * dz;
            if (Math.abs(k.y - y) <= 3 && d <= bd) {
                bd = d;
                best = i;
            }
        }
        return best;
    }

    /** Take in what this tick showed. The first frame only sets the health to compare with. */
    public void update(Frame f, boolean first) {
        for (Frame.Holo h : f.holos) {
            Known k = chests.get(h.chest);
            if (h.state == Frame.ACTIVE && (!k.seen || k.state != h.state)) {
                k.activeSeenAt = f.tick;
            }
            if (h.state == Frame.COOLDOWN && h.secs > k.cdSecs && h.secs < 3600) {
                k.cdSecs = h.secs;
            }
            k.seen = true;
            k.tick = f.tick;
            k.state = h.state;
            k.secs = h.secs;
            k.holoTick = f.tick;
            k.guessed = false;
            holoWorkAt = f.tick;
        }
        waves(f);
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

    /**
     * The known chest within r blocks across (and dy up or down) of this point, the nearest, or null.
     * Only one known to be a farm chest (a hologram said so) unless the server shows none.
     */
    private Known chestNear(double x, double y, double z, double r, double dy, boolean anyChest) {
        Known best = null;
        double bd = r * r;
        for (Known k : chests) {
            double dx = k.cx() - x, dz = k.cz() - z;
            double d = dx * dx + dz * dz;
            if ((anyChest || (k.seen && k.holoTick > -100000)) && Math.abs(k.y - y) <= dy && d <= bd) {
                bd = d;
                best = k;
            }
        }
        return best;
    }

    /**
     * The waves as their mobs show them, whatever the holograms show: a mob first seen next to a
     * chest (and near the player, so it spawned there rather than came into sight) is that chest's,
     * and so is any by a chest whose hologram says a wave is on. With no hologram read for a chest
     * a while, its state is guessed: a wave on while its mobs live, to be claimed once they are dead.
     */
    private void waves(Frame f) {
        long t = f.tick;
        Set<Integer> here = new HashSet<Integer>();
        for (Frame.Mob m : f.mobs) {
            here.add(m.id);
            Long seen = mobSeen.put(m.id, t);
            if (waveMob.containsKey(m.id)) {
                continue;
            }
            Known k = null;
            double dp = Math.hypot(m.x - f.x, m.z - f.z);
            if (seen == null && dp < SPAWN_SEEN_R) {
                // (Any chest found about, with no hologram read anywhere for five minutes.)
                k = chestNear(m.x, m.y, m.z, SPAWN_R, 4.0, t - holoWorkAt > 6000);
            } else {
                Known a = chestNear(m.x, m.y, m.z, 12.0, 6.0, false);
                if (a != null && a.seen && a.state == Frame.ACTIVE && t - a.holoTick < 40) {
                    k = a;
                }
            }
            if (k != null) {
                waveMob.put(m.id, k);
                k.idleWaits = 0;
                if (k.waveAt < 0) {
                    k.waveAt = t;
                }
            }
        }
        for (Known k : chests) {
            k.waveLeft = 0;
        }
        for (Iterator<Map.Entry<Integer, Known>> it = waveMob.entrySet().iterator(); it.hasNext();) {
            Map.Entry<Integer, Known> e = it.next();
            Known k = e.getValue();
            if (!chests.contains(k)) {
                it.remove();
                continue;
            }
            if (here.contains(e.getKey())) {
                k.waveLeft++;
                continue;
            }
            // Out of the world: dead (or gone with the wave), as the player is close enough to see it
            // were it alive; far away, kept a while (back from a retreat, it is still to be killed).
            long ago = t - mobSeen.get(e.getKey());
            double dc = Math.hypot(k.cx() - f.x, k.cz() - f.z);
            if ((ago > 40 && dc < 40.0) || ago > 2400) {
                it.remove();
            } else {
                k.waveLeft++;
            }
        }
        if (mobSeen.size() > 4000) {
            for (Iterator<Map.Entry<Integer, Long>> it = mobSeen.entrySet().iterator(); it.hasNext();) {
                Map.Entry<Integer, Long> e = it.next();
                if (t - e.getValue() > 6000 && !waveMob.containsKey(e.getKey())) {
                    it.remove();
                }
            }
        }
        for (Known k : chests) {
            boolean noHolo = t - k.holoTick > HOLO_STALE;
            if (k.waveLeft > 0) {
                if (noHolo) {
                    if (!k.seen || k.state != Frame.ACTIVE) {
                        k.activeSeenAt = k.waveAt;
                    }
                    k.seen = true;
                    k.state = Frame.ACTIVE;
                    k.secs = 0;
                    k.tick = t;
                    k.guessed = true;
                }
            } else if (k.waveAt >= 0) {
                // Its wave's mobs all dead: with no hologram to say so, the chest is clicked to claim it.
                k.waveAt = -1;
                if (noHolo && k.state == Frame.ACTIVE) {
                    k.state = Frame.CLAIMABLE;
                    k.tick = t;
                    k.claimTicks = 0;
                    k.guessed = true;
                }
            }
        }
    }

    /** A guessed claim done (clicked a while): the wait to the next wave, as long as the hologram said before, else a guess. */
    public void claimedGuess(Known k, long t) {
        k.state = Frame.COOLDOWN;
        k.secs = (k.cdSecs > 0 ? k.cdSecs : 30) << Math.min(k.idleWaits, 4);
        k.tick = t;
        k.claimTicks = 0;
        k.guessed = true;
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
