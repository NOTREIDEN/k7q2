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
        + "pocion_completa=no\n";

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
        v = numbers(p.getProperty("arena"), " ");
        if (v != null && v.length == 3 && v[2] >= 1.0) {
            s.arenaSet = true;
            s.arenaX = v[0];
            s.arenaZ = v[1];
            s.arenaHalf = v[2];
        }
        return s;
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

    /** Read a file; one that is not there is written first, with the defaults. */
    public static Settings load(File f) throws IOException {
        if (!f.exists()) {
            File dir = f.getParentFile();
            if (dir != null) {
                dir.mkdirs();
            }
            OutputStream out = new FileOutputStream(f);
            try {
                out.write(FIRST.getBytes("UTF-8"));
            } finally {
                out.close();
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
}
