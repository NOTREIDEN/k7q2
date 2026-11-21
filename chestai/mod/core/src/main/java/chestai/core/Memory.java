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
        /** The time its running wave had left as its hologram last showed it (-1: none shown), and when. */
        public int waveShown = -1;
        public long waveShownAt = -1;
        /** Since when its hologram says it is ready (-1: not ready). */
        public long readyAt = -1;
        /** Its wave on was seen come (the player there when it did), so its length can be measured. */
        public boolean waveFromStart;
        /** The most mobs its wave on had at once. */
        public int wavePeak;
        /** How long its last wave that ran out of time (reset, mobs alive) had been on, in seconds; -1: none seen. */
        public double resetSecs = -1;

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
    private float lastHp = Float.NaN, lastOwnHp = Float.NaN;
    /** Absorption down by this much in a tick is a blow (absorption going by itself goes by less). */
    private static final float HURT_ABS = 0.3f;
    /**
     * What the server is seen to do, for waves of several chests at once: the farthest the player
     * was from a chest when its wave came (it had been ready a while: the player came within reach
     * of it), the shortest a wave ran before it reset, how many of each were seen, and the last one.
     */
    public double spawnR = -1, resetMin = -1, lastSpawnD, lastResetSecs;
    public int spawnObs, resetObs;
    public Known lastSpawnK, lastResetK;
    /** The most mobs a wave seen from its start had at once; the waves of the agent's won from their start, their mobs, and their time all together. */
    public int waveSize, clears, clearMobs;
    public double clearSecs;
    /** When a farm hologram was last read anywhere: whether the server shows them at all. */
    public long holoWorkAt = -100000;
    /** Each mob seen (entity id) and when last; each wave mob and the chest it came from. */
    private final Map<Integer, Long> mobSeen = new HashMap<Integer, Long>();
    private final Map<Integer, Known> waveMob = new HashMap<Integer, Known>();
    /** Mobs first seen near the player with no chest yet to give them to, and where they were then. */
    private final Map<Integer, SpawnSighting> unmatchedSpawns = new HashMap<Integer, SpawnSighting>();

    /**
     * World bosses (bosses=si, -Dchestai.boss=false to turn off): a monster of no chest's wave that is a
     * zombie pigman, came by an end crystal, is named as a boss, or has far more health than any wave's
     * mob seen (BOSS_HP_X times the most, at least BOSS_HP_MIN). Fought in a crowd: another player on it
     * does not make it theirs.
     */
    public boolean bosses = true;
    /** The most health a wave's mob was seen with. */
    public float waveMaxHp;
    private static final float BOSS_HP_MIN = 300.0f, BOSS_HP_X = 2.0f;
    /** The bosses seen (entity id), their name and where and when last, for the play log. */
    private final Map<Integer, BossSeen> bossSeen = new HashMap<Integer, BossSeen>();
    /** Lines for the play log (Session writes and clears them). */
    public final List<String> notes = new ArrayList<String>();
    /**
     * The movement round 3 (-Dchestai.move3=true): another player by a chest keeps it theirs a few
     * seconds after they step off (no goal dropped and taken again as they walk in and out of 12 blocks).
     */
    private static final boolean MOVE3 = !"false".equals(System.getProperty("chestai.move3"));
    private static final int RESERVED_HOLD_TICKS = 60;

    private static final class BossSeen {
        final String name;
        double x, y, z;
        long tick;

        BossSeen(String name) {
            this.name = name;
        }
    }

    /** The health from which a monster of no wave is a boss. */
    public float bossHp() {
        return Math.max(BOSS_HP_MIN, BOSS_HP_X * waveMaxHp);
    }

    /** When each mob was first seen (entity id). */
    private final Map<Integer, Long> mobFirst = new HashMap<Integer, Long>();
    /** A pigman or a tough one only once seen this long without coming out of a chest, and no wave on within BOSS_CHEST_R. */
    private static final int BOSS_SETTLE_TICKS = 20;
    private static final double BOSS_CHEST_R = 12.0;

    /**
     * Whether this mob (of no wave) is a boss: by its name or an end crystal at once; a pigman or one
     * far tougher than the waves' once it has been about a second without being of a wave, and with no
     * chest's wave on by it (a server's waves of tough mobs or of pigmen are no bosses).
     */
    private boolean bossLike(Frame.Mob m, long t) {
        if (!m.hostile) {
            return false;
        }
        if (m.fromCrystal || m.bossName) {
            return true;
        }
        if (!m.pigman && m.maxHp < bossHp()) {
            return false;
        }
        Long first = mobFirst.get(m.id);
        if (first == null || t - first < BOSS_SETTLE_TICKS) {
            return false;
        }
        SpawnSighting sp = unmatchedSpawns.get(m.id);
        if (sp != null && chestNear(sp.x, sp.y, sp.z, SPAWN_R, 4.0, true) != null) {
            // First seen by a chest: maybe its wave, its hologram not read yet.
            return false;
        }
        for (Known k : chests) {
            if (k.seen && k.state == Frame.ACTIVE && Math.hypot(k.cx() - m.x, k.cz() - m.z) < BOSS_CHEST_R && Math.abs(k.y - m.y) < 8.0) {
                return false;
            }
        }
        return true;
    }

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
            if (h.state == Frame.READY && (!k.seen || k.state != Frame.READY)) {
                k.readyAt = f.tick;
            } else if (h.state == Frame.COOLDOWN || h.state == Frame.CLAIMABLE) {
                k.readyAt = -1;
            }
            // A wave seen from its start that went back to a countdown with its mobs alive: its time ran out.
            if (k.seen && !k.guessed && k.state == Frame.ACTIVE && (h.state == Frame.COOLDOWN || h.state == Frame.READY) && k.waveLeft > 0
                    && k.waveAt >= 0 && k.waveFromStart && !k.contestedWave && !k.reserved) {
                double secs = (f.tick - waveStart(k)) / 20.0;
                k.resetSecs = secs;
                resetMin = resetMin < 0 ? secs : Math.min(resetMin, secs);
                resetObs++;
                lastResetK = k;
                lastResetSecs = secs;
            }
            // One of the agent's seen from its start won: how long it took, for how many mobs.
            if (k.seen && k.state == Frame.ACTIVE && h.state == Frame.CLAIMABLE && k.waveAt >= 0 && k.waveFromStart && k.ours && !k.contestedWave
                    && k.wavePeak > 0) {
                clearSecs += (f.tick - waveStart(k)) / 20.0;
                clearMobs += k.wavePeak;
                clears++;
            }
            if (h.state == Frame.ACTIVE && h.waveSecs >= 0) {
                if (h.waveSecs != k.waveShown || k.waveShownAt < 0) {
                    k.waveShownAt = f.tick;
                }
                k.waveShown = h.waveSecs;
            } else {
                k.waveShown = -1;
                k.waveShownAt = -1;
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
            // A hurt: health itself down, or the absorption down by a blow's worth. Absorption that
            // goes by itself a little at a time (as on some servers) is no hurt.
            if (f.hp < lastOwnHp - 0.01f || hp < lastHp - HURT_ABS) {
                sinceHurt = 0;
            } else {
                sinceHurt = Math.min(sinceHurt + 1, 1000);
            }
        }
        lastHp = hp;
        lastOwnHp = f.hp;
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
            if (seen == null) {
                mobFirst.put(m.id, t);
            }
            if (waveMob.containsKey(m.id)) {
                waveMaxHp = Math.max(waveMaxHp, m.maxHp);
            }
            // A boss is no chest's wave, and nobody's: whoever else hits it, it is fought until it is dead.
            BossSeen bs = bosses ? bossSeen.get(m.id) : null;
            if (bs == null && bosses && !waveMob.containsKey(m.id) && bossLike(m, t)) {
                bs = new BossSeen(m.name);
                bossSeen.put(m.id, bs);
                unmatchedSpawns.remove(m.id);
                notes.add(String.format(java.util.Locale.ROOT, "boss: veo %s hp %.0f/%.0f en (%.0f,%.0f,%.0f)%s", m.name, m.hp, m.maxHp, m.x, m.y, m.z,
                    m.pigman ? " (pigman)" : m.fromCrystal ? " (cristal)" : m.bossName ? " (nombre)" : " (vida)"));
            }
            if (bs != null) {
                m.boss = true;
                m.contested = false;
                bs.x = m.x;
                bs.y = m.y;
                bs.z = m.z;
                bs.tick = t;
                continue;
            }
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
                    // Come by the player (first seen near it) with the chest ready before: its wave's start.
                    k.waveFromStart = sp != null && k.readyAt >= 0 && (k.state == Frame.READY || k.state == Frame.ACTIVE);
                    k.wavePeak = 0;
                    double d = dist(f.x, f.y, f.z, k);
                    if (k.waveFromStart && sp.tick == t && t - k.readyAt > 20 && !k.reserved && !rivalWithin(f, k, d + 2.0)) {
                        // Ready a second or more and only now come (its first mob seen this tick): the player
                        // just got within reach of it.
                        spawnR = Math.max(spawnR, d);
                        spawnObs++;
                        lastSpawnD = d;
                        lastSpawnK = k;
                    }
                }
                k.contestedWave |= !k.ours && (m.contested || (sp != null && sp.contested));
                // A chest with a wave is not left aside before its hologram is read.
                if (!k.seen) {
                    k.skipUntil = -1;
                }
            }
        }
        for (Iterator<Map.Entry<Integer, BossSeen>> it = bossSeen.entrySet().iterator(); it.hasNext();) {
            BossSeen b = it.next().getValue();
            if (b.tick == t) {
                continue;
            }
            // Gone from the world: dead (close by), or out of sight (far); kept a while for one back in sight.
            double d = Math.hypot(b.x - f.x, b.z - f.z);
            if (b.tick == t - 1) {
                notes.add(String.format(java.util.Locale.ROOT, "boss: ya no esta %s (a %.0f bloques)", b.name, d));
            }
            if (t - b.tick > 6000) {
                it.remove();
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
            mobFirst.keySet().retainAll(mobSeen.keySet());
        }
        for (Known k : chests) {
            if (k.waveAt >= 0 && k.waveLeft > k.wavePeak) {
                k.wavePeak = k.waveLeft;
                if (k.waveFromStart) {
                    waveSize = Math.max(waveSize, k.wavePeak);
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
                k.waveFromStart = false;
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
        k.reserved = !k.ours && (k.theirs || rival || (MOVE3 && f.tick - k.rivalSeenAt < RESERVED_HOLD_TICKS));
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

    /** From the player's feet to the middle of the chest's block at its foot, up and down too. */
    public static double dist(double x, double y, double z, Known k) {
        double dx = k.cx() - x, dy = k.y - y, dz = k.cz() - z;
        return Math.sqrt(dx * dx + dy * dy + dz * dz);
    }

    /** Another player within r blocks of the chest. */
    private static boolean rivalWithin(Frame f, Known k, double r) {
        for (double[] p : f.rivals) {
            if (dist(p[0], p[1], p[2], k) <= r) {
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

    /**
     * Seconds the running wave of a chest has left before it resets, by the time it has been on
     * (from its first mob) against a wave of {@code waveSecs}, or by its hologram when that shows
     * the time left, whichever is less. {@code waveSecs} when no wave is known to be on.
     */
    public double waveSecsLeft(Known k, long now, double waveSecs) {
        long from = waveStart(k);
        double left = from >= 0 ? waveSecs - (now - from) / 20.0 : waveSecs;
        if (k.waveShown >= 0 && k.waveShownAt >= 0 && now - k.waveShownAt < 100) {
            left = Math.min(left, k.waveShown - (now - k.waveShownAt) / 20.0);
        }
        return left;
    }

    /**
     * When the chest's running wave began: its first mob, or its hologram turning to a wave when
     * that came clearly later (a mob of the wave before still counted, out of sight); -1 for none.
     */
    public static long waveStart(Known k) {
        if (k.waveAt >= 0 && !k.guessed && k.state == Frame.ACTIVE && k.activeSeenAt > k.waveAt + 40) {
            return k.activeSeenAt;
        }
        return k.waveAt;
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
