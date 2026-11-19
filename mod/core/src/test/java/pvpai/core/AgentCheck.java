package pvpai.core;

import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.DoubleBuffer;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.Arrays;
import java.util.Random;

/**
 * Plays back the duels the simulator wrote with
 * {@code cargo run --release --example agent_dump -- FILE} through this
 * agent, and compares the counts it keeps, what it shows the policy and
 * the keys it presses with the simulator's, number by number:
 * {@code java pvpai.core.AgentCheck FILE}.
 *
 * A file of format 6 ({@code cargo run --release --example fh_dump}) is of
 * an agent with the fullheal macro on ({@link Agent#fullHeal}): it has the
 * movement state of the client's own player as well, which goes to
 * {@link View#walk}, the plan under way after each tick's keys, which is
 * compared with this agent's, and the movement state after the tick,
 * which is compared with what {@link Walk#tick} makes of the state before
 * on every tick it applies to (no item in use before or after, no hit, no
 * inventory). Format 7 has the potion guard and the decision rule of the
 * agent in its head as well ({@link Agent#potionGuard},
 * {@link Agent#potionRule}).
 */
public final class AgentCheck {
    private static DoubleBuffer d;

    private static double n() {
        return d.get();
    }

    private static int i() {
        return (int) d.get();
    }

    private static boolean b() {
        return d.get() != 0.0;
    }

    private static void v(double[] out) {
        d.get(out, 0, 3);
    }

    /** The movement state beyond the view (fh_dump.rs, walk). */
    private static void walk(Walk w) {
        v(w.min);
        v(w.max);
        w.inWeb = b();
        w.attrBase = n();
        w.attrSprint = b();
        w.speedModifier = n();
        w.jumpFactor = (float) n();
        w.moveForward = (float) n();
        w.sprintingTicksLeft = i();
        w.sprintToggleTimer = i();
        v(w.said);
        w.saidYaw = (float) n();
        w.saidPitch = (float) n();
        w.positionUpdateTicks = i();
    }

    /** The whole movement state after a tick (fh_dump.rs, walk_after). */
    private static Walk walkAfter() {
        Walk w = new Walk();
        v(w.pos);
        v(w.motion);
        w.onGround = b();
        w.collidedH = b();
        w.fallDistance = (float) n();
        w.yaw = (float) n();
        w.pitch = (float) n();
        w.sprinting = b();
        w.jumpTicks = i();
        w.serverSprint = b();
        w.usingItem = b();
        walk(w);
        return w;
    }

    private static boolean same(double a, double b) {
        return Double.doubleToRawLongBits(a) == Double.doubleToRawLongBits(b);
    }

    private static boolean same3(double[] a, double[] b) {
        return same(a[0], b[0]) && same(a[1], b[1]) && same(a[2], b[2]);
    }

    /** What of two movement states differs, empty if nothing. */
    private static String walkDiff(Walk got, Walk want) {
        StringBuilder s = new StringBuilder();
        if (!same3(got.pos, want.pos)) s.append(" pos ").append(Arrays.toString(got.pos)).append(" want ").append(Arrays.toString(want.pos));
        if (!same3(got.motion, want.motion)) s.append(" motion ").append(Arrays.toString(got.motion)).append(" want ").append(Arrays.toString(want.motion));
        if (!same3(got.min, want.min) || !same3(got.max, want.max)) s.append(" box");
        if (got.onGround != want.onGround) s.append(" onGround");
        if (got.collidedH != want.collidedH) s.append(" collidedH");
        if (got.inWeb != want.inWeb) s.append(" inWeb");
        if (!same(got.fallDistance, want.fallDistance)) s.append(" fall ").append(got.fallDistance).append(" want ").append(want.fallDistance);
        if (!same(got.yaw, want.yaw) || !same(got.pitch, want.pitch)) s.append(" view");
        if (got.sprinting != want.sprinting || got.attrSprint != want.attrSprint) s.append(" sprint");
        if (!same(got.attrBase, want.attrBase) || !same(got.speedModifier, want.speedModifier)) s.append(" attribute");
        if (!same(got.jumpFactor, want.jumpFactor)) s.append(" jumpFactor");
        if (got.jumpTicks != want.jumpTicks) s.append(" jumpTicks");
        if (!same(got.moveForward, want.moveForward)) s.append(" moveForward");
        if (got.sprintingTicksLeft != want.sprintingTicksLeft || got.sprintToggleTimer != want.sprintToggleTimer) s.append(" sprintTimers");
        if (got.serverSprint != want.serverSprint) s.append(" serverSprint");
        if (!same3(got.said, want.said) || !same(got.saidYaw, want.saidYaw) || !same(got.saidPitch, want.saidPitch)) s.append(" said");
        if (got.positionUpdateTicks != want.positionUpdateTicks) s.append(" positionUpdateTicks");
        return s.toString();
    }

    /** The plan of this agent after the keys against the simulator's (fh_dump.rs, plan): what differs, empty if nothing. */
    private static String planDiff(Agent agent) {
        int t = i();
        int got = agent.healPlanTick();
        StringBuilder s = new StringBuilder();
        if (t != got) {
            s.append(" plan tick ").append(got).append(" want ").append(t);
        }
        if (t == 1) {
            FullHeal.Plan p = agent.healPlan();
            int dy = i();
            float yaw = (float) n();
            int len = i(), ticks = i(), cls = i(), window = i();
            float pitch = (float) n();
            int jump = i();
            int[] keys = new int[4 * FullHeal.PLAN_TICKS];
            for (int k = 0; k < keys.length; k++) {
                keys[k] = i();
            }
            if (p != null && got == 1) {
                int[] mine = new int[keys.length];
                for (int k = 0; k < FullHeal.PLAN_TICKS; k++) {
                    mine[4 * k] = p.keys[k].fw;
                    mine[4 * k + 1] = p.keys[k].st;
                    mine[4 * k + 2] = p.keys[k].sprint ? 1 : 0;
                    mine[4 * k + 3] = p.keys[k].jump ? 1 : 0;
                }
                if (p.dy != dy || !same(p.yaw, yaw) || p.len != len || p.ticks != ticks || p.cls != cls || p.window != window
                        || !same(p.pitch, pitch) || p.jump != jump || !Arrays.equals(mine, keys)) {
                    s.append(" plan (dy ").append(p.dy).append(" pitch ").append(p.pitch).append(" ticks ").append(p.ticks).append(" class ")
                        .append(p.cls).append(" window ").append(p.window).append(" jump ").append(p.jump).append(") want (dy ").append(dy)
                        .append(" pitch ").append(pitch).append(" ticks ").append(ticks).append(" class ").append(cls).append(" window ")
                        .append(window).append(" jump ").append(jump).append(")");
                }
            }
        }
        return s.toString();
    }

    private static void view(View v) {
        v(v.pos);
        v(v.motion);
        v.onGround = b();
        v.collidedH = b();
        v.fallDistance = (float) n();
        v.yaw = (float) n();
        v.pitch = (float) n();
        v.sprinting = b();
        v.jumpTicks = i();
        v.speed = n();
        v.serverSprint = b();
        v.hurtTime = i();
        v(v.lastKb);
        v.ticksSinceKb = i();
        v.ticksSinceSwing = i();
        v.ticksSinceAttack = i();
        v.ticksSinceHurt = i();
        v.ticksSinceMoved = i();
        v.usingItem = b();
        v.rightClickDelay = i();
        v.flagUsing = b();
        v.held = i();
        v.pearls = i();
        v.potions = i();
        v.drinks = i();
        v.bottles = i();
        v.steak = i();
        v.potionsSeen = i();
        v.potionsPending = i();
        v.food = i();
        v.saturation = (float) n();
        v.speedTicks = i();
        v.health = (float) n();
        boolean consuming = b();
        v.useSlot = i();
        v.useCount = i();
        v.invOpen = b();
        for (int k = 0; k < 27; k++) {
            v.rows[k] = i();
        }
        if (consuming != v.consuming()) {
            throw new IllegalStateException("drinking or eating is told differently");
        }
        v(v.oPos);
        v(v.oPrevPos);
        v(v.oTarget);
        v.oIncrements = i();
        v(v.oMin);
        v(v.oMax);
        v.oOnGround = b();
        v.oSprinting = b();
        v.oHurtTime = i();
        v.oSwinging = b();
        v.oSwingProgress = i();
        v.oYaw = (float) n();
        v.oPitch = (float) n();
        v.oTicksSinceHurt = i();
        v.oTicksSinceJump = i();
        v.oUsing = b();
        v.oItem = i();
        v.oHealth = (float) n();
        v.oFx = b();
        v.onTarget = b();
        v.thrown.clear();
        int count = i();
        for (int k = 0; k < count; k++) {
            View.Thrown e = new View.Thrown();
            e.id = (long) n();
            e.kind = i();
            e.mine = b();
            v(e.pos);
            v(e.motion);
            e.age = i();
            e.air = i();
            v(e.serverSide);
            v.thrown.add(e);
        }
        v.tool = i();
        v.toolSlot = i();
        v.toolCount = i();
        v.toolWear = i();
    }

    /** The counts of a view that a {@link Book} keeps. */
    private static double[] counts(View v) {
        double[] c = new double[13 + 7 * v.thrown.size()];
        System.arraycopy(v.lastKb, 0, c, 0, 3);
        c[3] = v.ticksSinceKb;
        c[4] = v.ticksSinceHurt;
        c[5] = v.ticksSinceSwing;
        c[6] = v.ticksSinceAttack;
        c[7] = v.ticksSinceMoved;
        c[8] = v.potionsSeen;
        c[9] = v.potionsPending;
        c[10] = v.oTicksSinceHurt;
        c[11] = v.oTicksSinceJump;
        c[12] = v.thrown.size();
        int at = 13;
        for (View.Thrown t : v.thrown) {
            c[at] = t.id;
            c[at + 1] = t.mine ? 1 : 0;
            c[at + 2] = t.age;
            c[at + 3] = t.air;
            System.arraycopy(t.serverSide, 0, c, at + 4, 3);
            at += 7;
        }
        return c;
    }

    public static void main(String[] args) throws IOException {
        byte[] raw = Files.readAllBytes(Paths.get(args[0]));
        d = ByteBuffer.wrap(raw).order(ByteOrder.LITTLE_ENDIAN).asDoubleBuffer();
        if (n() != 5050.0) {
            throw new IOException("not a file of duels");
        }
        int format = i();
        if (format < 5 || format > 7) {
            throw new IOException("a file of another format");
        }
        int obsDim = i(), heads = i();
        if (obsDim != Agent.OBS_DIM || heads != Agent.ACT_DIMS.length) {
            throw new IOException("the file is of an agent with other sizes");
        }
        boolean heal = format >= 6;
        int healS = heal ? i() : 0, healD = heal ? i() : 0, healR = heal ? i() : 1;
        // Format 7: the potion guard and the decision rule of the agent as well.
        boolean guard = false;
        float[] rule = new float[10];
        if (format == 7) {
            guard = b();
            for (int k = 0; k < 10; k++) {
                rule[k] = (float) n();
            }
        }
        int plansMade = 0, planTicks = 0, planTicksSame = 0, plansSame = 0, walkTicks = 0, walkSame = 0;
        int firstPlan = -1, firstWalk = -1;
        int plansTried = 0, ruleActs = 0;
        double planFirstMs = 0.0, planMaxMs = 0.0, planSumMs = 0.0;
        float mouseF1 = 1.0f;
        Random rng = new Random(1);
        Agent agent = null;
        Book book = null;
        int booksSame = 0, firstBook = -1;
        double[] p3 = new double[3], said = new double[3];
        Duel duel = null;
        Blocks blocks = null;
        View view = new View();
        float[] obs = new float[obsDim];
        int[] action = new int[heads];
        double[] feet = new double[3];
        int ticks = 0, duels = 0, keysSame = 0, obsSame = 0, terrains = 0;
        long same = 0, close = 0, numbers = 0;
        double worst = 0.0;
        int worstAt = -1, worstTick = -1;
        int[] off = new int[obsDim];
        int firstKeys = -1;
        while (d.hasRemaining()) {
            if (b()) {
                duel = new Duel();
                duel.arenaHalf = n();
                duel.floorY = n();
                boolean walls = b();
                duel.realHealth = b();
                duel.hitHealth = (float) n();
                duel.pearlCooldown = i();
                duel.drinkWindowS = n();
                double pingOwn = n(), pingOther = n(), cps = n();
                float mouse = (float) n();
                mouseF1 = mouse;
                v(feet);
                Arena arena = new Arena(duel.floorY, duel.arenaHalf, walls);
                if (b()) {
                    int[] box = new int[6];
                    for (int q = 0; q < 6; q++) {
                        box[q] = i();
                    }
                    byte[] cells = new byte[box[3] * box[4] * box[5]];
                    for (int q = 0; q < cells.length; q++) {
                        cells[q] = (byte) i();
                    }
                    boolean[] voids = new boolean[box[3] * box[5]];
                    for (int q = 0; q < voids.length; q++) {
                        voids[q] = b();
                    }
                    arena.terrain(box, cells, voids);
                    terrains++;
                }
                blocks = arena;
                if (heal && duels == 0 && Integer.getInteger("warm", 0) > 0) {
                    // As the mod does when the agent starts (-Dwarm=N plans).
                    long w0 = System.nanoTime();
                    FullHeal.warmUp(arena, feet, Integer.getInteger("warm", 0));
                    System.out.println(String.format(java.util.Locale.ROOT, "warm-up: %.0f ms", (System.nanoTime() - w0) / 1e6));
                }
                agent = new Agent(pingOwn, pingOther, cps, mouse, feet);
                agent.potionGuard = guard;
                System.arraycopy(rule, 0, agent.potionRule, 0, 10);
                agent.fullHeal = heal;
                agent.fullHealS = healS;
                agent.fullHealD = healD;
                agent.fullHealRestore = healR;
                book = new Book(feet);
                duels++;
            }
            for (int left = i(); left > 0; left--) {
                int code = i();
                if (code == 1) {
                    int x = i(), y = i(), z = i();
                    book.velocity(x, y, z);
                } else if (code == 2) {
                    book.hurt(b());
                } else if (code == 3) {
                    book.moved();
                } else if (code == 4) {
                    v(p3);
                    int x = i(), y = i(), z = i();
                    book.otherTeleported(p3, x, y, z);
                } else if (code == 9) {
                    book.destroyed((long) n());
                } else {
                    long id = (long) n();
                    int kind = code == 5 ? i() : 0;
                    boolean own = code == 5 && kind == Inv.KIND_HOOK && b();
                    int x = i(), y = i(), z = i();
                    if (code == 5) {
                        book.spawned(id, kind, own, x, y, z);
                    } else if (code == 6) {
                        book.thrownVelocity(id, x, y, z);
                    } else {
                        v(p3);
                        if (code == 7) {
                            book.thrownMoved(id, x, y, z, p3);
                        } else {
                            book.thrownTeleported(id, x, y, z, p3);
                        }
                    }
                }
            }
            int from = d.position();
            view(view);
            double[] row = new double[d.position() - from];
            d.position(from);
            d.get(row);
            if (!Arrays.equals(row, view.list())) {
                throw new IllegalStateException("tick " + ticks + ": a view is not written as it was read");
            }
            if (heal) {
                walk(view.walk);
                view.walkKnown = true;
            }
            double[] told = counts(view);
            book.fill(view);
            if (Arrays.equals(told, counts(view))) {
                booksSame++;
            } else if (firstBook < 0) {
                firstBook = ticks;
                System.out.println("tick " + ticks + ": counts " + Arrays.toString(counts(view)) + ", the simulator's " + Arrays.toString(told));
            }
            double[] eye = view.eye();
            agent.beforeTick(view, blocks);
            agent.observe(view, blocks, duel, obs);
            boolean all = true;
            for (int k = 0; k < obsDim; k++) {
                float want = (float) n();
                double diff = Math.abs((double) want - (double) obs[k]);
                numbers++;
                if (Float.floatToIntBits(want) == Float.floatToIntBits(obs[k]) || want == obs[k]) {
                    same++;
                    close++;
                } else {
                    all = false;
                    if (diff <= 1e-5) {
                        close++;
                    } else {
                        off[k]++;
                    }
                    if (diff > worst) {
                        worst = diff;
                        worstAt = k;
                        worstTick = ticks;
                    }
                }
            }
            if (all) {
                obsSame++;
            }
            for (int k = 0; k < heads; k++) {
                action[k] = i();
            }
            int tried = agent.healPlansTried, acts = agent.potionRuleActs;
            long t0 = System.nanoTime();
            int[] got = agent.keys(view, blocks, duel, action, rng).list();
            ruleActs += agent.potionRuleActs - acts;
            if (agent.healPlansTried != tried) {
                double ms = (System.nanoTime() - t0) / 1e6;
                if (plansTried == 0) {
                    planFirstMs = ms;
                } else {
                    planMaxMs = Math.max(planMaxMs, ms);
                    planSumMs += ms;
                }
                plansTried++;
            }
            int[] want = new int[got.length];
            for (int k = 0; k < want.length; k++) {
                want[k] = i();
            }
            boolean keysOk = Arrays.equals(got, want);
            if (keysOk) {
                keysSame++;
            } else if (firstKeys < 0) {
                firstKeys = ticks;
                System.out.println("tick " + ticks + ": keys " + Arrays.toString(got) + ", the simulator's " + Arrays.toString(want) + ", for " + Arrays.toString(action));
            }
            if (heal) {
                int pt = agent.healPlanTick();
                int at = d.position();
                int want0 = (int) d.get(at);
                String pd = planDiff(agent);
                if (want0 == 1) {
                    plansMade++;
                    if (pd.isEmpty()) {
                        plansSame++;
                    }
                }
                if (want0 > 0 || pt > 0) {
                    planTicks++;
                    if (pd.isEmpty() && keysOk) {
                        planTicksSame++;
                    }
                }
                if (!pd.isEmpty() && firstPlan < 0) {
                    firstPlan = ticks;
                    System.out.println("tick " + ticks + ":" + pd);
                }
            }
            Keys pressed = new Keys();
            pressed.forward = want[0];
            pressed.strafe = want[1];
            pressed.jump = want[2] != 0;
            pressed.sprintKey = want[3] != 0;
            pressed.attack = want[4] != 0;
            pressed.extraClicks = want[5];
            pressed.dx = want[6];
            pressed.dy = want[7];
            pressed.useItem = want[8] != 0;
            pressed.useAgain = want[9] != 0;
            pressed.slotKey = want[10];
            pressed.inv = want[11];
            v(feet);
            boolean consuming = b(), oppUsing = b();
            int oppItem = i();
            boolean threw = b(), threwPotion = b(), threwBall = b(), usedRod = b(), swung = b(), attacked = b();
            v(said);
            float saidYaw = (float) n(), saidPitch = (float) n();
            if (heal) {
                Walk after = walkAfter();
                if (!view.usingItem && !after.usingItem && pressed.inv == 0 && !view.invOpen && !attacked) {
                    Walk w = Walk.of(view);
                    w.tick(blocks, pressed.forward, pressed.strafe, pressed.jump, pressed.sprintKey, pressed.dx, pressed.dy, mouseF1);
                    w.usingItem = false;
                    walkTicks++;
                    String wd = walkDiff(w, after);
                    if (wd.isEmpty()) {
                        walkSame++;
                    } else if (firstWalk < 0) {
                        firstWalk = ticks;
                        System.out.println("tick " + ticks + ": the client's tick made differently:" + wd);
                    }
                }
            }
            book.ticked(swung, attacked, threw, threwPotion, threwBall, eye, said, saidYaw, saidPitch);
            agent.ticked(pressed, duel, feet, consuming, oppUsing && (oppItem == Inv.DRINK || oppItem == Inv.FOOD), threw, threwPotion);
            ticks++;
        }
        System.out.println(ticks + " ticks of " + duels + " duels (" + terrains + " on terrain): the same counts on " + booksSame + ", the same keys on " + keysSame + ", all " + obsDim + " numbers shown the same on " + obsSame);
        System.out.println(same + " of " + numbers + " numbers the same to the last bit, " + (numbers - close) + " further off than 0.00001; the largest difference " + worst + " (number " + worstAt + ", tick " + worstTick + ")");
        StringBuilder where = new StringBuilder();
        for (int k = 0; k < obsDim; k++) {
            if (off[k] > 0) {
                where.append(' ').append(k).append(':').append(off[k]);
            }
        }
        if (where.length() > 0) {
            System.out.println("numbers further off, and on how many ticks:" + where);
        }
        if (heal) {
            System.out.println("fullheal (gate S " + healS + ", D " + healD + ", restore " + healR + "): " + plansMade + " plans made, the same " + plansSame
                + "; " + planTicks + " ticks with a plan under way, the same plan and keys on " + planTicksSame + "; the client's tick worked out on "
                + walkTicks + ", the same to the last bit on " + walkSame);
            if (format == 7) {
                System.out.println("the decision rule (guard " + guard + ", " + Arrays.toString(rule) + ") changed the choice on " + ruleActs + " ticks");
            }
            if (plansTried > 0) {
                System.out.println(String.format(java.util.Locale.ROOT, "plans worked out %d times: the first (cold) took %.1f ms, the rest %.2f ms on average and at most %.1f ms",
                    plansTried, planFirstMs, plansTried > 1 ? planSumMs / (plansTried - 1) : 0.0, planMaxMs));
            }
        }
        if (keysSame != ticks || numbers != close || booksSame != ticks || plansSame != plansMade || planTicksSame != planTicks || walkSame != walkTicks) {
            System.exit(1);
        }
    }

    private AgentCheck() {}
}
