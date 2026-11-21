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
        /** When its countdown last showed a new number (or started), for counting it down between holograms; -1 with none. */
        private long countdownAt = -1;
        public long activeSeenAt;
        /** Ticks spent trying to claim it since it was last not claimable; left aside until skipUntil when that runs long. */
        public int claimTicks;
        /** The claim clicks tried on it, and when the last one was. */
        public int claimAttempts;
        public long lastClaimAttempt = -1000;
        public long skipUntil = -1;
        /** When its hologram was last read; what it says since then is a guess from its mobs. */
        public long holoTick = -100000;
        public boolean guessed;
        /** Its wave's mobs still alive (by their spawning next to it), and when the first came. */
        public int waveLeft;
        public long waveAt = -1;
        /** Whether another player fights its wave (its claim is not the agent's), and whether another player stands by it. */
        public boolean contestedWave, reserved;
        /**
         * Who was at it first. ours: the player came to it (OWN_R) with no other player by it, and
         * has not gone far since: it stays the agent's whoever comes after. theirs: another player
         * was already by it when the player came: left to them until they go.
         */
        public boolean ours, theirs;
        public long awayAt = -1, rivalSeenAt = -100000;
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
    /** Mobs first seen near the player with no chest yet to give them to, and where they were then. */
    private final Map<Integer, SpawnSighting> unmatchedSpawns = new HashMap<Integer, SpawnSighting>();

    /** Within this of a chest (and 5 up or down) the player is at it; this far for AWAY_TICKS, it is no longer the agent's. */
    private static final double OWN_R = 10.0, AWAY_R = 24.0;
    private static final int AWAY_TICKS = 200;
    /** Another player gone from a chest this long: it is free again. */
    private static final int RIVAL_GONE_TICKS = 100;

    /** No hologram read for its chest this long: its state is taken from its mobs. */
    public static final int HOLO_STALE = 100;
    /** A mob first seen this close to the player and to a chest spawned there: that chest's wave. */
    private static final double SPAWN_SEEN_R = 32.0, SPAWN_R = 10.0;
    /** How long where a mob was first seen is kept for a chest found later (its hologram read late). */
    private static final int SPAWN_SIGHTING_TICKS = 100;

    /** Where a mob was first seen, and whether another player was on it since. */
    private static final class SpawnSighting {
        final double x, y, z;
        final long tick;
        boolean contested;

        SpawnSighting(Frame.Mob m, long t) {
            this.x = m.x;
            this.y = m.y;
            this.z = m.z;
            this.tick = t;
        }
    }

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
        int i = indexOfChest(x, y, z);
        if (i >= 0) {
            return i;
        }
        chests.add(new Known(x, y, z));
        return chests.size() - 1;
    }

    /** The index of the chest at this block, or -1. */
    public int indexOfChest(int x, int y, int z) {
        for (int i = 0; i < chests.size(); i++) {
            Known k = chests.get(i);
            if (k.x == x && k.y == y && k.z == z) {
                return i;
            }
        }
        return -1;
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
        for (Known k : chests) {
            ownership(f, k);
        }
        for (Frame.Holo h : f.holos) {
            Known k = chests.get(h.chest);
            if (h.state == Frame.ACTIVE && (!k.seen || k.state != h.state)) {
                k.activeSeenAt = f.tick;
            }
            // Its wave over and no one else's any more.
            if (k.waveLeft == 0 && (h.state == Frame.READY || h.state == Frame.COOLDOWN)) {
                k.contestedWave = false;
            }
            if (h.state == Frame.COOLDOWN && h.secs > k.cdSecs && h.secs < 3600) {
                k.cdSecs = h.secs;
            }
            // A countdown that shows the same number a while (a hologram that is not
            // redrawn every second) keeps counting from when the number came.
            if (h.state != Frame.COOLDOWN) {
                k.countdownAt = -1;
            } else if (k.state != Frame.COOLDOWN || k.guessed || h.secs != k.secs || k.countdownAt < 0) {
                k.countdownAt = f.tick;
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
     * The known chest within r blocks across (and dy up or down) of this point, the nearest (up and down
     * too), or null. Only one known to be a farm chest (a hologram said so) unless the server shows none.
     */
    private Known chestNear(double x, double y, double z, double r, double maxDy, boolean anyChest) {
        Known best = null;
        double bd = Double.POSITIVE_INFINITY;
        for (Known k : chests) {
            double dx = k.cx() - x, dz = k.cz() - z, dy = k.y - y;
            double across = dx * dx + dz * dz;
            double d = across + dy * dy;
            if ((anyChest || (k.seen && k.holoTick > -100000)) && Math.abs(dy) <= maxDy && across <= r * r && d < bd) {
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
        unmatchedSpawns.values().removeIf(sp -> t - sp.tick > SPAWN_SIGHTING_TICKS);
        Set<Integer> here = new HashSet<Integer>();
        for (Frame.Mob m : f.mobs) {
            here.add(m.id);
            Long seen = mobSeen.put(m.id, t);
            // Another player on it (it hit it last, or stands by it): its wave is not the agent's to claim.
            // ...unless its chest is the agent's (the player was there first), or it is at the player
            // and hurting them: that one is always fought back.
            boolean selfDefence = sinceHurt < 40 && Math.hypot(m.x - f.x, m.z - f.z) < 3.5 && Math.abs(m.y - f.y) < 2.5;
            m.contested = (m.hitByOtherPlayer || rivalNear(f, m)) && !selfDefence && !ofOurs(m);
            if (waveMob.containsKey(m.id)) {
                if (m.contested) {
                    waveMob.get(m.id).contestedWave = true;
                }
                continue;
            }
            Known k = null;
            // Where it was first seen counts, for a while, if its chest is only known later.
            if (seen == null && Math.hypot(m.x - f.x, m.z - f.z) < SPAWN_SEEN_R) {
                unmatchedSpawns.put(m.id, new SpawnSighting(m, t));
            }
            SpawnSighting sp = unmatchedSpawns.get(m.id);
            if (sp != null) {
                sp.contested |= m.contested;
                // (Any chest found about, with no hologram read anywhere for five minutes.)
                k = chestNear(sp.x, sp.y, sp.z, SPAWN_R, 4.0, t - holoWorkAt > 6000);
            }
            if (k == null) {
                Known a = chestNear(m.x, m.y, m.z, 12.0, 6.0, false);
                if (a != null && a.seen && a.state == Frame.ACTIVE && t - a.holoTick < 40) {
                    k = a;
                }
            }
            if (k != null) {
                waveMob.put(m.id, k);
                k.idleWaits = 0;
                unmatchedSpawns.remove(m.id);
                if (k.waveAt < 0) {
                    if (k.state != Frame.CLAIMABLE) {
                        k.contestedWave = false;
                    }
                    k.waveAt = sp == null ? t : sp.tick;
                }
                k.contestedWave |= !k.ours && (m.contested || (sp != null && sp.contested));
                // A chest with a wave is not left aside before its hologram is read.
                if (!k.seen) {
                    k.skipUntil = -1;
                }
            }
        }
        // Every mob of a wave another player has is theirs.
        for (Frame.Mob m : f.mobs) {
            Known k = waveMob.get(m.id);
            boolean selfDefence = sinceHurt < 40 && Math.hypot(m.x - f.x, m.z - f.z) < 3.5 && Math.abs(m.y - f.y) < 2.5;
            m.contested |= k != null && (k.contestedWave || k.reserved) && !selfDefence;
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
                    if (k.contestedWave || k.reserved) {
                        // Another player's wave: they claim it, the chest waits for its next one.
                        claimedGuess(k, t);
                        k.contestedWave = false;
                    } else {
                        k.state = Frame.CLAIMABLE;
                        k.tick = t;
                        k.claimTicks = 0;
                        k.guessed = true;
                    }
                }
            }
        }
    }

    /**
     * Who the chest is for: the first one at it. The player at it with nobody else by it makes it the
     * agent's, and then nobody coming later takes it away (the wave is fought and claimed); another
     * player already by it when the player comes makes it theirs, until they have gone from it.
     */
    private void ownership(Frame f, Known k) {
        boolean rival = rivalNearChest(f, k);
        if (rival) {
            k.rivalSeenAt = f.tick;
        }
        double d = Math.hypot(k.cx() - f.x, k.cz() - f.z);
        boolean at = d <= OWN_R && Math.abs(k.y - f.y) <= 5.0;
        if (k.ours) {
            if (d > AWAY_R) {
                if (k.awayAt < 0) {
                    k.awayAt = f.tick;
                } else if (f.tick - k.awayAt > AWAY_TICKS) {
                    k.ours = false;
                }
            } else {
                k.awayAt = -1;
            }
        }
        if (k.theirs && f.tick - k.rivalSeenAt > RIVAL_GONE_TICKS) {
            k.theirs = false;
        }
        if (!k.ours && !k.theirs && at) {
            if (rival) {
                k.theirs = true;
            } else {
                k.ours = true;
                k.awayAt = -1;
                // Whatever another player did at it before is over: the player is the one at it now.
                k.contestedWave = false;
            }
        }
        k.reserved = !k.ours && (k.theirs || rival);
    }

    /** The chest this mob belongs to (its wave's, or the nearest within 12 blocks) is the agent's. */
    private boolean ofOurs(Frame.Mob m) {
        Known w = waveMob.get(m.id);
        if (w != null) {
            return w.ours;
        }
        Known n = chestNear(m.x, m.y, m.z, 12.0, 8.0, true);
        return n != null && n.ours;
    }

    /** Another player within 6 blocks of the mob. */
    private static boolean rivalNear(Frame f, Frame.Mob m) {
        for (double[] r : f.rivals) {
            double dx = r[0] - m.x, dy = r[1] - m.y, dz = r[2] - m.z;
            if (dx * dx + dy * dy + dz * dz <= 36.0) {
                return true;
            }
        }
        return false;
    }

    /** Another player within 12 blocks across (and four up or down) of the chest. */
    private static boolean rivalNearChest(Frame f, Known k) {
        for (double[] r : f.rivals) {
            double dx = r[0] - k.cx(), dz = r[2] - k.cz();
            if (Math.abs(r[1] - k.y) <= 4.0 && dx * dx + dz * dz <= 144.0) {
                return true;
            }
        }
        return false;
    }

    /** A guessed claim done (clicked a while): the wait to the next wave, as long as the hologram said before, else a guess. */
    public void claimedGuess(Known k, long t) {
        k.state = Frame.COOLDOWN;
        k.secs = (k.cdSecs > 0 ? k.cdSecs : 30) << Math.min(k.idleWaits, 4);
        k.tick = t;
        k.countdownAt = t;
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
            long from = k.countdownAt >= 0 ? k.countdownAt : k.tick;
            return Math.max(k.secs - (now - from) / 20.0, 0.0);
        }
        return 0.0;
    }
}
