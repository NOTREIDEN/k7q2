package chestai.core;

import java.io.BufferedInputStream;
import java.io.DataInputStream;
import java.io.FileInputStream;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;

/**
 * Runs the exported policy over the observations of a check file of
 * python/export_policy.py and compares its logits with PyTorch's.
 *
 *     java chestai.core.NetCheck policy.bin check.bin
 */
public final class NetCheck {
    public static void main(String[] a) throws IOException {
        Net net = Net.load(new BufferedInputStream(new FileInputStream(a[0])));
        DataInputStream d = new DataInputStream(new BufferedInputStream(new FileInputStream(a[1])));
        int ticks = Integer.reverseBytes(d.readInt()), dim = Integer.reverseBytes(d.readInt());
        double worst = 0.0;
        for (int t = 0; t < ticks; t++) {
            float[] obs = floats(d, dim);
            int n = Integer.reverseBytes(d.readInt());
            float[] want = floats(d, n);
            float[][] got = net.logits(obs);
            int i = 0;
            for (float[] head : got) {
                for (float v : head) {
                    worst = Math.max(worst, Math.abs(v - want[i++]));
                }
            }
        }
        System.out.println(ticks + " ticks, largest logit difference " + worst);
        if (worst > 1e-3) {
            System.exit(1);
        }
    }

    static float[] floats(DataInputStream d, int n) throws IOException {
        byte[] b = new byte[4 * n];
        d.readFully(b);
        float[] v = new float[n];
        ByteBuffer.wrap(b).order(ByteOrder.LITTLE_ENDIAN).asFloatBuffer().get(v);
        return v;
    }
}
