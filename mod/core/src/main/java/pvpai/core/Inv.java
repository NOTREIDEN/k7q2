package pvpai.core;

/** The hotbar and the inventory of a duel, and a shift click between them. */
public final class Inv {
    public static final int SWORD = 0, PEARL = 1, POTION = 2, DRINK = 3, BOTTLE = 4, FOOD = 5, EMPTY = 6;
    /** Something a duel has no use for: its slot is taken, and that is all an agent goes by. */
    public static final int OTHER = 7;
    public static final int KIND_PEARL = 0, KIND_POTION = 1;
    /** Hotbar slot of the sword, of the pearls and of the steak. */
    public static final int SLOT_SWORD = 0, SLOT_PEARL = 1, SLOT_FOOD = 8;
    /** The ticks a drink or a meal takes. */
    public static final int USE_TICKS = 32;
    /** Window slots of the first inventory row and of the first hotbar slot. */
    public static final int WINDOW_ROWS = 9, WINDOW_BAR = 36;
    /** Added to a slot in {@link Keys#inv}: shift-click it and shut the screen. */
    public static final int SHUT = 64;
    static final int BOTTLE_STACK = 64;

    /** The part of the hotbar that changes during a duel. */
    public static final class Bar {
        public int pearls, potions, drinks, bottles, steak;
        /** One bit per slot taken by something else, which stays where it is. */
        public int others;

        public Bar(int pearls, int potions, int drinks, int bottles, int steak) {
            this.pearls = pearls;
            this.potions = potions;
            this.drinks = drinks;
            this.bottles = bottles;
            this.steak = steak;
        }

        public static Bar of(View v) {
            Bar b = new Bar(v.pearls, v.potions, v.drinks, v.bottles, v.steak);
            b.others = v.others;
            return b;
        }

        /** The item in a slot other than the sword's. */
        public int item(int slot) {
            if (slot > 8 || slot == SLOT_SWORD) {
                return EMPTY;
            }
            if ((potions >> slot & 1) != 0) {
                return POTION;
            }
            if ((drinks >> slot & 1) != 0) {
                return DRINK;
            }
            if ((bottles >> slot & 1) != 0) {
                return BOTTLE;
            }
            if ((others >> slot & 1) != 0) {
                return OTHER;
            }
            if (slot == SLOT_PEARL && pearls > 0) {
                return PEARL;
            }
            if (slot == SLOT_FOOD && steak > 0) {
                return FOOD;
            }
            return EMPTY;
        }

        void put(int slot, int item, int count) {
            if (slot == SLOT_SWORD || slot > 8) {
                return;
            }
            int bit = 1 << slot;
            potions &= ~bit;
            drinks &= ~bit;
            bottles &= ~bit;
            if (slot == SLOT_PEARL) {
                pearls = 0;
            }
            if (slot == SLOT_FOOD) {
                steak = 0;
            }
            if (count == 0) {
                return;
            }
            if (item == POTION) {
                potions |= bit;
            } else if (item == DRINK) {
                drinks |= bit;
            } else if (item == BOTTLE) {
                bottles |= bit;
            } else if (item == PEARL && slot == SLOT_PEARL) {
                pearls = count;
            } else if (item == FOOD && slot == SLOT_FOOD) {
                steak = count;
            }
        }

        /** The first empty slot after the sword's, or -1. */
        public int freeSlot() {
            for (int s = 1; s < 9; s++) {
                if (item(s) == EMPTY) {
                    return s;
                }
            }
            return -1;
        }
    }

    static int rowItem(int v) {
        return v == 0 ? EMPTY : v == 1 ? POTION : v == 2 ? DRINK : BOTTLE;
    }

    /** How many slots of the rows hold this item. */
    public static int count(int[] rows, int item) {
        int n = 0;
        for (int v : rows) {
            if (rowItem(v) == item) {
                n++;
            }
        }
        return n;
    }

    /**
     * The window slot to shift-click next to get a potion of this kind down
     * from the rows: the potion if the hotbar has room, an empty bottle of
     * the hotbar if it has none, -1 if there is no such potion or no way.
     */
    public static int refillClick(Bar bar, int[] rows, int kind) {
        int from = -1;
        for (int i = 0; i < 27; i++) {
            if (rowItem(rows[i]) == kind) {
                from = i + WINDOW_ROWS;
                break;
            }
        }
        if (from < 0) {
            return -1;
        }
        if (bar.freeSlot() >= 0) {
            return from;
        }
        for (int s = 1; s < 9; s++) {
            if (bar.item(s) == BOTTLE) {
                return WINDOW_BAR + s;
            }
        }
        return -1;
    }

    /** What a shift click on this window slot does to the hotbar and the rows. Returns whether anything moved. */
    public static boolean shiftClick(Bar bar, int[] rows, int slot) {
        if (slot >= WINDOW_ROWS && slot < WINDOW_BAR) {
            int i = slot - WINDOW_ROWS;
            int item = rowItem(rows[i]);
            if (item != POTION && item != DRINK) {
                return false;
            }
            int to = bar.freeSlot();
            if (to < 0) {
                return false;
            }
            bar.put(to, item, 1);
            rows[i] = 0;
            return true;
        }
        if (slot >= WINDOW_BAR + 1 && slot < WINDOW_BAR + 9) {
            int s = slot - WINDOW_BAR;
            int item = bar.item(s);
            if (item == POTION || item == DRINK) {
                int to = firstEmpty(rows);
                if (to < 0) {
                    return false;
                }
                rows[to] = item == POTION ? 1 : 2;
            } else if (item == BOTTLE) {
                int room = -1;
                for (int i = 0; i < 27; i++) {
                    if (rows[i] >= 3 && rows[i] - 2 < BOTTLE_STACK) {
                        room = i;
                        break;
                    }
                }
                if (room >= 0) {
                    rows[room] += 1;
                } else {
                    int to = firstEmpty(rows);
                    if (to < 0) {
                        return false;
                    }
                    rows[to] = 3;
                }
            } else {
                return false;
            }
            bar.put(s, EMPTY, 0);
            return true;
        }
        return false;
    }

    private static int firstEmpty(int[] rows) {
        for (int i = 0; i < 27; i++) {
            if (rows[i] == 0) {
                return i;
            }
        }
        return -1;
    }

    private Inv() {}
}
