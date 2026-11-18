package chestai.client;

import java.lang.reflect.Field;

/**
 * Fields of the game that are not public, looked up by their name in a
 * workspace and in a running game. Null if neither is there.
 */
final class Names {
    private Names() {
    }

    static Field field(Class<?> owner, String... names) {
        for (String n : names) {
            try {
                Field f = owner.getDeclaredField(n);
                f.setAccessible(true);
                return f;
            } catch (NoSuchFieldException e) {
                // The next name.
            } catch (RuntimeException e) {
                return null;
            }
        }
        return null;
    }

    static void setInt(Field f, Object o, int v) {
        if (f == null) {
            return;
        }
        try {
            f.setInt(o, v);
        } catch (IllegalAccessException e) {
            // Left as it is.
        }
    }
}
