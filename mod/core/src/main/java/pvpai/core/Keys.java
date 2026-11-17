package pvpai.core;

/** The keys and the mouse of one client tick. */
public final class Keys {
    /** W is 1 and S is -1. */
    public int forward;
    /** A (left) is 1 and D (right) is -1. */
    public int strafe;
    public boolean jump;
    public boolean sprintKey;
    /** One full left click inside this tick. */
    public boolean attack;
    /** Further full left clicks inside the same tick. */
    public int extraClicks;
    /** Mouse counts since the last tick: positive turns right, and looks down. */
    public int dx, dy;
    /** The right button is down on this tick. */
    public boolean useItem;
    /** The right button was let go and pressed again since the last tick. */
    public boolean useAgain;
    /** A number key pressed on this tick, 1 to 9 (0: none). */
    public int slotKey;
    /**
     * The inventory screen. With it shut, anything but 0 opens it. With it
     * open: 0 shuts it, 1 leaves it open, 9 to 44 shift-clicks that slot of
     * the window, and {@link Inv#SHUT} more than a slot shift-clicks it and
     * shuts the screen in the same tick. On a tick that belongs to the
     * screen nothing else here is set.
     */
    public int inv;

    public Keys copy() {
        Keys k = new Keys();
        k.forward = forward;
        k.strafe = strafe;
        k.jump = jump;
        k.sprintKey = sprintKey;
        k.attack = attack;
        k.extraClicks = extraClicks;
        k.dx = dx;
        k.dy = dy;
        k.useItem = useItem;
        k.useAgain = useAgain;
        k.slotKey = slotKey;
        k.inv = inv;
        return k;
    }

    /** The numbers above in the order they are declared in. */
    public int[] list() {
        return new int[] {forward, strafe, jump ? 1 : 0, sprintKey ? 1 : 0, attack ? 1 : 0, extraClicks, dx, dy, useItem ? 1 : 0, useAgain ? 1 : 0, slotKey, inv};
    }
}
