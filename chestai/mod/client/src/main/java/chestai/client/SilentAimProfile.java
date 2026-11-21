package chestai.client;

import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.Locale;
import java.util.Properties;

/**
 * What the player sets of Minecadia's SilentAim for the farmer's blows,
 * in silentaim.properties in config/chestai: each option by its key, a
 * missing or broken value read as the option's default.
 */
final class SilentAimProfile {
    private final Path file;
    private final Properties values = new Properties();

    SilentAimProfile(File dir) throws IOException {
        file = new File(dir, "silentaim.properties").toPath();
        if (Files.isRegularFile(file)) {
            try (InputStream in = Files.newInputStream(file)) {
                values.load(in);
            }
        }
    }

    /** The option's value: a Float, an Integer, a Boolean or one of its choices. */
    Object get(Option option) {
        return option.parse(values.getProperty(option.key));
    }

    /**
     * Moves the option one step (a number by its step, -1 down and 1 up;
     * a yes or no and a choice to the next) and saves the file, whole or
     * not at all (written aside and moved over the old one).
     */
    void change(Option option, int delta) throws IOException {
        Properties next = new Properties();
        next.putAll(values);
        next.setProperty(option.key, option.adjust(get(option), delta).toString());
        Files.createDirectories(file.getParent());
        Path tmp = Files.createTempFile(file.getParent(), "silentaim-", ".tmp");
        try {
            try (OutputStream out = Files.newOutputStream(tmp)) {
                next.store(out, "Chest AI SilentAim");
            }
            Files.move(tmp, file, StandardCopyOption.REPLACE_EXISTING);
            values.putAll(next);
        } finally {
            Files.deleteIfExists(tmp);
        }
    }

    /** The options: the key in the file, the name of SilentAim's value it sets, and what the screen calls it. */
    enum Option {
        SPEED("velocidad", "Aim speed", "Velocidad", 7.0F, 1, 10, 0.1F),
        /** 0 leaves SilentAim off, and with it every blow (the farmer only goes to the mobs and looks at them). */
        ANGLE("angulo", "Max angle", "Ángulo máximo", 360, 0, 360, 5),
        AREA("zona", "Target area", "Zona del objetivo", "Center", "Closest"),
        MOVEMENT("movimiento", "Movement correction", "Corrección de movimiento", "Proper", "None", "Slow"),
        PREDICT("prediccion", "Predict movement", "Predecir movimiento", false),
        SHOW("mostrar", "Show target", "Mostrar objetivo", false),
        RENDER("indicador", "Render type", "Indicador", "Ring", "Box"),
        THIRD_PERSON("tercera_persona", "3rd person aim view", "Ver mira en tercera persona", false);

        final String key, moduleValue, label;
        final Object fallback;
        /** The choices of a choice (the first one the default), none for a number or a yes or no. */
        final String[] choices;
        /** The range of a number and its step; a step of 0 for what is not a number. */
        final float min, max, step;

        /** A yes or no. */
        Option(String key, String moduleValue, String label, Object fallback) {
            this(key, moduleValue, label, fallback, 0, 0, 0);
        }

        /** A choice, the first one the default. */
        Option(String key, String moduleValue, String label, String... choices) {
            this(key, moduleValue, label, choices[0], 0, 0, 0, choices);
        }

        Option(String key, String moduleValue, String label, Object fallback, float min, float max, float step, String... choices) {
            this.key = key;
            this.moduleValue = moduleValue;
            this.label = label;
            this.fallback = fallback;
            this.min = min;
            this.max = max;
            this.step = step;
            this.choices = choices;
        }

        Object parse(String text) {
            if (text == null) {
                return fallback;
            }
            if (step != 0) {
                try {
                    float v = Float.parseFloat(text);
                    return Float.isFinite(v) ? number(v) : fallback;
                } catch (NumberFormatException e) {
                    return fallback;
                }
            }
            if (fallback instanceof Boolean) {
                return Boolean.parseBoolean(text);
            }
            for (String choice : choices) {
                if (choice.equalsIgnoreCase(text)) {
                    return choice;
                }
            }
            return fallback;
        }

        /** A number within the range, on a step, of the default's kind (to a tenth for a Float). */
        private Object number(float v) {
            float stepped = Math.max(min, Math.min(max, Math.round(v / step) * step));
            if (fallback instanceof Integer) {
                return (int) stepped;
            }
            return Math.round(stepped * 10) / 10.0F;
        }

        Object adjust(Object value, int delta) {
            if (step != 0) {
                return number(((Number) value).floatValue() + delta * step);
            }
            if (value instanceof Boolean) {
                return !(Boolean) value;
            }
            for (int i = 0; i < choices.length; i++) {
                if (choices[i].equals(value)) {
                    return choices[(i + 1) % choices.length];
                }
            }
            return fallback;
        }

        String display(Object value) {
            if (value instanceof Float) {
                return String.format(Locale.ROOT, "%.1f", (float) (Float) value);
            }
            if (value instanceof Boolean) {
                return (Boolean) value ? "Sí" : "No";
            }
            return value.toString();
        }
    }
}
