package pvpai.client;

import java.lang.reflect.Field;

/**
 * Fields of the game that are not public, looked up by the name they have
 * in a workspace and by the one they have in a running game.
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
            }
        }
        throw new IllegalStateException("no field " + names[0] + " in " + owner.getName());
    }

    /** The same, or null when the game has none of these names: what needs it is then left off. */
    static Field fieldOrNull(Class<?> owner, String... names) {
        try {
            return field(owner, names);
        } catch (IllegalStateException e) {
            return null;
        }
    }

    static int getInt(Field f, Object o) {
        try {
            return f.getInt(o);
        } catch (IllegalAccessException e) {
            throw new IllegalStateException(e);
        }
    }

    static boolean getBoolean(Field f, Object o) {
        try {
            return f.getBoolean(o);
        } catch (IllegalAccessException e) {
            throw new IllegalStateException(e);
        }
    }

    static double getDouble(Field f, Object o) {
        try {
            return f.getDouble(o);
        } catch (IllegalAccessException e) {
            throw new IllegalStateException(e);
        }
    }

    static float getFloat(Field f, Object o) {
        try {
            return f.getFloat(o);
        } catch (IllegalAccessException e) {
            throw new IllegalStateException(e);
        }
    }

    static Object get(Field f, Object o) {
        try {
            return f.get(o);
        } catch (IllegalAccessException e) {
            throw new IllegalStateException(e);
        }
    }
}
