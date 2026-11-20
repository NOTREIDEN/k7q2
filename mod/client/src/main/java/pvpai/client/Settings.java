package pvpai.client;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.util.Properties;

/**
 * What the player sets for the duels an agent plays, kept in a small text
 * file. What is left to "auto" is asked of the game when a duel starts.
 */
public final class Settings {
    /** Clicks a second: one is drawn for each duel between these two. */
    public double cpsMin = 14.0, cpsMax = 17.0;
    /** Pings in milliseconds; less than zero: what the player list says. */
    public double ping = -1.0, otherPing = -1.0;
    /** Seconds the server makes a player wait between two pearls. */
    public double pearlCooldownS = 16.0;
    /** The arena, if it is not to be measured: its centre and half its side. */
    public boolean arenaSet;
    public double arenaX, arenaZ, arenaHalf;
    /** Whether the agent may block with its sword. */
    public boolean block = true;
    /**
     * Whether a potion thrown with a turn is planned to give all eight
     * points (pvpai.core.FullHeal): the pitch of the throw and the
     * movement keys until it is due.
     */
    public boolean fullHeal = false;
    /**
     * A last guard against dying with potions at hand (pvpai.core.Agent#potSafety):
     * a potion thrown at once when the health left is within what the
     * other player can take before a potion thrown now would heal.
     */
    public boolean potSafety = false;
    /** The guard only on a server seen to hide the opponent's health (pvpai.core.Agent#potSafetyHidden): pocion_segura=oculta. */
    public boolean potSafetyHidden = false;
    /**
     * A potion thrown with a turn lets go of the block on the tick of the
     * turn (pvpai.core.Agent#turnRelease), so that its click is not lost.
     */
    public boolean turnRelease = false;
    /**
     * The opponent's health the agent goes by (vida_rival): what the server
     * sends (real); a guess of it (pvpai.core.Agent#hpGuess) always
     * (estimada) or once the server is seen to hide it (auto: minemen.club
     * sends 1 every tick); or a set value in its place
     * (pvpai.core.Agent#oppShown) on every server (oculta, for a network
     * trained with the health hidden in every duel) or once the server is
     * seen to hide it (oculta_auto). hpGuess: 0 off, 1 always, 2 auto;
     * oppShown: 0 off, 1 auto, 2 always; oppShownHealth (vida_rival_valor):
     * the value, below zero (propia) its own health. hpGuessScale
     * (vida_rival_golpe): what the guess takes a hit seen to cost, in plain
     * hits (pvpai.core.Agent#hpGuessScale). Default auto with 1.3: in long
     * duels on a minemen-like server against a network that sees the
     * health, the one that won the most of those tried in the simulator
     * (10 shown in its place made the agent hold back, 1 made it die).
     */
    public int hpGuess = 2;
    public int oppShown = 0;
    public float oppShownHealth = 10.0f;
    public float hpGuessScale = 1.3f;
    /**
     * How firmly the agent follows its preferences (temperatura,
     * pvpai.core.Policy#temperature): 1 as in training, 0 always the
     * likeliest choice. Default 0.7: in long minemen-like duels against
     * the 23:40 network and the shipped one, it won 5 points more than 1
     * in the simulator.
     */
    public float temperature = 0.7f;

    /** The line on the temperature, before those on the opponent's health. */
    static final String TEMPERATURE = ""
        + "# Que tan fijo elige el bot: 1 como en el entrenamiento; mas bajo, elige mas seguido lo que mas le\n"
        + "# conviene (0: siempre eso).\n"
        + "temperatura=0.7\n";

    /** The lines on the opponent's health, last in the file. */
    static final String RIVAL = ""
        + "# La vida del rival que mira el bot. auto: si el servidor la esconde (minemen manda siempre 1), la\n"
        + "# estima con los golpes que le da y las pociones que le curan; si la manda, la de verdad. real: siempre\n"
        + "# la que manda el servidor. estimada: la estima siempre. oculta_auto: si el servidor la esconde, mira\n"
        + "# vida_rival_valor en su lugar. oculta: siempre vida_rival_valor, en todo servidor (para una red\n"
        + "# entrenada sin ver la vida del rival). vida_rival_valor: un numero, o propia (la vida del bot).\n"
        + "# vida_rival_golpe: cuanto cree la estimacion que le saca cada golpe, en golpes comunes (mas alto:\n"
        + "# lo cree mas bajo de vida y pelea mas agresivo).\n"
        + "vida_rival=auto\n"
        + "vida_rival_valor=10\n"
        + "vida_rival_golpe=1.3\n";

    /**
     * The same lines as the jars of 2026-11-19 wrote them: a file that
     * still holds them untouched gets the new ones in their place.
     */
    static final String RIVAL_BEFORE = ""
        + "# La vida del rival que mira el bot. oculta_auto: si el servidor la esconde (minemen manda siempre 1),\n"
        + "# el bot mira vida_rival_valor en su lugar; si la manda, la de verdad. real: siempre la que manda el\n"
        + "# servidor (apaga lo anterior). auto: la estima (golpes que le da y pociones que le curan) si el servidor\n"
        + "# la esconde; estimada: la estima siempre. oculta: siempre vida_rival_valor, en todo servidor (para una\n"
        + "# red entrenada sin ver la vida del rival). vida_rival_valor: un numero, o propia (la vida del bot).\n"
        + "vida_rival=oculta_auto\n"
        + "vida_rival_valor=10\n";

    /** The file a player finds the first time, in the player's language. */
    public static final String FIRST = ""
        + "# PvP AI: ajustes. Se leen cada vez que se prende el bot.\n"
        + "\n"
        + "# Clics por segundo: un numero, o un rango del que se sortea uno en cada duelo.\n"
        + "cps=14-17\n"
        + "\n"
        + "# Ping propio y del rival en milisegundos. Con auto se lee de la lista de jugadores (TAB).\n"
        + "ping=auto\n"
        + "ping_rival=auto\n"
        + "\n"
        + "# Segundos que el servidor hace esperar entre dos perlas (0 si no hay espera).\n"
        + "perla_espera=16\n"
        + "\n"
        + "# La arena. Con auto se mide con los bloques de alrededor al prender.\n"
        + "# Para darla a mano:  arena=X Z MITAD  (el centro de la arena y la mitad de su lado), por ejemplo  arena=0 0 24\n"
        + "arena=auto\n"
        + "\n"
        + "# Bloquear con la espada (clic derecho). Con no, pelea sin bloquear nunca.\n"
        + "bloqueo=si\n"
        + "\n"
        + "# Pociones tiradas con giro: elegir la inclinacion y moverse para que curen las 8 vidas.\n"
        + "pocion_completa=no\n"
        + "\n"
        + "# Ultimo recurso contra morir con pociones: tirar una enseguida cuando la vida que queda\n"
        + "# se la puede quitar el rival antes de que cure una tirada ahora (cuenta el ping).\n"
        + "# Con oculta, solo en servidores que esconden la vida del rival (minemen).\n"
        + "pocion_segura=no\n"
        + "\n"
        + "# Soltar el bloqueo en el giro de una pocion tirada con giro, para que el clic no se pierda.\n"
        + "soltar_bloqueo=no\n"
        + "\n"
        + TEMPERATURE
        + "\n"
        + RIVAL;

    /** Read from what a file holds; what cannot be read keeps its default. */
    public static Settings parse(Properties p) {
        Settings s = new Settings();
        double[] cps = numbers(p.getProperty("cps"), "-");
        if (cps != null && cps.length >= 1 && cps[0] > 0.0) {
            s.cpsMin = Math.min(cps[0], 20.0);
            s.cpsMax = Math.min(Math.max(cps.length > 1 ? cps[1] : cps[0], s.cpsMin), 20.0);
        }
        double[] v = numbers(p.getProperty("ping"), " ");
        if (v != null && v.length == 1 && v[0] >= 0.0) {
            s.ping = v[0];
        }
        v = numbers(p.getProperty("ping_rival"), " ");
        if (v != null && v.length == 1 && v[0] >= 0.0) {
            s.otherPing = v[0];
        }
        v = numbers(p.getProperty("perla_espera"), " ");
        if (v != null && v.length == 1 && v[0] >= 0.0) {
            s.pearlCooldownS = v[0];
        }
        String block = p.getProperty("bloqueo");
        if (block != null) {
            block = block.trim().toLowerCase(java.util.Locale.ROOT);
            s.block = !(block.equals("no") || block.equals("0") || block.equals("false") || block.equals("off"));
        }
        String full = p.getProperty("pocion_completa");
        if (full != null) {
            full = full.trim().toLowerCase(java.util.Locale.ROOT);
            s.fullHeal = full.equals("si") || full.equals("1") || full.equals("true") || full.equals("on");
        }
        String safe = p.getProperty("pocion_segura");
        if (safe != null && safe.trim().toLowerCase(java.util.Locale.ROOT).equals("oculta")) {
            s.potSafety = true;
            s.potSafetyHidden = true;
        } else {
            s.potSafety = yes(safe, s.potSafety);
        }
        s.turnRelease = yes(p.getProperty("soltar_bloqueo"), s.turnRelease);
        String rival = p.getProperty("vida_rival");
        if (rival != null) {
            rival = rival.trim().toLowerCase(java.util.Locale.ROOT);
            if (rival.equals("real") || rival.equals("no")) {
                s.hpGuess = 0;
                s.oppShown = 0;
            } else if (rival.equals("estimada")) {
                s.hpGuess = 1;
                s.oppShown = 0;
            } else if (rival.equals("auto")) {
                s.hpGuess = 2;
                s.oppShown = 0;
            } else if (rival.equals("oculta")) {
                s.hpGuess = 0;
                s.oppShown = 2;
            } else if (rival.equals("oculta_auto")) {
                s.hpGuess = 0;
                s.oppShown = 1;
            }
        }
        String value = p.getProperty("vida_rival_valor");
        if (value != null) {
            value = value.trim().toLowerCase(java.util.Locale.ROOT);
            if (value.equals("propia")) {
                s.oppShownHealth = -1.0f;
            } else {
                v = numbers(value, " ");
                if (v != null && v.length == 1 && v[0] >= 0.0 && v[0] <= 20.0) {
                    s.oppShownHealth = (float) v[0];
                }
            }
        }
        v = numbers(p.getProperty("temperatura"), " ");
        if (v != null && v.length == 1 && v[0] >= 0.0 && v[0] <= 2.0) {
            s.temperature = (float) v[0];
        }
        v = numbers(p.getProperty("vida_rival_golpe"), " ");
        if (v != null && v.length == 1 && v[0] > 0.0 && v[0] <= 5.0) {
            s.hpGuessScale = (float) v[0];
        }
        v = numbers(p.getProperty("arena"), " ");
        if (v != null && v.length == 3 && v[2] >= 1.0) {
            s.arenaSet = true;
            s.arenaX = v[0];
            s.arenaZ = v[1];
            s.arenaHalf = v[2];
        }
        return s;
    }

    /** A yes or no setting, or the default if it is not there. */
    private static boolean yes(String value, boolean otherwise) {
        if (value == null) {
            return otherwise;
        }
        value = value.trim().toLowerCase(java.util.Locale.ROOT);
        return value.equals("si") || value.equals("1") || value.equals("true") || value.equals("on");
    }

    /** The numbers of a value, or null if it is not all numbers. */
    private static double[] numbers(String value, String between) {
        if (value == null) {
            return null;
        }
        String[] parts = value.trim().replace(',', ' ').split(between.equals(" ") ? "\\s+" : "\\s*" + between + "\\s*");
        double[] r = new double[parts.length];
        try {
            for (int i = 0; i < parts.length; i++) {
                r[i] = Double.parseDouble(parts[i]);
                if (Double.isNaN(r[i]) || Double.isInfinite(r[i])) {
                    return null;
                }
            }
        } catch (NumberFormatException e) {
            return null;
        }
        return r;
    }

    /**
     * Read a file; one that is not there is written first, with the
     * defaults. One that holds the lines on the opponent's health as an
     * earlier jar wrote them, untouched, gets the new ones in their place;
     * lines a file lacks (the temperature, the opponent's health) are added,
     * before those on the opponent's health or at the end.
     */
    public static Settings load(File f) throws IOException {
        if (!f.exists()) {
            File dir = f.getParentFile();
            if (dir != null) {
                dir.mkdirs();
            }
            write(f, FIRST);
        } else {
            String text = upgrade(read(f));
            if (text != null) {
                write(f, text);
            }
        }
        Properties p = new Properties();
        InputStream in = new FileInputStream(f);
        try {
            p.load(new InputStreamReader(in, "UTF-8"));
        } finally {
            in.close();
        }
        return parse(p);
    }

    /** What a file should hold instead (see #load), or null if it is fine as it is. */
    static String upgrade(String text) {
        String t = text;
        if (t.contains(RIVAL_BEFORE)) {
            t = t.replace(RIVAL_BEFORE, RIVAL);
        }
        if (!t.contains("temperatura")) {
            int at = t.indexOf(RIVAL);
            t = at >= 0 ? t.substring(0, at) + TEMPERATURE + "\n" + t.substring(at) : end(t) + TEMPERATURE;
        }
        if (!t.contains("vida_rival")) {
            t = end(t) + RIVAL;
        }
        return t.equals(text) ? null : t;
    }

    /** The text with a blank line after it, to add lines at the end. */
    private static String end(String t) {
        return t.isEmpty() ? t : t.endsWith("\n\n") ? t : t.endsWith("\n") ? t + "\n" : t + "\n\n";
    }

    private static String read(File f) throws IOException {
        InputStream in = new FileInputStream(f);
        try {
            java.io.ByteArrayOutputStream b = new java.io.ByteArrayOutputStream();
            byte[] buf = new byte[4096];
            for (int n; (n = in.read(buf)) > 0; ) {
                b.write(buf, 0, n);
            }
            return new String(b.toByteArray(), "UTF-8");
        } finally {
            in.close();
        }
    }

    private static void write(File f, String text) throws IOException {
        OutputStream out = new FileOutputStream(f);
        try {
            out.write(text.getBytes("UTF-8"));
        } finally {
            out.close();
        }
    }
}
