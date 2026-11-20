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
    public String claimText = "click here to claim,click to claim,reclamar";
    /** Colour codes (the letter after the section sign) of the line a running wave shows. */
    public String activeColors = "c4";
    /** Draw the agent's choices as in training (true), or take the likeliest one. */
    public boolean draw = true;
    /** Write what the agent sees and does to config/chestai/registro.txt. */
    public boolean log = true;
    /** Mobs (by entity name, lower case) to leave alone. */
    public final List<String> ignore = new ArrayList<String>();
    /** Health (of 20) at which a splash potion is thrown, and at which a golden apple is eaten. */
    public double potHp = 10.0, appleHp = 14.0;
    /** Seconds at least between two golden apples. */
    public double appleWaitS = 8.0;
    /** Punch arrows shot up to travel far. */
    public boolean bow = true;
    /** Chests of the loaded chunks taken as possible farm chests until a look shows they are not. */
    public boolean search = true;
    public double searchRadius = 160.0;
    /** The blocks of every chunk seen kept on disk, for planning ways. */
    public boolean map = true;

    /** The lines of the newer settings, added to a file written by an older version. */
    static final String MORE = ""
        + "# Vida (de 20) a la que tira una pocion de curacion a los pies.\n"
        + "pocion_vida=10\n"
        + "\n"
        + "# Vida (de 20) a la que come una manzana dorada, solo si le estan pegando y no le queda efecto de otra.\n"
        + "manzana_vida=14\n"
        + "\n"
        + "# Segundos minimos entre dos manzanas doradas.\n"
        + "manzana_espera=8\n"
        + "\n"
        + "# si: para ir lejos en campo abierto se tira una flecha con Punch para arriba que lo empuja.\n"
        + "arco_punch=si\n"
        + "\n"
        + "# si: los cofres de los chunks cargados (hasta buscar_radio bloques) se toman como posibles cofres de farmeo\n"
        + "# hasta que se acerca y ve que no tienen holograma de oleada.\n"
        + "buscar_cofres=si\n"
        + "buscar_radio=160\n"
        + "\n"
        + "# si: guarda los bloques de cada chunk que ve en config/chestai/mapa-<servidor>/ para planear caminos.\n"
        + "mapa=si\n";

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
        + "# Textos del holograma cuando el cofre se puede reclamar, separados por comas (sin importar mayusculas).\n"
        + "texto_claim=click here to claim,click to claim,reclamar\n"
        + "\n"
        + "# Colores (la letra despues de la seccion) del mensaje del holograma mientras hay una oleada. c = rojo, 4 = rojo oscuro.\n"
        + "colores_oleada=c4\n"
        + "\n"
        + "# azar: elige como en el entrenamiento (recomendado). mejor: siempre la opcion mas probable.\n"
        + "eleccion=azar\n"
        + "\n"
        + "# Mobs a ignorar, por nombre de entidad separados por comas (por ejemplo silverfish,bat).\n"
        + "ignorar=bat\n"
        + "\n"
        + "# si: anota lo que ve y hace en config/chestai/registro.txt (para revisar problemas). no: nada.\n"
        + "registro=si\n"
        + "\n" + MORE;

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
        String lg = p.getProperty("registro");
        if (lg != null) {
            s.log = !lg.trim().toLowerCase(Locale.ROOT).startsWith("n");
        }
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
        s.potHp = Math.min(number(p.getProperty("pocion_vida"), 10.0), 20.0);
        s.appleHp = Math.min(number(p.getProperty("manzana_vida"), 14.0), 20.0);
        s.appleWaitS = number(p.getProperty("manzana_espera"), 8.0);
        s.bow = yes(p.getProperty("arco_punch"), true);
        s.search = yes(p.getProperty("buscar_cofres"), true);
        s.searchRadius = number(p.getProperty("buscar_radio"), 160.0);
        s.map = yes(p.getProperty("mapa"), true);
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

    private static boolean yes(String v, boolean dflt) {
        return v == null || v.trim().length() == 0 ? dflt : !v.trim().toLowerCase(Locale.ROOT).startsWith("n");
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
        // A file of an older version: the newer settings added at its end.
        Properties old = new Properties();
        InputStream in0 = new FileInputStream(f);
        try {
            old.load(new InputStreamReader(in0, "UTF-8"));
        } finally {
            in0.close();
        }
        if (old.getProperty("pocion_vida") == null) {
            OutputStream out = new FileOutputStream(f, true);
            try {
                out.write(("\n" + MORE).getBytes("UTF-8"));
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
