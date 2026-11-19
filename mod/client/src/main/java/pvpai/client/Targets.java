package pvpai.client;

import java.util.HashMap;
import java.util.Map;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityOtherPlayerMP;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.network.NetHandlerPlayClient;
import net.minecraft.scoreboard.Team;
import net.minecraft.util.MathHelper;

/**
 * Whom the agent fights when there are several players about. It keeps the
 * one it fights unless another is a better target by enough: one that hits
 * it while the one it fights does not, or a near one when the one it fights
 * went away. When none hits it, it goes from one that just took a hit to
 * the next in reach that can take one, so that all of those are hit.
 */
final class Targets {
    /** Blocks within which a player is a target at all. */
    static final double RANGE = 24.0;
    /** How far a player can be and still have hit it, with the lag of both. */
    private static final double HIT_REACH = 6.5;
    /** Ticks a player counts as one that hits it after its last hit. */
    private static final int THREAT_TICKS = 40;
    /** Ticks a new target is kept at least, unless it goes. */
    private static final int HOLD = 8;
    /** What staying on the same target is worth, in blocks. */
    private static final double STAY = 1.5;
    /** What hitting it is worth, in blocks. */
    private static final double THREAT = 3.0;
    /** What a player in reach that just took a hit is worse, in blocks, while nobody hits it. */
    private static final double JUST_HIT = 3.5;

    /** The tick of the last hit each player is taken to have given it, by entity number. */
    private final Map<Integer, Integer> hitMe = new HashMap<Integer, Integer>();
    private int tick, held = 1000;

    /**
     * The player of {@code o} as the entity the world holds now, if the
     * server sent it again: a player that goes out of the server's view (a
     * long pearl) is dropped and comes back as a new entity. Null while it
     * is away.
     */
    static EntityOtherPlayerMP same(Minecraft mc, EntityOtherPlayerMP o) {
        if (o == null || mc.theWorld == null) {
            return null;
        }
        for (Object e : mc.theWorld.playerEntities) {
            if (e instanceof EntityOtherPlayerMP && e != o) {
                EntityOtherPlayerMP p = (EntityOtherPlayerMP) e;
                if (o.getUniqueID().equals(p.getUniqueID()) && !p.isDead && p.getHealth() > 0.0F
                        && mc.theWorld.getEntityByID(p.getEntityId()) == p) {
                    return p;
                }
            }
        }
        return null;
    }

    /** Whether the player list, which names players, no longer names this one: it left the server. */
    static boolean unlisted(Minecraft mc, EntityOtherPlayerMP o) {
        NetHandlerPlayClient net = mc.getNetHandler();
        return net != null && !net.getPlayerInfoMap().isEmpty() && net.getPlayerInfo(o.getUniqueID()) == null;
    }

    /** Whether this player is one to fight: there, alive, playing, near and not a teammate it cannot hurt. */
    static boolean valid(Minecraft mc, EntityOtherPlayerMP o) {
        EntityPlayerSP me = mc.thePlayer;
        if (o == null || me == null || mc.theWorld == null || o.isDead || o.getHealth() <= 0.0F
                || mc.theWorld.getEntityByID(o.getEntityId()) != o || o.isSpectator()) {
            return false;
        }
        Team team = me.getTeam();
        if (team != null && !team.getAllowFriendlyFire() && me.isOnSameTeam(o)) {
            return false;
        }
        return me.getDistanceToEntity(o) <= RANGE;
    }

    /**
     * At the start of a tick, with the packets in: if its player was hurt
     * since the last one, the player most likely to have hit it is noted,
     * by where the knockback sends it and who swung.
     */
    void tick(Minecraft mc) {
        tick++;
        held++;
        EntityPlayerSP me = mc.thePlayer;
        if (me.maxHurtTime <= 0 || me.hurtTime != me.maxHurtTime) {
            return;
        }
        double kx = me.motionX, kz = me.motionZ, kl = Math.sqrt(kx * kx + kz * kz);
        EntityOtherPlayerMP best = null;
        double least = Double.MAX_VALUE;
        for (Object e : mc.theWorld.playerEntities) {
            if (!(e instanceof EntityOtherPlayerMP) || !valid(mc, (EntityOtherPlayerMP) e)) {
                continue;
            }
            EntityOtherPlayerMP o = (EntityOtherPlayerMP) e;
            double dx = me.posX - o.posX, dz = me.posZ - o.posZ, d = Math.sqrt(dx * dx + dz * dz);
            if (d > HIT_REACH) {
                continue;
            }
            double s = d;
            if (kl > 0.05 && d > 0.1) {
                // The knockback points away from the one that hit.
                s += 3.0 * (1.0 - (dx * kx + dz * kz) / (d * kl));
            }
            if (o.isSwingInProgress) {
                s -= 2.0;
            }
            if (s < least) {
                least = s;
                best = o;
            }
        }
        if (best != null) {
            hitMe.put(best.getEntityId(), tick);
        }
    }

    /** Each player about as the choice sees it, for a log: name, blocks, degrees to turn, whether it hits, its hurt time. */
    String describe(Minecraft mc) {
        StringBuilder b = new StringBuilder();
        for (Object e : mc.theWorld.playerEntities) {
            if (e instanceof EntityOtherPlayerMP) {
                EntityOtherPlayerMP o = (EntityOtherPlayerMP) e;
                b.append(String.format(java.util.Locale.ROOT, "%s %s %.1f %.0f %s %d; ", o.getName(), valid(mc, o) ? "ok" : "no",
                    mc.thePlayer.getDistanceToEntity(o), turn(mc.thePlayer, o), hits(o) ? "hits" : "-", o.hurtTime));
            }
        }
        return b.toString();
    }

    private boolean hits(EntityOtherPlayerMP o) {
        Integer t = hitMe.get(o.getEntityId());
        return t != null && tick - t <= THREAT_TICKS;
    }

    /** How far the view has to turn to this player, in degrees. */
    static float turn(EntityPlayerSP me, EntityOtherPlayerMP o) {
        float yaw = (float) (Math.atan2(o.posZ - me.posZ, o.posX - me.posX) * 180.0 / Math.PI) - 90.0F;
        return Math.abs(MathHelper.wrapAngleTo180_float(yaw - me.rotationYaw));
    }

    /**
     * The player to fight on this tick, given the one fought until now
     * (null at the start): that one, or another that is better by enough.
     * Null when there is nobody.
     */
    EntityOtherPlayerMP choose(Minecraft mc, EntityOtherPlayerMP now) {
        boolean keep = valid(mc, now);
        if (keep && held < HOLD) {
            return now;
        }
        EntityPlayerSP me = mc.thePlayer;
        boolean threat = false;
        for (Object e : mc.theWorld.playerEntities) {
            if (e instanceof EntityOtherPlayerMP && valid(mc, (EntityOtherPlayerMP) e) && hits((EntityOtherPlayerMP) e)
                    && me.getDistanceToEntity((EntityOtherPlayerMP) e) <= HIT_REACH) {
                threat = true;
            }
        }
        EntityOtherPlayerMP best = null;
        double least = Double.MAX_VALUE;
        for (Object e : mc.theWorld.playerEntities) {
            if (!(e instanceof EntityOtherPlayerMP) || !valid(mc, (EntityOtherPlayerMP) e)) {
                continue;
            }
            EntityOtherPlayerMP o = (EntityOtherPlayerMP) e;
            double d = me.getDistanceToEntity(o);
            double s = d + turn(me, o) / 90.0;
            if (hits(o)) {
                s -= THREAT;
            }
            if (!threat && d < 4.0 && o.hurtTime > 3) {
                s += JUST_HIT;
            }
            if (o == now) {
                s -= STAY;
            }
            if (s < least) {
                least = s;
                best = o;
            }
        }
        if (best == null) {
            return keep ? now : null;
        }
        if (best != now) {
            held = 0;
        }
        return best;
    }
}
