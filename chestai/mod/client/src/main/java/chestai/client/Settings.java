package chestai.client;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Properties;

/** What the player sets, kept in config/chestai/ajustes.txt. */
public final class Settings {
    public double cpsMin = 6.0, cpsMax = 9.0;
    /** Less than zero: what the player list says. */
    public double ping = -1.0;
    /** How far from its chest a mob may get before the server takes it back. */
    public double leash = 5.5;
    /** Seconds the server makes a player wait between two pearls. */
    public double pearlCooldownS = 15.0;
    public String claimText = "click here to claim";
    /** Colour codes (the letter after the section sign) of the line a running wave shows. */
    public String activeColors = "c4";
    /** Draw the agent's choices as in training (true), or take the likeliest one. */
    public boolean draw = true;
    /** Mobs (by entity name, lower case) to leave alone. */
    public final List<String> ignore = new ArrayList<String>();

    public static final String FIRST = ""
        + "# Chest AI: ajustes. Se leen cada vez que se prende el bot.\n"
        + "\n"
        + "# Clics por segundo: un rango del que se sortea uno cada vez que se prende (6 a 9 como en el entrenamiento).\n"
        + "cps=6-9\n"
        + "\n"
        + "# Ping propio en milisegundos. Con auto se lee de la lista de jugadores (TAB).\n"
        + "ping=auto\n"
        + "\n"
        + "# Bloques que un mob se puede alejar de su cofre antes de que el servidor lo teletransporte de vuelta.\n"
        + "correa=5.5\n"
        + "\n"
        + "# Segundos de espera del servidor entre dos perlas.\n"
        + "perla_espera=15\n"
        + "\n"
        + "# Texto del holograma cuando el cofre se puede reclamar (sin importar mayusculas).\n"
        + "texto_claim=click here to claim\n"
        + "\n"
        + "# Colores (la letra despues de la seccion) del mensaje del holograma mientras hay una oleada. c = rojo, 4 = rojo oscuro.\n"
        + "colores_oleada=c4\n"
        + "\n"
        + "# azar: elige como en el entrenamiento (recomendado). mejor: siempre la opcion mas probable.\n"
        + "eleccion=azar\n"
        + "\n"
        + "# Mobs a ignorar, por nombre de entidad separados por comas (por ejemplo silverfish,bat).\n"
        + "ignorar=bat\n";

    public static Settings parse(Properties p) {
        Settings s = new Settings();
        String cps = p.getProperty("cps");
        if (cps != null) {
            String[] parts = cps.trim().split("\\s*-\\s*");
            try {
                double a = Double.parseDouble(parts[0]);
                double b = parts.length > 1 ? Double.parseDouble(parts[1]) : a;
                if (a > 0 && b >= a) {
                    s.cpsMin = Math.min(a, 20.0);
                    s.cpsMax = Math.min(b, 20.0);
                }
            } catch (NumberFormatException e) {
                // The default.
            }
        }
        s.ping = number(p.getProperty("ping"), -1.0);
        s.leash = number(p.getProperty("correa"), 5.5);
        s.pearlCooldownS = number(p.getProperty("perla_espera"), 15.0);
        String t = p.getProperty("texto_claim");
        if (t != null && t.trim().length() > 0) {
            s.claimText = t.trim().toLowerCase(Locale.ROOT);
        }
        t = p.getProperty("colores_oleada");
        if (t != null && t.trim().length() > 0) {
            s.activeColors = t.trim().toLowerCase(Locale.ROOT);
        }
        t = p.getProperty("eleccion");
        s.draw = t == null || !t.trim().toLowerCase(Locale.ROOT).startsWith("mejor");
        t = p.getProperty("ignorar");
        if (t != null) {
            for (String n : t.split(",")) {
                if (n.trim().length() > 0) {
                    s.ignore.add(n.trim().toLowerCase(Locale.ROOT));
                }
            }
        }
        return s;
    }

    private static double number(String v, double dflt) {
        if (v == null) {
            return dflt;
        }
        try {
            double d = Double.parseDouble(v.trim());
            return Double.isNaN(d) || d < 0 ? dflt : d;
        } catch (NumberFormatException e) {
            return dflt;
        }
    }

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
