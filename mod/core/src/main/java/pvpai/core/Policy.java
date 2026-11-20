package pvpai.core;

import java.io.DataInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.Random;

/**
 * The network that chooses the agent's actions, as trained: two layers that
 * read the observation, a memory (LSTM) and one layer per control. Read
 * from the file python/export_policy.py writes. One call of
 * {@link #logits} or {@link #act} is one game tick; {@link #reset} empties
 * the memory, as at the start of a duel.
 */
public final class Policy {
    public final int obsDim;
    public final int hidden;
    /** Choices of each control. */
    public final int[] dims;
    /** Samples the network was trained on. */
    public final long samples;

    private final float[] w0, b0, w1, b1;
    private final float[] wIn, wMem, bIn, bMem;
    private final float[][] headW, headB;
    private final float[] h, c;
    private final float[] x0, x1, gates;

    private Policy(int obsDim, int hidden, int[] dims, long samples, ByteBuffer f) {
        this.obsDim = obsDim;
        this.hidden = hidden;
        this.dims = dims;
        this.samples = samples;
        w0 = take(f, hidden * obsDim);
        b0 = take(f, hidden);
        w1 = take(f, hidden * hidden);
        b1 = take(f, hidden);
        wIn = take(f, 4 * hidden * hidden);
        wMem = take(f, 4 * hidden * hidden);
        bIn = take(f, 4 * hidden);
        bMem = take(f, 4 * hidden);
        headW = new float[dims.length][];
        headB = new float[dims.length][];
        for (int i = 0; i < dims.length; i++) {
            headW[i] = take(f, dims[i] * hidden);
            headB[i] = take(f, dims[i]);
        }
        h = new float[hidden];
        c = new float[hidden];
        x0 = new float[hidden];
        x1 = new float[hidden];
        gates = new float[4 * hidden];
    }

    private static float[] take(ByteBuffer f, int n) {
        float[] a = new float[n];
        f.asFloatBuffer().get(a);
        f.position(f.position() + 4 * n);
        return a;
    }

    public static Policy load(InputStream in) throws IOException {
        DataInputStream d = new DataInputStream(in);
        byte[] head = new byte[20];
        d.readFully(head);
        ByteBuffer b = ByteBuffer.wrap(head).order(ByteOrder.LITTLE_ENDIAN);
        if (b.get() != 'P' || b.get() != 'V' || b.get() != 'P' || b.get() != 'P' || b.getInt() != 1) {
            throw new IOException("not a policy file of format 1");
        }
        int obsDim = b.getInt(), hidden = b.getInt(), heads = b.getInt();
        if (obsDim <= 0 || obsDim > 4096 || hidden <= 0 || hidden > 4096 || heads <= 0 || heads > 64) {
            throw new IOException("policy file with sizes out of range");
        }
        byte[] rest = new byte[4 * heads + 8];
        d.readFully(rest);
        b = ByteBuffer.wrap(rest).order(ByteOrder.LITTLE_ENDIAN);
        int[] dims = new int[heads];
        long floats = (long) hidden * obsDim + hidden + (long) hidden * hidden + hidden + 8L * hidden * hidden + 8L * hidden;
        for (int i = 0; i < heads; i++) {
            dims[i] = b.getInt();
            if (dims[i] <= 0 || dims[i] > 1024) {
                throw new IOException("policy file with sizes out of range");
            }
            floats += (long) dims[i] * hidden + dims[i];
        }
        long samples = b.getLong();
        byte[] body = new byte[(int) (4 * floats)];
        d.readFully(body);
        return new Policy(obsDim, hidden, dims, samples, ByteBuffer.wrap(body).order(ByteOrder.LITTLE_ENDIAN));
    }

    /** Empty the memory. */
    public void reset() {
        java.util.Arrays.fill(h, 0f);
        java.util.Arrays.fill(c, 0f);
    }

    private static void layer(float[] w, float[] b, float[] in, int nIn, float[] out, int nOut, boolean relu) {
        for (int i = 0, k = 0; i < nOut; i++) {
            float s = b[i];
            for (int j = 0; j < nIn; j++, k++) {
                s += w[k] * in[j];
            }
            out[i] = relu && s < 0f ? 0f : s;
        }
    }

    private static float sigmoid(float v) {
        return (float) (1.0 / (1.0 + Math.exp(-v)));
    }

    /**
     * One tick: take the observation in and return the preferences for the
     * choices of each control.
     */
    public float[][] logits(float[] obs) {
        // A policy trained before the agent showed more reads the first
        // values only, as if the later ones had no weight.
        if (obs.length < obsDim) {
            throw new IllegalArgumentException("observation of " + obs.length + " values, the policy reads " + obsDim);
        }
        int n = hidden;
        layer(w0, b0, obs, obsDim, x0, n, true);
        layer(w1, b1, x0, n, x1, n, true);
        for (int i = 0, k = 0; i < 4 * n; i++) {
            float s = bIn[i] + bMem[i];
            for (int j = 0; j < n; j++, k++) {
                s += wIn[k] * x1[j] + wMem[k] * h[j];
            }
            gates[i] = s;
        }
        for (int i = 0; i < n; i++) {
            float in = sigmoid(gates[i]);
            float forget = sigmoid(gates[n + i]);
            float cell = (float) Math.tanh(gates[2 * n + i]);
            float out = sigmoid(gates[3 * n + i]);
            c[i] = forget * c[i] + in * cell;
            h[i] = out * (float) Math.tanh(c[i]);
        }
        float[][] r = new float[dims.length][];
        for (int i = 0; i < dims.length; i++) {
            r[i] = new float[dims[i]];
            layer(headW[i], headB[i], h, n, r[i], dims[i], false);
        }
        return r;
    }

    /**
     * How firmly the choices follow the preferences: each control's are
     * divided by this before the draw. 1 as in training; below one the
     * likeliest choices come more often; 0 always the likeliest.
     */
    public float temperature = 1.0f;

    /**
     * One tick: take the observation in and draw a choice for each control
     * by its preferences, as in training (at {@link #temperature}).
     */
    public int[] act(float[] obs, Random rng) {
        float[][] lg = logits(obs);
        int[] a = new int[lg.length];
        for (int i = 0; i < lg.length; i++) {
            a[i] = temperature == 1.0f ? draw(lg[i], rng) : draw(lg[i], temperature, rng);
        }
        return a;
    }

    /** A choice drawn with the preferences divided by t; at t 0 or below the likeliest (the first of equals). */
    static int draw(float[] lg, float t, Random rng) {
        if (t <= 0.0f) {
            rng.nextDouble();
            int best = 0;
            for (int i = 1; i < lg.length; i++) {
                if (lg[i] > lg[best]) {
                    best = i;
                }
            }
            return best;
        }
        float[] s = new float[lg.length];
        for (int i = 0; i < lg.length; i++) {
            s[i] = lg[i] / t;
        }
        return draw(s, rng);
    }

    /** A choice drawn with chances in proportion to exp(preference). */
    static int draw(float[] lg, Random rng) {
        float top = lg[0];
        for (float v : lg) {
            top = Math.max(top, v);
        }
        double sum = 0.0;
        for (float v : lg) {
            sum += Math.exp(v - top);
        }
        double u = rng.nextDouble() * sum;
        for (int i = 0; i < lg.length; i++) {
            u -= Math.exp(lg[i] - top);
            if (u < 0.0) {
                return i;
            }
        }
        return lg.length - 1;
    }
}
