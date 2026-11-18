package chestai.core;

import java.util.List;
import java.util.Locale;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * What a chest's hologram says, as a state of the simulator: the claim
 * line, a red line while a wave runs, and the countdown to the next wave
 * ("Next Wave: 2 minutes 5 seconds", "45 seconds", "1:30"). A countdown
 * at zero is a chest that is ready.
 */
public final class HoloText {
    private static final Pattern MIN_SEC = Pattern.compile("(\\d+)\\s*minutes?\\s*(?:and\\s*)?(\\d{1,2})\\s*seconds?", Pattern.CASE_INSENSITIVE);
    private static final Pattern MIN = Pattern.compile("(\\d+)\\s*minutes?", Pattern.CASE_INSENSITIVE);
    private static final Pattern SEC = Pattern.compile("(\\d+)\\s*seconds?", Pattern.CASE_INSENSITIVE);
    private static final Pattern CLOCK = Pattern.compile("(?:^|\\D)(\\d+):(\\d{1,2})(?:\\D|$)");

    private HoloText() {
    }

    /** The text without colour codes. */
    public static String plain(String s) {
        return s.replaceAll("§.", "").replaceAll("\\s+", " ").trim();
    }

    static int countdown(String line) {
        Matcher m = MIN_SEC.matcher(line);
        if (m.find()) {
            return Integer.parseInt(m.group(1)) * 60 + Integer.parseInt(m.group(2));
        }
        m = SEC.matcher(line);
        if (m.find()) {
            return Integer.parseInt(m.group(1));
        }
        m = MIN.matcher(line);
        if (m.find()) {
            return Integer.parseInt(m.group(1)) * 60;
        }
        m = CLOCK.matcher(line);
        if (m.find()) {
            return Integer.parseInt(m.group(1)) * 60 + Integer.parseInt(m.group(2));
        }
        return -1;
    }

    /** The first colour code of a line, or 0. */
    static char color(String raw) {
        for (int i = 0; i + 1 < raw.length(); i++) {
            if (raw.charAt(i) == '§') {
                char c = Character.toLowerCase(raw.charAt(i + 1));
                if ((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f')) {
                    return c;
                }
            }
        }
        return 0;
    }

    /**
     * {state, seconds} of the lines of a hologram (with their colour codes),
     * or null if they say nothing the agent knows.
     */
    public static int[] parse(List<String> lines, String claimText, String activeColors) {
        boolean red = false;
        int secs = -1;
        for (String raw : lines) {
            String p = plain(raw).toLowerCase(Locale.ROOT);
            if (p.length() == 0) {
                continue;
            }
            if (p.contains(claimText)) {
                return new int[] {Frame.CLAIMABLE, 0};
            }
            if (p.equals("wave defeated")) {
                continue;
            }
            char c = color(raw);
            if (c != 0 && activeColors.indexOf(c) >= 0) {
                red = true;
            }
            if (secs < 0) {
                secs = countdown(p);
            }
        }
        if (red) {
            return new int[] {Frame.ACTIVE, 0};
        }
        if (secs > 0) {
            return new int[] {Frame.COOLDOWN, secs};
        }
        if (secs == 0) {
            return new int[] {Frame.READY, 0};
        }
        return null;
    }
}
