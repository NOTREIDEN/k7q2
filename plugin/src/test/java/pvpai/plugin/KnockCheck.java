package pvpai.plugin;

import java.io.BufferedReader;
import java.io.FileReader;
import java.io.IOException;

/**
 * Compares {@link Knock} with the simulator's server on the hits that
 * `cargo run --release --example kb_dump -- hits.txt` writes:
 *
 *     java -cp classes pvpai.plugin.KnockCheck hits.txt
 */
public final class KnockCheck {
    private static double d(String s) {
        return Double.longBitsToDouble(Long.parseUnsignedLong(s, 16));
    }

    private static float f(String s) {
        return Float.intBitsToFloat(Integer.parseUnsignedInt(s, 16));
    }

    public static void main(String[] args) throws IOException {
        int[] hits = new int[2], same = new int[2], near = new int[2];
        int tops = 0;
        BufferedReader in = new BufferedReader(new FileReader(args[0]));
        for (String line = in.readLine(); line != null; line = in.readLine()) {
            String[] part = line.split(" \\| ");
            String[] k = part[0].split(" "), a = part[1].split(" "), v = part[2].split(" "), want = part[3].split(" ");
            Profile p = new Profile();
            double ax = d(a[0]), az = d(a[2]);
            float yaw = f(a[3]);
            boolean serverSprint = a[4].equals("1"), clientSprint = a[5].equals("1");
            int ench = Integer.parseInt(a[6]);
            double vx = d(v[0]), vy = d(v[1]), vz = d(v[2]);
            double[] motion = {d(v[3]), d(v[4]), d(v[5])};
            boolean ground = v[6].equals("1");
            boolean[] touched = {v[8].equals("1")};
            double[] got;
            int kind;
            if (k[0].equals("A")) {
                kind = 0;
                p.kind = Profile.GAME;
                p.friction = d(k[1]);
                p.horizontal = d(k[2]);
                p.vertical = d(k[3]);
                p.verticalLimit = d(k[4]);
                p.extraHorizontal = f(k[5]);
                p.extraVertical = d(k[6]);
                got = Knock.game(p, motion, ax, az, vx, vz, yaw, ench + (serverSprint ? 1 : 0));
            } else {
                kind = 1;
                p.kind = Profile.HIT;
                p.hGround = d(k[1]);
                p.hAir = d(k[2]);
                p.hSprint = d(k[3]);
                p.vGround = d(k[4]);
                p.vAir = d(k[5]);
                p.vSprint = d(k[6]);
                p.stopSprint = k[7].equals("1");
                p.yLimit = d(k[8]);
                p.vAfterLimit = d(k[9]);
                boolean sprint = p.stopSprint ? serverSprint : clientSprint;
                got = Knock.hit(p, ax, az, vx, vy, vz, ground, yaw, ench + (sprint ? 1 : 0), d(v[7]), touched);
                boolean wantTop = want[3].equals("1");
                tops += wantTop ? 1 : 0;
                if (touched[0] != wantTop) {
                    System.out.println("the height limit differs: " + line);
                    System.exit(1);
                }
            }
            int[] sent = Knock.sent(got);
            int off = 0;
            for (int i = 0; i < 3; i++) {
                off = Math.max(off, Math.abs(sent[i] - Integer.parseInt(want[i])));
            }
            hits[kind]++;
            same[kind] += off == 0 ? 1 : 0;
            near[kind] += off == 1 ? 1 : 0;
            if (off > 1) {
                System.out.println("differs by " + off + ": " + line);
                System.exit(1);
            }
        }
        in.close();
        System.out.printf("the game's way: %d hits, %d the same, %d one 8000th off; from the hit alone: %d hits, %d the same, %d one 8000th off, %d over the height limit%n",
            hits[0], same[0], near[0], hits[1], same[1], near[1], tops);
        // The game's way is the same arithmetic to the last bit. The other
        // way has a sine and a cosine of the language's own library in it.
        if (hits[0] < 1000 || hits[1] < 1000 || same[0] != hits[0] || near[1] * 1000 > hits[1]) {
            System.exit(1);
        }
    }
}
