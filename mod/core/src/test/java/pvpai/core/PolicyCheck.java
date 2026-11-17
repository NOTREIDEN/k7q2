package pvpai.core;

import java.io.BufferedInputStream;
import java.io.DataInputStream;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.Random;

/**
 * Compares {@link Policy} with the network it was exported from:
 * <pre>java pvpai.core.PolicyCheck policy.bin check.bin</pre>
 * check.bin holds the observations of a few duels and the preferences
 * the network gave on each tick (python/export_policy.py --check).
 */
public final class PolicyCheck {
    public static void main(String[] args) throws IOException {
        Policy p;
        try (InputStream in = new BufferedInputStream(new FileInputStream(args[0]))) {
            p = Policy.load(in);
        }
        int ticks, obsDim, prefs;
        float[] data;
        try (DataInputStream in = new DataInputStream(new BufferedInputStream(new FileInputStream(args[1])))) {
            byte[] head = new byte[20];
            in.readFully(head);
            ByteBuffer b = ByteBuffer.wrap(head).order(ByteOrder.LITTLE_ENDIAN);
            if (b.get() != 'P' || b.get() != 'V' || b.get() != 'P' || b.get() != 'C' || b.getInt() != 1) {
                throw new IOException("not a check file of format 1");
            }
            ticks = b.getInt();
            obsDim = b.getInt();
            prefs = b.getInt();
            byte[] body = new byte[4 * ticks * (1 + obsDim + prefs)];
            in.readFully(body);
            data = new float[ticks * (1 + obsDim + prefs)];
            ByteBuffer.wrap(body).order(ByteOrder.LITTLE_ENDIAN).asFloatBuffer().get(data);
        }
        int total = 0;
        for (int d : p.dims) {
            total += d;
        }
        if (obsDim != p.obsDim || prefs != total) {
            throw new IOException("the check file is of another policy");
        }
        double worst = 0.0;
        int same = 0, choices = 0;
        float[] obs = new float[obsDim];
        int duels = 0;
        for (int t = 0, at = 0; t < ticks; t++) {
            if (data[at++] != 0f) {
                p.reset();
                duels++;
            }
            System.arraycopy(data, at, obs, 0, obsDim);
            at += obsDim;
            float[][] lg = p.logits(obs);
            for (float[] head : lg) {
                int best = 0, want = 0;
                for (int i = 0; i < head.length; i++) {
                    worst = Math.max(worst, Math.abs(head[i] - data[at + i]));
                    if (head[i] > head[best]) {
                        best = i;
                    }
                    if (data[at + i] > data[at + want]) {
                        want = i;
                    }
                }
                same += best == want ? 1 : 0;
                choices++;
                at += head.length;
            }
        }
        // The draw: a control's choices come up as often as their
        // preferences say.
        Random rng = new Random(1);
        float[] lg = {0f, (float) Math.log(3.0), (float) Math.log(0.5)};
        int[] seen = new int[3];
        for (int i = 0; i < 90000; i++) {
            seen[Policy.draw(lg, rng)]++;
        }
        boolean drawn = Math.abs(seen[0] - 20000) < 600 && Math.abs(seen[1] - 60000) < 600 && Math.abs(seen[2] - 10000) < 600;
        System.out.printf("%dM samples, %d ticks of %d duels: largest difference %.2e, same first choice %d of %d, draws %d %d %d%n",
                p.samples / 1000000, ticks, duels, worst, same, choices, seen[0], seen[1], seen[2]);
        if (worst > 1e-3 || duels < 2 || !drawn) {
            System.exit(1);
        }
    }
}
