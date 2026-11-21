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
    private static final Pattern MIN_SEC = Pattern.compile("(\\d{1,4})\\s*(?:minutes?|minutos?|mins?|m)\\b\\s*(?:and\\s*|y\\s*|,\\s*)?(\\d{1,2})\\s*(?:seconds?|segundos?|secs?|segs?|s)\\b", Pattern.CASE_INSENSITIVE);
    private static final Pattern MIN = Pattern.compile("(\\d{1,4})\\s*(?:minutes?|minutos?|mins?|m)\\b", Pattern.CASE_INSENSITIVE);
    private static final Pattern SEC = Pattern.compile("(\\d{1,5})\\s*(?:seconds?|segundos?|secs?|segs?|s)\\b", Pattern.CASE_INSENSITIVE);
    private static final Pattern CLOCK = Pattern.compile("(?:^|\\D)(\\d{1,4}):(\\d{1,2})(?:\\D|$)");
    /** A line about the next wave (its countdown is the one that counts), and a line of a wave that runs (of lowercase text). */
    private static final Pattern NEXT = Pattern.compile("next wave|pr[o\u00f3]xima oleada|\\brespawns?\\b|\\bspawns? in\\b|reaparece|cooldown");
    private static final Pattern ACTIVE = Pattern.compile("wave in progress|oleada en curso|kill (?:the )?(?:mobs|creatures|monsters)|mat[a\u00e1].*(?:mobs|criaturas|monstruos)");

    private HoloText() {
    }

    /** The text without colour codes. */
    public static String plain(String s) {
        return s.replaceAll("§.", "").replaceAll("\\s+", " ").trim();
    }

    /** Whether a line is one of a chest's hologram: the claim line, a wave's, the next wave's or a defeated wave's. */
    public static boolean isChestStatus(String raw, String claimText) {
        String p = plain(raw).toLowerCase(Locale.ROOT);
        if (NEXT.matcher(p).find() || ACTIVE.matcher(p).find() || p.equals("wave defeated") || p.equals("oleada derrotada")) {
            return true;
        }
        return isClaim(p, claimText.split(","));
    }

    /** Whether a line is the claim line. */
    public static boolean isClaimLabel(String raw, String claimText) {
        return isClaim(plain(raw).toLowerCase(Locale.ROOT), claimText.split(","));
    }

    /** Whether the lowercase plain line {@code p} has one of the claim texts in it. */
    private static boolean isClaim(String p, String[] claims) {
        for (String ct : claims) {
            String t = ct.trim().toLowerCase(Locale.ROOT);
            if (!t.isEmpty() && p.contains(t)) {
                return true;
            }
        }
        return false;
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
        // A wave runs by what a line says; a line in a wave's colour only counts with no countdown.
        boolean active = false, colored = false;
        int secs = -1, nextSecs = -1;
        String[] claims = claimText.split(",");
        for (String raw : lines) {
            String p = plain(raw).toLowerCase(Locale.ROOT);
            if (p.length() == 0) {
                continue;
            }
            if (isClaim(p, claims)) {
                return new int[] {Frame.CLAIMABLE, 0};
            }
            if (p.equals("wave defeated") || p.equals("oleada derrotada")) {
                continue;
            }
            boolean next = NEXT.matcher(p).find();
            active |= !next && ACTIVE.matcher(p).find();
            char c = color(raw);
            colored |= c != 0 && activeColors.indexOf(c) >= 0;
            int s = countdown(p);
            if (next && s >= 0 && nextSecs < 0) {
                nextSecs = s;
            }
            if (secs < 0) {
                secs = s;
            }
        }
        if (active) {
            return new int[] {Frame.ACTIVE, 0};
        }
        int left = nextSecs >= 0 ? nextSecs : secs;
        if (left >= 0) {
            return new int[] {left > 0 ? Frame.COOLDOWN : Frame.READY, left};
        }
        return colored ? new int[] {Frame.ACTIVE, 0} : null;
    }
}
