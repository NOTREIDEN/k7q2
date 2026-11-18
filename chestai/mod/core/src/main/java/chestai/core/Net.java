package chestai.core;

import java.io.DataInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.HashMap;
import java.util.Map;
import java.util.Random;

/**
 * The policy network of the chest farmer as python/chestai/model.py builds
 * it, read from the file python/export_policy.py writes: an encoder of the
 * player's own state, a small convolution over the terrain patch, one
 * encoder with attention and a pool for each list (mobs, chests,
 * projectiles), a trunk, a memory (LSTM) and one head per control. One
 * call of {@link #logits} is one game tick.
 */
public final class Net {
    public final int obsDim, hidden;
    public final int[] dims;
    public final long samples;
    private final Map<String, float[]> t = new HashMap<String, float[]>();
    private final Map<String, int[]> shape = new HashMap<String, int[]>();
    private final float[] h, c;

    private Net(int obsDim, int hidden, int[] dims, long samples) {
        this.obsDim = obsDim;
        this.hidden = hidden;
        this.dims = dims;
        this.samples = samples;
        h = new float[hidden];
        c = new float[hidden];
    }

    public static Net load(InputStream in) throws IOException {
        DataInputStream d = new DataInputStream(in);
        byte[] head = new byte[20];
        d.readFully(head);
        ByteBuffer b = ByteBuffer.wrap(head).order(ByteOrder.LITTLE_ENDIAN);
        if (b.get() != 'C' || b.get() != 'H' || b.get() != 'A' || b.get() != 'I' || b.getInt() != 1) {
            throw new IOException("not a chest policy of format 1");
        }
        int obsDim = b.getInt(), hidden = b.getInt(), heads = b.getInt();
        if (obsDim != Obs.DIM) {
            throw new IOException("policy reads " + obsDim + " values, the mod builds " + Obs.DIM);
        }
        if (hidden <= 0 || hidden > 4096 || heads <= 0 || heads > 64) {
            throw new IOException("policy file with sizes out of range");
        }
        int[] dims = new int[heads];
        for (int i = 0; i < heads; i++) {
            dims[i] = readInt(d);
        }
        long samples = Long.reverseBytes(d.readLong());
        Net n = new Net(obsDim, hidden, dims, samples);
        int count = readInt(d);
        for (int i = 0; i < count; i++) {
            byte[] name = new byte[readInt(d)];
            d.readFully(name);
            int rank = readInt(d);
            int[] sz = new int[rank];
            int total = 1;
            for (int k = 0; k < rank; k++) {
                sz[k] = readInt(d);
                total *= sz[k];
            }
            byte[] raw = new byte[4 * total];
            d.readFully(raw);
            float[] v = new float[total];
            ByteBuffer.wrap(raw).order(ByteOrder.LITTLE_ENDIAN).asFloatBuffer().get(v);
            String key = new String(name, "UTF-8");
            n.t.put(key, v);
            n.shape.put(key, sz);
        }
        return n;
    }

    private static int readInt(DataInputStream d) throws IOException {
        return Integer.reverseBytes(d.readInt());
    }

    public void reset() {
        java.util.Arrays.fill(h, 0f);
        java.util.Arrays.fill(c, 0f);
    }

    private float[] w(String k) {
        float[] v = t.get(k);
        if (v == null) {
            throw new IllegalStateException("policy without " + k);
        }
        return v;
    }

    /** out = W in + b for a torch Linear called name, from offset off of in. */
    private float[] linear(String name, float[] in, int off, int nIn, boolean relu) {
        float[] wt = w(name + ".weight"), bs = w(name + ".bias");
        int nOut = bs.length;
        if (wt.length != nOut * nIn) {
            throw new IllegalStateException(name + " takes " + wt.length / nOut + " values, not " + nIn);
        }
        float[] out = new float[nOut];
        for (int i = 0, k = 0; i < nOut; i++) {
            float s = bs[i];
            for (int j = 0; j < nIn; j++, k++) {
                s += wt[k] * in[off + j];
            }
            out[i] = relu && s < 0f ? 0f : s;
        }
        return out;
    }

    /** A 3x3 convolution with padding 1, of a [cin][n][n] input. */
    private float[] conv(String name, float[] in, int cin, int n, int stride) {
        float[] wt = w(name + ".weight"), bs = w(name + ".bias");
        int cout = bs.length;
        int m = (n + 2 - 3) / stride + 1;
        float[] out = new float[cout * m * m];
        for (int o = 0; o < cout; o++) {
            for (int y = 0; y < m; y++) {
                for (int x = 0; x < m; x++) {
                    float s = bs[o];
                    for (int ci = 0; ci < cin; ci++) {
                        for (int ky = 0; ky < 3; ky++) {
                            int iy = y * stride + ky - 1;
                            if (iy < 0 || iy >= n) {
                                continue;
                            }
                            for (int kx = 0; kx < 3; kx++) {
                                int ix = x * stride + kx - 1;
                                if (ix < 0 || ix >= n) {
                                    continue;
                                }
                                s += wt[((o * cin + ci) * 3 + ky) * 3 + kx] * in[(ci * n + iy) * n + ix];
                            }
                        }
                    }
                    out[(o * m + y) * m + x] = s < 0f ? 0f : s;
                }
            }
        }
        return out;
    }

    /** Encoder, attention from the query and masked max pool of a list. */
    private float[] entities(String name, float[] obs, int off, int k, int dIn, float[] query) {
        int d = w(name + ".enc.0.bias").length;
        float[] q = linear(name + ".q", query, 0, query.length, false);
        float[][] e = new float[k][];
        float[][] v = new float[k][];
        float[] att = new float[k];
        boolean any = false;
        float top = -Float.MAX_VALUE;
        for (int i = 0; i < k; i++) {
            if (!(obs[off + i * dIn] > 0.5f)) {
                continue;
            }
            any = true;
            float[] e1 = linear(name + ".enc.0", obs, off + i * dIn, dIn, true);
            e[i] = linear(name + ".enc.2", e1, 0, d, true);
            float[] kk = linear(name + ".k", e[i], 0, d, false);
            v[i] = linear(name + ".v", e[i], 0, d, false);
            float s = 0f;
            for (int j = 0; j < d; j++) {
                s += q[j] * kk[j];
            }
            att[i] = s / (float) Math.sqrt(d);
            top = Math.max(top, att[i]);
        }
        float[] out = new float[2 * d];
        if (!any) {
            return out;
        }
        double sum = 0.0;
        double[] wgt = new double[k];
        for (int i = 0; i < k; i++) {
            if (e[i] != null) {
                wgt[i] = Math.exp(att[i] - top);
                sum += wgt[i];
            }
        }
        for (int j = 0; j < d; j++) {
            out[d + j] = -1e4f;
        }
        for (int i = 0; i < k; i++) {
            if (e[i] == null) {
                continue;
            }
            float a = (float) (wgt[i] / sum);
            for (int j = 0; j < d; j++) {
                out[j] += a * v[i][j];
                out[d + j] = Math.max(out[d + j], e[i][j]);
            }
        }
        return out;
    }

    private static float sigmoid(float v) {
        return (float) (1.0 / (1.0 + Math.exp(-v)));
    }

    private static float[] cat(float[]... parts) {
        int n = 0;
        for (float[] p : parts) {
            n += p.length;
        }
        float[] r = new float[n];
        int i = 0;
        for (float[] p : parts) {
            System.arraycopy(p, 0, r, i, p.length);
            i += p.length;
        }
        return r;
    }

    /** One tick: the preferences for the choices of each control. */
    public float[][] logits(float[] obs) {
        if (obs.length != obsDim) {
            throw new IllegalArgumentException("observation of " + obs.length + " values, the policy reads " + obsDim);
        }
        int offTerrain = Obs.SELF_DIM;
        int offMobs = offTerrain + Obs.TERRAIN_DIM;
        int offChests = offMobs + Obs.K_MOBS * Obs.MOB_DIM;
        int offProjs = offChests + Obs.K_CHESTS * Obs.CHEST_DIM;
        int offOwn = offProjs + Obs.K_PROJ * Obs.PROJ_DIM;
        float[] selfIn = new float[Obs.SELF_DIM + Obs.OWN_DIM];
        System.arraycopy(obs, 0, selfIn, 0, Obs.SELF_DIM);
        System.arraycopy(obs, offOwn, selfIn, Obs.SELF_DIM, Obs.OWN_DIM);
        float[] s = linear("self_enc.0", selfIn, 0, selfIn.length, true);
        // The terrain as [channel][right][forward].
        int g = 2 * Obs.GRID + 1;
        float[] tin = new float[2 * g * g];
        for (int i = 0; i < g * g; i++) {
            tin[i] = obs[offTerrain + 2 * i];
            tin[g * g + i] = obs[offTerrain + 2 * i + 1];
        }
        float[] t1 = conv("terrain.0", tin, 2, g, 1);
        float[] t2 = conv("terrain.2", t1, w("terrain.0.bias").length, g, 2);
        float[] tr = linear("terrain.5", t2, 0, t2.length, true);
        float[] m = entities("mobs", obs, offMobs, Obs.K_MOBS, Obs.MOB_DIM, s);
        float[] ch = entities("chests", obs, offChests, Obs.K_CHESTS, Obs.CHEST_DIM, s);
        float[] pr = entities("projs", obs, offProjs, Obs.K_PROJ, Obs.PROJ_DIM, s);
        float[] tin2 = cat(s, tr, m, ch, pr);
        float[] x = linear("trunk.0", tin2, 0, tin2.length, true);
        int n = hidden;
        float[] wi = w("lstm.weight_ih_l0"), wh = w("lstm.weight_hh_l0"), bi = w("lstm.bias_ih_l0"), bh = w("lstm.bias_hh_l0");
        float[] gates = new float[4 * n];
        for (int i = 0, k = 0; i < 4 * n; i++) {
            float sum = bi[i] + bh[i];
            for (int j = 0; j < n; j++, k++) {
                sum += wi[k] * x[j] + wh[k] * h[j];
            }
            gates[i] = sum;
        }
        for (int i = 0; i < n; i++) {
            float in = sigmoid(gates[i]);
            float forget = sigmoid(gates[n + i]);
            float cell = (float) Math.tanh(gates[2 * n + i]);
            float out = sigmoid(gates[3 * n + i]);
            c[i] = forget * c[i] + in * cell;
            h[i] = out * (float) Math.tanh(c[i]);
        }
        float[] post = cat(h, x);
        float[] z = linear("post.0", post, 0, post.length, true);
        float[][] r = new float[dims.length][];
        for (int i = 0; i < dims.length; i++) {
            r[i] = linear("heads." + i, z, 0, n, false);
        }
        return r;
    }

    /** One tick, a choice drawn for each control as in training (or the likeliest one). */
    public int[] act(float[] obs, Random rng, boolean likeliest) {
        float[][] lg = logits(obs);
        int[] a = new int[lg.length];
        for (int i = 0; i < lg.length; i++) {
            a[i] = likeliest ? argmax(lg[i]) : draw(lg[i], rng);
        }
        return a;
    }

    static int argmax(float[] v) {
        int b = 0;
        for (int i = 1; i < v.length; i++) {
            if (v[i] > v[b]) {
                b = i;
            }
        }
        return b;
    }

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
