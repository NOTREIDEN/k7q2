package chestai.core;

import java.util.Arrays;

/** The hologram texts the server shows, read as the simulator's states. */
public final class HoloCheck {
    static int fails;

    static void is(String[] lines, int state, int secs) {
        int[] r = HoloText.parse(Arrays.asList(lines), "click here to claim", "c4");
        int s = r == null ? -1 : r[0], n = r == null ? -1 : r[1];
        if (s != state || n != secs) {
            fails++;
            System.out.println(Arrays.toString(lines) + ": " + s + " " + n + ", wanted " + state + " " + secs);
        }
    }

    /** A running wave's time left as read from its hologram. */
    static void left(String[] lines, int secs) {
        int[] r = HoloText.parse(Arrays.asList(lines), "click here to claim", "c4");
        int n = r == null || r.length < 3 ? -2 : r[2];
        if (n != secs) {
            fails++;
            System.out.println(Arrays.toString(lines) + ": wave left " + n + ", wanted " + secs);
        }
    }

    public static void main(String[] a) {
        is(new String[] {"§aNext Wave: §f2 minutes 5 seconds"}, Frame.COOLDOWN, 125);
        is(new String[] {"§eNext Wave: 45 seconds"}, Frame.COOLDOWN, 45);
        is(new String[] {"Next Wave: 1 minute 0 seconds"}, Frame.COOLDOWN, 60);
        is(new String[] {"Next Wave: 0 seconds"}, Frame.READY, 0);
        is(new String[] {"§a§lWAVE DEFEATED", "§eClick here to claim the rewards!"}, Frame.CLAIMABLE, 0);
        is(new String[] {"§cKill the mobs!"}, Frame.ACTIVE, 0);
        is(new String[] {"§4§lWave 3", "§c8 seconds left"}, Frame.ACTIVE, 0);
        is(new String[] {"§bSome chest"}, -1, -1);
        left(new String[] {"§4§lWave 3", "§c8 seconds left"}, 8);
        left(new String[] {"§cKill the mobs! §f17 seconds"}, 17);
        left(new String[] {"§c§lWAVE IN PROGRESS", "§7Kill the creatures to get the rewards!"}, -1);
        System.out.println(fails == 0 ? "holograms read right" : fails + " wrong");
        if (fails > 0) {
            System.exit(1);
        }
    }
}
