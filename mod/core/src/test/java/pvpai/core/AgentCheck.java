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
        if (n() != 5050.0 || n() != 5.0) {
            throw new IOException("not a file of duels, or of another format");
        }
        int obsDim = i(), heads = i();
        if (obsDim != Agent.OBS_DIM || heads != Agent.ACT_DIMS.length) {
            throw new IOException("the file is of an agent with other sizes");
        }
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
                agent = new Agent(pingOwn, pingOther, cps, mouse, feet);
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
            int[] got = agent.keys(view, blocks, duel, action, rng).list();
            int[] want = new int[got.length];
            for (int k = 0; k < want.length; k++) {
                want[k] = i();
            }
            if (Arrays.equals(got, want)) {
                keysSame++;
            } else if (firstKeys < 0) {
                firstKeys = ticks;
                System.out.println("tick " + ticks + ": keys " + Arrays.toString(got) + ", the simulator's " + Arrays.toString(want) + ", for " + Arrays.toString(action));
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
        if (keysSame != ticks || numbers != close || booksSame != ticks) {
            System.exit(1);
        }
    }

    private AgentCheck() {}
}
