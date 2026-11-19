package pvpai.client;

import java.io.BufferedWriter;
import java.io.File;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.Writer;
import java.text.SimpleDateFormat;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Date;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.zip.GZIPOutputStream;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityOtherPlayerMP;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.multiplayer.ServerData;
import net.minecraft.enchantment.Enchantment;
import net.minecraft.enchantment.EnchantmentHelper;
import net.minecraft.entity.Entity;
import net.minecraft.item.ItemStack;
import net.minecraft.network.Packet;
import net.minecraft.network.play.server.S08PacketPlayerPosLook;
import net.minecraft.network.play.server.S13PacketDestroyEntities;
import net.minecraft.network.play.server.S18PacketEntityTeleport;
import net.minecraft.network.play.server.S19PacketEntityStatus;
import net.minecraft.network.play.server.S28PacketEffect;
import net.minecraft.util.BlockPos;
import org.apache.logging.log4j.LogManager;
import org.apache.logging.log4j.Logger;
import pvpai.core.Duel;
import pvpai.core.Inv;
import pvpai.core.View;

/**
 * Writes down a duel the agent plays as the client sees it, one line a
 * tick and one for each hit, potion and pearl, into a file of its own in
 * pvpai/duels of the game's folder, and sums it up at the end in a line
 * for the game's log that is the last line of the file too. It only reads
 * the game and writes that file: nothing it does reaches the keys, the
 * agent or the server, and whatever goes wrong in it stops the writing,
 * never the duel.
 *
 * <p>What the client can tell and how: a hurt of either player is the
 * server's entity status 2; one of the other player is this one's hit if
 * this one attacked within a second before, one of this one is a hit
 * taken unless its own pearl or a fall did it or the other player is far.
 * A potion broke where the server's effect 2002 says (the block) and, more
 * closely, where the views had it; the server tells the health it gave
 * in the next health update, which comes after that effect: the first
 * change of health after it is the heal, less the hits that came in
 * between. The lines are read by real_duels.py.
 */
final class Recorder {
    private static final Logger LOG = LogManager.getLogger("pvpai");
    /** Own places kept, for where the server had the player some ticks ago. */
    private static final int HISTORY = 64;
    /**
     * Ticks an own throw waits for its potion to break or its pearl to land,
     * and a broken potion for its heal: the server tells the health when it
     * handles the client's next move, which a slow line can hold back.
     */
    private static final int POTION_WAIT = 40, PEARL_WAIT = 100, HEAL_WAIT = 10;
    /** Ticks after an own attack within which a hurt of the other player is taken for its hit. */
    private static final int ATTACK_WINDOW = 20;
    /** Farther than this the other player did not hit this one: something else hurt it. */
    private static final double HIT_FAR = 8.0;
    /** What a pearl's landing hurts its thrower for, before Protection and Feather Falling. */
    private static final double PEARL_DAMAGE = 5.0;
    private static final int FLUSH_TICKS = 100;

    private final Minecraft mc;
    private final EntityPlayerSP me;
    private EntityOtherPlayerMP other;
    private final double duelPing;
    private final Tally tally = new Tally();
    private Writer out;
    private String fileName = "-";
    private boolean broken, closed;
    /** The tick under way: the packets handled now belong to it, and it is written at its end. */
    private int tick;

    private final double[][] feet = new double[HISTORY][3];
    private double maxHp = 20.0;
    /** The health as last seen, between two packets or at the end of a tick. */
    private double known;
    private int ping = -1;
    private int lastAttack = -1000, lastOwnPearl = -1000, lastOtherJump = -1000, bigLanding = -1000;
    private float fallBefore;
    private View lastView;
    private int potionsLeft = -1;
    private boolean ownDeath, otherDeath;

    /**
     * A hurt of this player (with what did it) or of the other (what did
     * it is told two ticks later, when a pearl of its own that hurt it has
     * been seen to land), in the order they came.
     */
    private static final class Hurt {
        /** Its tick, and the ticks since this player's last attack before it. */
        int tick, sinceAttack;
        boolean own;
        String cause;
    }

    private final List<Hurt> hurts = new ArrayList<Hurt>();
    /** Ticks the other player was put more than three blocks away at once: its pearls. */
    private final List<Integer> jumps = new ArrayList<Integer>();
    /** Entity statuses 2 (hurt) of the other player and of this one in the packets of this tick. */
    private int otherHurtNow, ownHurtNow;
    private boolean teleported, otherJumped;
    private double[] beforeTeleport;

    /** The health plain hits of the other player took lately, as the changes of health showed them. */
    private final double[] hitCosts = new double[8];
    private int hitCostsN;
    private final double hitHealth;
    private double pearlCost = PEARL_DAMAGE;
    /** Own hurts since the last change of health, what they are reckoned to cost, and whether all were hits. */
    private int unseen;
    private double unseenCost;
    private boolean unseenHits = true;

    /** A potion or pearl in flight as the last view had it. */
    private static final class Flying {
        int kind;
        boolean mine;
        int seen;
        final double[] pos = new double[3], motion = new double[3];
    }

    private final Map<Long, Flying> flying = new LinkedHashMap<Long, Flying>();
    /**
     * Potions the effect of a breaking potion was told of, by the tick: the
     * client keeps showing them until the server takes them away, a tick or
     * so later.
     */
    private final Map<Long, Integer> gone = new LinkedHashMap<Long, Integer>();
    /** Ticks of own potions thrown that have not broken yet, oldest first; own pearls with where they were thrown from. */
    private final List<Integer> potionThrows = new ArrayList<Integer>();
    private final List<double[]> pearlThrows = new ArrayList<double[]>();

    /** A potion of its own that broke and whose heal is still to come in. */
    private static final class Pending {
        int tick;
        double before, missing;
        int guess;
        String line;
    }

    private final List<Pending> pending = new ArrayList<Pending>();
    /** Hurts since the first of {@link #pending} broke, and the health they are reckoned to have cost. */
    private int hurtsInHeal;
    private double hurtCost;
    /** An own pearl landed while a heal was still to come: what it took is not known. */
    private boolean pearlInHeal;

    private Recorder(Minecraft mc, EntityOtherPlayerMP other, float hitHealth, double ping) {
        this.mc = mc;
        this.me = mc.thePlayer;
        this.other = other;
        this.hitHealth = hitHealth;
        this.duelPing = ping;
    }

    /** For the duel that starts now. Never fails: without a file it still sums the duel up in the log. */
    static Recorder open(Minecraft mc, EntityOtherPlayerMP other, Duel d, double ping, double otherPing, double cps, boolean block) {
        Recorder r = new Recorder(mc, other, d.hitHealth, ping);
        try {
            r.start(d, ping, otherPing, cps, block);
        } catch (Throwable t) {
            r.out = null;
            LOG.warn("PvP AI: no duel file: " + t);
        }
        return r;
    }

    private void start(Duel d, double ping, double otherPing, double cps, boolean block) throws Exception {
        known = me.getHealth();
        maxHp = me.getMaxHealth();
        pearlCost = fallCost(me, PEARL_DAMAGE);
        for (double[] f : feet) {
            f[0] = me.posX;
            f[1] = me.posY;
            f[2] = me.posZ;
        }
        Date now = new Date();
        File dir = new File(new File(mc.mcDataDir, "pvpai"), "duels");
        dir.mkdirs();
        String base = new SimpleDateFormat("yyyyMMdd-HHmmss", Locale.ROOT).format(now) + "-" + clean(other.getName()).replace('.', '_').replace(':', '_');
        File f = new File(dir, base + ".log.gz");
        for (int k = 2; f.exists() && k < 100; k++) {
            f = new File(dir, base + "-" + k + ".log.gz");
        }
        out = new BufferedWriter(new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(f), 8192, true), "UTF-8"), 1 << 15);
        fileName = f.getName();
        ServerData server = mc.getCurrentServerData();
        StringBuilder b = new StringBuilder("# pvpai duel 1\n# start ");
        b.append(new SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ss", Locale.ROOT).format(now));
        b.append(" me=").append(clean(me.getName())).append(" vs=").append(clean(other.getName()));
        b.append(" server=").append(server == null ? (mc.isIntegratedServerRunning() ? "local" : "-") : clean(server.serverIP));
        b.append(" ping=").append(Math.round(ping)).append(" oping=").append(Math.round(otherPing));
        b.append(" cps=");
        num(b, cps, 2);
        b.append(" hit=");
        num(b, d.hitHealth, 3);
        b.append(" arena=");
        num(b, d.centerX, 2);
        b.append(',');
        num(b, d.centerZ, 2);
        b.append(',');
        num(b, d.arenaHalf, 2);
        b.append(" floor=");
        num(b, d.floorY, 3);
        b.append(" pearlcd=").append(d.pearlCooldown).append(" block=").append(block ? 1 : 0);
        b.append(" maxhp=");
        num(b, maxHp, 2);
        b.append('\n');
        b.append("# T tick x y z yaw pitch ground hurt hp ox oy oz oyaw opitch oground ohurt ohp ohurts hurts flags action ping\n");
        out.write(b.toString());
    }

    /**
     * The health a fall of this much damage takes with the armour worn:
     * Protection and Feather Falling, on average over what they draw
     * (EnchantmentProtection.calcModifierDamage); a fall goes through the
     * armour itself.
     */
    private static double fallCost(EntityPlayerSP p, double damage) {
        int m = 0;
        for (ItemStack s : p.inventory.armorInventory) {
            if (s == null) {
                continue;
            }
            int prot = EnchantmentHelper.getEnchantmentLevel(Enchantment.protection.effectId, s);
            int ff = EnchantmentHelper.getEnchantmentLevel(Enchantment.featherFalling.effectId, s);
            m += prot > 0 ? (int) Math.floor((6 + prot * prot) / 3.0F * 0.75F) : 0;
            m += ff > 0 ? (int) Math.floor((6 + ff * ff) / 3.0F * 2.5F) : 0;
        }
        m = Math.min(m, 25);
        double k = Math.min(((m + 1) >> 1) + (m >> 1) * 0.5, 20.0);
        return damage * (25.0 - k) / 25.0;
    }

    private static String clean(String s) {
        if (s == null || s.isEmpty()) {
            return "-";
        }
        StringBuilder b = new StringBuilder();
        for (int i = 0; i < s.length() && b.length() < 40; i++) {
            char c = s.charAt(i);
            b.append(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '_' || c == '-' || c == '.' || c == ':' ? c : '_');
        }
        return b.toString();
    }

    private void fail(Throwable t) {
        if (!broken) {
            LOG.warn("PvP AI: the duel recorder stopped: " + t);
        }
        broken = true;
        try {
            if (out != null) {
                out.close();
            }
        } catch (Throwable again) {
            // Nothing left to save.
        }
        out = null;
    }

    private void write(String line) throws Exception {
        if (out != null) {
            out.write(line);
            out.write('\n');
        }
    }

    // Packets: called before and after the game handles each the agent goes by.

    void before(Packet<?> p) {
        if (broken || closed) {
            return;
        }
        try {
            observe();
            if (p instanceof S19PacketEntityStatus) {
                status((S19PacketEntityStatus) p);
            } else if (p instanceof S08PacketPlayerPosLook) {
                beforeTeleport = new double[] {me.posX, me.posY, me.posZ};
            } else if (p instanceof S18PacketEntityTeleport) {
                S18PacketEntityTeleport t = (S18PacketEntityTeleport) p;
                if (t.getEntityId() == other.getEntityId()) {
                    double[] was = Reader.target(other);
                    double[] at = {t.getX() / 32.0, t.getY() / 32.0, t.getZ() / 32.0};
                    if (dist2(at, was) > 9.0) {
                        // More than three blocks at once: a pearl.
                        otherJumped = true;
                        jumps.add(tick);
                        write(point(new StringBuilder("O ").append(tick), at).toString());
                    }
                }
            } else if (p instanceof S28PacketEffect) {
                S28PacketEffect e = (S28PacketEffect) p;
                if (e.getSoundType() == 2002) {
                    BlockPos b = e.getSoundPos();
                    broke(b.getX(), b.getY(), b.getZ());
                }
            } else if (p instanceof S13PacketDestroyEntities) {
                for (int id : ((S13PacketDestroyEntities) p).getEntityIDs()) {
                    gone.remove((long) id);
                    Flying f = flying.remove((long) id);
                    if (f != null && f.kind == Inv.KIND_POTION && f.mine) {
                        // Gone without the effect of a potion that breaks: taken for broken where it was.
                        splash(true, "noeff", ahead(f), null);
                    }
                }
            }
        } catch (Throwable t) {
            fail(t);
        }
    }

    private void status(S19PacketEntityStatus s) throws Exception {
        Entity e = s.getEntity(mc.theWorld);
        if (e != me && e != other) {
            return;
        }
        if (s.getOpCode() == 2) {
            Hurt h = new Hurt();
            h.tick = tick;
            h.sinceAttack = tick - lastAttack;
            h.own = e == me;
            if (h.own) {
                ownHurtNow++;
                // An own pearl hurts its thrower right after it puts it where it landed.
                boolean far = Math.hypot(other.posX - me.posX, other.posZ - me.posZ) > HIT_FAR;
                h.cause = tick - lastOwnPearl <= 1 ? "pearl" : tick - bigLanding <= rttTicks() + 4 ? "fall" : far ? "far" : "hit";
                double cost = h.cause.equals("pearl") ? pearlCost : hitCost();
                if (!pending.isEmpty()) {
                    hurtsInHeal++;
                    hurtCost += cost;
                } else {
                    unseen++;
                    unseenCost += cost;
                    unseenHits &= h.cause.equals("hit");
                }
            } else {
                otherHurtNow++;
            }
            hurts.add(h);
        } else if (s.getOpCode() == 3) {
            if (e == me) {
                ownDeath = true;
            } else {
                otherDeath = true;
            }
            write("D " + tick + (e == me ? " me" : " opp"));
        }
    }

    void after(Packet<?> p) {
        if (broken || closed) {
            return;
        }
        try {
            if (p instanceof S08PacketPlayerPosLook && beforeTeleport != null) {
                teleported();
            }
        } catch (Throwable t) {
            fail(t);
        }
    }

    /** The server put this player somewhere: where an own pearl landed, or a correction. */
    private void teleported() throws Exception {
        teleported = true;
        double[] at = {me.posX, me.posY, me.posZ};
        double moved = Math.sqrt(dist2(at, beforeTeleport));
        beforeTeleport = null;
        boolean pearlNear = false;
        for (Flying f : flying.values()) {
            pearlNear |= f.kind == Inv.KIND_PEARL && f.mine && dist2(f.pos, at) < 9.0;
        }
        StringBuilder b = new StringBuilder();
        if (!pearlThrows.isEmpty() && (moved > 1.0 || pearlNear)) {
            double[] from = pearlThrows.remove(0);
            lastOwnPearl = tick;
            // It hurts its thrower now with a hurt status of its own, or,
            // while a hit leaves the thrower invulnerable, by what it does
            // beyond that hit (nothing after a sword's) without one.
            pearlInHeal |= !pending.isEmpty();
            String where = Tally.where(at[0], at[2], from[1], from[3], other.posX, other.posZ);
            boolean faced = Tally.intoFacing(at[0], at[2], other.posX, other.posZ, other.rotationYaw);
            tally.landed(where, faced);
            point(b.append("L ").append(tick), at);
            b.append(' ');
            num(b, from[1], 3);
            b.append(' ');
            num(b, from[3], 3);
            b.append(' ');
            num(b, other.posX, 3);
            b.append(' ');
            num(b, other.posZ, 3);
            b.append(' ');
            num(b, other.rotationYaw, 1);
            b.append(' ');
            num(b, Math.hypot(at[0] - other.posX, at[2] - other.posZ), 2);
            b.append(' ').append(where).append(' ').append(faced ? 1 : 0);
        } else {
            point(b.append("M ").append(tick), at);
            b.append(' ');
            num(b, moved, 2);
        }
        write(b.toString());
    }

    /** Another player is the opponent from now on. */
    void target(EntityOtherPlayerMP o) {
        if (broken || closed) {
            return;
        }
        try {
            other = o;
            write("N " + tick + " " + clean(o.getName()));
        } catch (Throwable t) {
            fail(t);
        }
    }

    // Health.

    /** What a plain hit of the other player costs now: the median of the last ones seen, or the reckoning of the duel's start. */
    private double hitCost() {
        if (hitCostsN == 0) {
            return hitHealth;
        }
        int n = Math.min(hitCostsN, hitCosts.length);
        double[] c = Arrays.copyOf(hitCosts, n);
        Arrays.sort(c);
        return n % 2 == 1 ? c[n / 2] : (c[n / 2 - 1] + c[n / 2]) / 2.0;
    }

    /** Looks whether the health changed since it was last seen. */
    private void observe() throws Exception {
        double h = me.getHealth();
        if (h == known) {
            return;
        }
        double delta = h - known;
        known = h;
        if (!pending.isEmpty()) {
            heal(delta + hurtCost, hurtsInHeal > 0 || pearlInHeal);
        } else if (unseen > 0 && unseenHits && delta < 0.0) {
            hitCosts[hitCostsN++ % hitCosts.length] = -delta / unseen;
        }
        unseen = 0;
        unseenCost = 0.0;
        unseenHits = true;
    }

    /** The heal of the potions waiting for it, shared out in the order they broke. */
    private void heal(double total, boolean mixed) throws Exception {
        double room = Math.max(total, 0.0), given = 0.0;
        for (Pending p : pending) {
            double h = Math.min(Math.min(room, Tally.POTION_HEAL), Math.max(p.missing - given, 0.0));
            settle(p, h, Math.max(p.missing - given, 0.0), mixed);
            room -= h;
            given += h;
        }
        pending.clear();
        hurtsInHeal = 0;
        hurtCost = 0.0;
        pearlInHeal = false;
    }

    private void settle(Pending p, double healed, double missing, boolean mixed) throws Exception {
        double[] split = tally.splash(healed, missing, p.guess, mixed);
        StringBuilder b = new StringBuilder(p.line);
        b.append(' ');
        num(b, p.before, 2);
        b.append(' ');
        num(b, missing, 2);
        b.append(' ');
        num(b, healed, 2);
        b.append(' ');
        num(b, split[0], 2);
        b.append(' ');
        num(b, split[1], 2);
        b.append(' ');
        num(b, split[2], 2);
        b.append(' ').append(tick - p.tick).append(mixed ? " mixed" : healed < missing - 0.01 ? " exact" : " capped");
        write(b.toString());
    }

    // Potions.

    private int rttTicks() {
        double ms = ping >= 0 ? ping : duelPing;
        return (int) Math.max(0, Math.min(Math.round(ms / 50.0), HISTORY - 8));
    }

    /** Where the server had the player's feet when something broke that the client hears of now. */
    private double[] serverFeet() {
        int back = Math.min(1 + rttTicks(), tick);
        return feet[((tick - back) % HISTORY + HISTORY) % HISTORY];
    }

    /** Where the server had a potion at the start of the tick it broke in: the last view's place of it moved on by the ticks since. */
    private double[] ahead(Flying f) {
        double[] at = f.pos.clone(), v = f.motion.clone();
        for (int k = f.seen + 1; k < tick; k++) {
            for (int a = 0; a < 3; a++) {
                at[a] += v[a];
                v[a] *= 0.99;
            }
            v[1] -= 0.05;
        }
        return at;
    }

    /** The effect of a potion breaking, in this block: which of the potions in flight it was, and where in the block. */
    private void broke(int bx, int by, int bz) throws Exception {
        Long best = null;
        double[] at = null;
        double least = 4.0;
        for (Map.Entry<Long, Flying> en : flying.entrySet()) {
            Flying f = en.getValue();
            if (f.kind != Inv.KIND_POTION) {
                continue;
            }
            double[] a = ahead(f);
            double dx = bx + 0.5 - a[0], dy = by + 0.5 - a[1], dz = bz + 0.5 - a[2];
            double d = dx * dx + dy * dy + dz * dz;
            if (d < least) {
                least = d;
                best = en.getKey();
                at = a;
            }
        }
        int[] block = {bx, by, bz};
        if (best != null) {
            Flying f = flying.remove(best);
            gone.put(best, tick);
            for (int a = 0; a < 3; a++) {
                at[a] = Math.min(Math.max(at[a], block[a] + 0.001), block[a] + 0.999);
            }
            splash(f.mine, "view", at, block);
            return;
        }
        // One the client never had in a view: its own if one of its throws
        // is still waiting and it broke near enough.
        at = new double[] {bx + 0.5, by + 0.25, bz + 0.5};
        boolean mine = !potionThrows.isEmpty() && Math.sqrt(dist2(at, serverFeet())) < 5.0;
        splash(mine, "block", at, block);
    }

    private void splash(boolean mine, String src, double[] at, int[] block) throws Exception {
        double[] f = serverFeet();
        double d = Math.sqrt(dist2(at, f));
        // The player's box has to be within two blocks up or down of the potion's.
        boolean reach = f[1] < at[1] + 2.25 && f[1] + 1.8 > at[1] - 2.0;
        int guess = reach ? Tally.offered(d) : 0;
        StringBuilder b = new StringBuilder("S ").append(tick).append(mine ? " me " : " opp ").append(src);
        point(b, at);
        b.append(' ').append(block == null ? "-" : block[0] + "," + block[1] + "," + block[2]);
        b.append(' ');
        num(b, d, 2);
        b.append(' ').append(guess).append(' ');
        num(b, Math.hypot(at[0] - other.posX, at[2] - other.posZ), 2);
        if (!mine) {
            write(b.toString());
            return;
        }
        if (!potionThrows.isEmpty()) {
            potionThrows.remove(0);
        }
        if (pending.isEmpty()) {
            // Hurts whose change of health has not come yet come with the heal's.
            hurtsInHeal = unseen;
            hurtCost = unseenCost;
            pearlInHeal = tick - lastOwnPearl <= 2;
            unseen = 0;
            unseenCost = 0.0;
            unseenHits = true;
        }
        Pending p = new Pending();
        p.tick = tick;
        // The health the server had when it broke, as far as the client knows.
        p.before = Math.max(known - hurtCost, 0.0);
        p.missing = Math.max(maxHp - p.before, 0.0);
        p.guess = guess;
        p.line = b.toString();
        pending.add(p);
    }

    /** Own throws of this tick, and the ones that waited too long. */
    private void throwsOf(View view, boolean threwPearl, boolean threwPotion) throws Exception {
        while (!potionThrows.isEmpty() && tick - potionThrows.get(0) > POTION_WAIT) {
            potionThrows.remove(0);
        }
        while (!pearlThrows.isEmpty() && tick - pearlThrows.get(0)[0] > PEARL_WAIT) {
            pearlThrows.remove(0);
            write("L " + tick + " lost");
        }
        if (threwPotion) {
            // As the client had its health when it threw.
            double hp = view != null ? view.health : me.getHealth();
            double u = tally.thrown(hp, maxHp, potionThrows.size());
            StringBuilder b = new StringBuilder("P ").append(tick).append(' ');
            num(b, hp, 2);
            b.append(' ');
            num(b, maxHp - hp, 2);
            b.append(' ').append(maxHp - hp < Tally.POTION_HEAL ? 1 : 0).append(' ');
            num(b, u, 2);
            b.append(' ').append(potionThrows.size()).append(' ');
            num(b, me.rotationYaw, 1);
            b.append(' ');
            num(b, me.rotationPitch, 1);
            write(b.toString());
            potionThrows.add(tick);
        }
        if (threwPearl) {
            tally.pearls++;
            double[] at = {me.posX, me.posY, me.posZ};
            pearlThrows.add(new double[] {tick, at[0], at[1], at[2]});
            StringBuilder b = point(new StringBuilder("E ").append(tick), at);
            b.append(' ');
            num(b, me.rotationYaw, 1);
            b.append(' ');
            num(b, me.rotationPitch, 1);
            b.append(' ');
            num(b, Math.hypot(other.posX - me.posX, other.posZ - me.posZ), 2);
            write(b.toString());
        }
    }

    /** What a fresh view has of the potions and pearls in flight and of the potions left. */
    private void seen(View v) {
        for (View.Thrown t : v.thrown) {
            if (t.kind != Inv.KIND_POTION && t.kind != Inv.KIND_PEARL || gone.containsKey(t.id)) {
                continue;
            }
            Flying f = flying.get(t.id);
            if (f == null) {
                f = new Flying();
                f.kind = t.kind;
                flying.put(t.id, f);
            }
            f.mine = t.mine;
            f.seen = tick;
            System.arraycopy(t.serverSide, 0, f.pos, 0, 3);
            System.arraycopy(t.motion, 0, f.motion, 0, 3);
        }
        for (Iterator<Flying> it = flying.values().iterator(); it.hasNext();) {
            if (tick - it.next().seen > 200) {
                it.remove();
            }
        }
        for (Iterator<Integer> it = gone.values().iterator(); it.hasNext();) {
            if (tick - it.next() > 100) {
                it.remove();
            }
        }
        int n = Integer.bitCount(v.potions);
        for (int r : v.rows) {
            n += r == 1 ? 1 : 0;
        }
        potionsLeft = n;
    }

    // The tick.

    /**
     * At the end of each tick the agent played, with the view it went by
     * (the same object as the tick before if it did not look again), what
     * it chose and what came of the keys.
     */
    void tick(View view, int[] action, boolean attacked, boolean swung, boolean threwPearl, boolean threwPotion, boolean screen, EntityOtherPlayerMP o) {
        if (broken || closed) {
            return;
        }
        try {
            other = o;
            boolean fresh = view != null && view != lastView;
            lastView = view;
            observe();
            hurts(tick - 2);
            for (Iterator<Pending> it = pending.iterator(); it.hasNext();) {
                Pending p = it.next();
                // No change of health came: it gave nothing (the server tells only a change).
                if (hurtsInHeal == 0 && !pearlInHeal && (p.missing <= 0.0 || tick - p.tick >= HEAL_WAIT)) {
                    settle(p, 0.0, p.missing, false);
                    it.remove();
                }
            }
            if (!pending.isEmpty() && tick - pending.get(0).tick >= 2 * HEAL_WAIT) {
                // Hurts came and no change of health: it gave what they took.
                heal(hurtCost, true);
            }
            throwsOf(view, threwPearl, threwPotion);
            if (fresh) {
                seen(view);
            }
            if (tick % 20 == 0) {
                double ms = Scout.ping(mc, me);
                ping = ms < 0 ? -1 : (int) Math.round(ms);
            }
            line(fresh ? action : null, attacked, swung, threwPearl, threwPotion, screen);
            feet[tick % HISTORY][0] = me.posX;
            feet[tick % HISTORY][1] = me.posY;
            feet[tick % HISTORY][2] = me.posZ;
            if (attacked || swung && view != null && view.onTarget) {
                lastAttack = tick;
            }
            if (!me.onGround) {
                fallBefore = me.fallDistance;
            } else {
                if (fallBefore > 3.0F) {
                    bigLanding = tick;
                }
                fallBefore = 0.0F;
            }
            tick++;
            tally.ticks = tick;
            if (out != null && tick % FLUSH_TICKS == 0) {
                out.flush();
            }
        } catch (Throwable t) {
            fail(t);
        }
    }

    /**
     * The hurts up to this tick, in the order they came: told what did
     * them, written down and counted. One of the other player within a
     * tick before or two after it was seen to land a pearl was that pearl;
     * else it was this player's hit if this one attacked within a second
     * before it.
     */
    private void hurts(int upTo) throws Exception {
        if (otherJumped) {
            lastOtherJump = tick;
        }
        otherJumped = false;
        while (!jumps.isEmpty() && jumps.get(0) < upTo - 10) {
            jumps.remove(0);
        }
        while (!hurts.isEmpty() && hurts.get(0).tick <= upTo) {
            Hurt h = hurts.remove(0);
            if (!h.own) {
                boolean jumped = false;
                for (int j : jumps) {
                    jumped |= j >= h.tick - 1 && j <= h.tick + 2;
                }
                h.cause = jumped ? "pearl" : h.sinceAttack <= ATTACK_WINDOW ? "hit" : "noatk";
            }
            if (h.cause.equals("hit")) {
                tally.hit(h.tick, h.own ? 1 : 0);
            }
            write("H " + h.tick + (h.own ? " K " : " L ") + h.cause);
        }
    }

    private static final String DIGITS = "0123456789abcdefghijklmnopqrstuvwxyz";

    private void line(int[] action, boolean attacked, boolean swung, boolean threwPearl, boolean threwPotion, boolean screen) throws Exception {
        StringBuilder b = new StringBuilder(160).append("T ").append(tick);
        player(b, me.posX, me.posY, me.posZ, me.rotationYaw, me.rotationPitch, me.onGround, me.hurtTime, me.getHealth());
        player(b, other.posX, other.posY, other.posZ, other.rotationYaw, other.rotationPitch, other.onGround, other.hurtTime, other.getHealth());
        b.append(' ').append(otherHurtNow).append(' ').append(ownHurtNow).append(' ');
        otherHurtNow = 0;
        ownHurtNow = 0;
        int at = b.length();
        if (attacked) {
            b.append('a');
        }
        if (swung) {
            b.append('s');
        }
        if (threwPotion) {
            b.append('p');
        }
        if (threwPearl) {
            b.append('e');
        }
        if (screen) {
            b.append('i');
        }
        if (me.isUsingItem()) {
            b.append('u');
        }
        if (teleported) {
            b.append('m');
        }
        if (lastOtherJump == tick) {
            b.append('j');
        }
        if (b.length() == at) {
            b.append('-');
        }
        teleported = false;
        b.append(' ');
        if (action == null) {
            b.append('-');
        } else {
            for (int a : action) {
                b.append(a >= 0 && a < DIGITS.length() ? DIGITS.charAt(a) : '?');
            }
        }
        b.append(' ').append(ping);
        write(b.toString());
    }

    private static void player(StringBuilder b, double x, double y, double z, float yaw, float pitch, boolean ground, int hurt, float hp) {
        point(b, new double[] {x, y, z});
        b.append(' ');
        num(b, yaw, 1);
        b.append(' ');
        num(b, pitch, 1);
        b.append(' ').append(ground ? 1 : 0).append(' ').append(hurt).append(' ');
        num(b, hp, 2);
    }

    private static StringBuilder point(StringBuilder b, double[] p) {
        for (double v : p) {
            b.append(' ');
            num(b, v, 3);
        }
        return b;
    }

    /**
     * The duel is over, for this reason (the line the player was shown,
     * or null when the player took the keys back): what is left of it is
     * counted, the summary goes to the log and closes the file.
     */
    void close(String why) {
        if (closed || broken) {
            closed = true;
            return;
        }
        try {
            // The packets that ended it came after the last tick.
            observe();
            hurts(Integer.MAX_VALUE);
            if (!pending.isEmpty()) {
                heal(hurtCost, hurtsInHeal > 0 || pearlInHeal);
            }
            closed = true;
            boolean died = ownDeath || me.isDead || me.getHealth() <= 0.0F;
            boolean killed = !died && (otherDeath || other.getHealth() <= 0.0F);
            if (died || killed) {
                write("D " + tick + (died ? " me" : " opp") + " end");
            }
            String result = died ? "loss" : killed ? "win" : "none";
            String end = why == null ? "off" : why.contains("murio") ? "died" : why.contains("rival") ? "gone" : why.contains("mundo") ? "world"
                : why.contains("pantalla") ? "screen" : "other";
            tally.end();
            tally.left = potionsLeft;
            StringBuilder b = new StringBuilder(tally.summary(clean(other.getName()), result, end, died));
            b.append(" hp=");
            num(b, me.getHealth(), 2);
            b.append(" ohp=");
            num(b, other.getHealth(), 2);
            b.append(" ping=").append(ping).append(" file=").append(fileName);
            String summary = b.toString();
            LOG.info(summary);
            write(summary);
            if (out != null) {
                out.close();
            }
            out = null;
        } catch (Throwable t) {
            closed = true;
            fail(t);
        }
    }

    private static double dist2(double[] a, double[] b) {
        double dx = a[0] - b[0], dy = a[1] - b[1], dz = a[2] - b[2];
        return dx * dx + dy * dy + dz * dz;
    }

    /** A number with at most this many decimals (1 to 3), without trailing zeros. */
    static void num(StringBuilder b, double v, int decimals) {
        if (Double.isNaN(v) || Double.isInfinite(v)) {
            b.append("nan");
            return;
        }
        long scale = decimals <= 1 ? 10 : decimals == 2 ? 100 : 1000;
        long r = Math.round(v * scale);
        if (r < 0) {
            b.append('-');
            r = -r;
        }
        b.append(r / scale);
        long frac = r % scale;
        if (frac != 0) {
            String s = Long.toString(frac + scale).substring(1);
            int end = s.length();
            while (s.charAt(end - 1) == '0') {
                end--;
            }
            b.append('.').append(s, 0, end);
        }
    }
}
