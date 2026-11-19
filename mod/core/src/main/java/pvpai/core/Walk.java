package pvpai.core;

/**
 * The client's own player as far as its movement goes, and the part of the
 * client tick that moves it: a copy, line for line, of what the simulator
 * does (sim/src/client.rs {@code Client::tick}, living.rs, body.rs), which
 * is the game's {@code EntityPlayerSP.onUpdate} with
 * {@code onLivingUpdate}, {@code moveEntityWithHeading},
 * {@code moveEntity} and {@code onUpdateWalkingPlayer}. Only ticks without
 * a click, a right button or the inventory are made here: what an agent
 * works out ahead for itself.
 */
public final strictfp class Walk {
    /** Player width 0.6F and height 1.8F, widened to double the way the game does. */
    static final double HALF_WIDTH = (double) 0.3f, HEIGHT = (double) 1.8f;
    private static final double STEP_HEIGHT = (double) 0.6f;
    private static final double SPRINT_BOOST = 0.30000001192092896;
    /** The amount of the modifier of a potion of Speed of amplifier 0: amplifier a gives (a + 1) times it. */
    public static final double SPEED_MODIFIER = 0.20000000298023224;

    /** Where it stands, its box, and how it moves per tick. */
    public final double[] pos = new double[3], min = new double[3], max = new double[3], motion = new double[3];
    public boolean onGround, collidedH, collidedV;
    /** {@code isInWeb}: the last move ended with the box in a cobweb. */
    public boolean inWeb;
    public float fallDistance, yaw, pitch;
    /** The sprint bit of the entity's flags. */
    public boolean sprinting;
    /**
     * The movement speed attribute: its base, whether the sprint's
     * modifier is on, and the amount of the modifier of a potion of Speed
     * (0 for none).
     */
    public double attrBase = (double) 0.1f;
    public boolean attrSprint;
    public double speedModifier;
    /** {@code jumpMovementFactor} and {@code jumpTicks}. */
    public float jumpFactor = 0.02f;
    public int jumpTicks;
    /** {@code movementInput.moveForward} as the last tick left it. */
    public float moveForward;
    public int sprintingTicksLeft, sprintToggleTimer;
    public boolean serverSprint;
    /** What the server was last told: the place, the view, and the ticks since the place. */
    public final double[] said = new double[3];
    public float saidYaw, saidPitch;
    public int positionUpdateTicks;
    public int food = 20;
    public boolean usingItem, invOpen;

    /** What the last tick's movement packet carried: the place, the view, and onGround. */
    public boolean sentPos, sentRot, sentOnGround;

    public Walk copy() {
        Walk w = new Walk();
        System.arraycopy(pos, 0, w.pos, 0, 3);
        System.arraycopy(min, 0, w.min, 0, 3);
        System.arraycopy(max, 0, w.max, 0, 3);
        System.arraycopy(motion, 0, w.motion, 0, 3);
        w.onGround = onGround;
        w.collidedH = collidedH;
        w.collidedV = collidedV;
        w.inWeb = inWeb;
        w.fallDistance = fallDistance;
        w.yaw = yaw;
        w.pitch = pitch;
        w.sprinting = sprinting;
        w.attrBase = attrBase;
        w.attrSprint = attrSprint;
        w.speedModifier = speedModifier;
        w.jumpFactor = jumpFactor;
        w.jumpTicks = jumpTicks;
        w.moveForward = moveForward;
        w.sprintingTicksLeft = sprintingTicksLeft;
        w.sprintToggleTimer = sprintToggleTimer;
        w.serverSprint = serverSprint;
        System.arraycopy(said, 0, w.said, 0, 3);
        w.saidYaw = saidYaw;
        w.saidPitch = saidPitch;
        w.positionUpdateTicks = positionUpdateTicks;
        w.food = food;
        w.usingItem = usingItem;
        w.invOpen = invOpen;
        w.sentPos = sentPos;
        w.sentRot = sentRot;
        w.sentOnGround = sentOnGround;
        return w;
    }

    /**
     * The movement state of a view: what {@link View} has of the player,
     * and the rest from {@link View#walk}, which the client fills in
     * alongside.
     */
    public static Walk of(View v) {
        Walk w = v.walk.copy();
        System.arraycopy(v.pos, 0, w.pos, 0, 3);
        System.arraycopy(v.motion, 0, w.motion, 0, 3);
        w.onGround = v.onGround;
        w.collidedH = v.collidedH;
        w.fallDistance = v.fallDistance;
        w.yaw = v.yaw;
        w.pitch = v.pitch;
        w.sprinting = v.sprinting;
        w.jumpTicks = v.jumpTicks;
        w.serverSprint = v.serverSprint;
        w.food = v.food;
        w.usingItem = v.usingItem;
        w.invOpen = v.invOpen;
        return w;
    }

    /** {@code generic.movementSpeed}: the sprint first, then the potion (living.rs, {@code Attr::value}). */
    double speed() {
        double d1 = attrBase;
        if (attrSprint) {
            d1 *= 1.0 + SPRINT_BOOST;
        }
        if (speedModifier != 0.0) {
            d1 *= 1.0 + speedModifier;
        }
        return Geo.clamp(d1, 0.0, 1024.0);
    }

    /** {@code EntityPlayerSP.setSprinting}. */
    void setSprinting(boolean s) {
        sprinting = s;
        attrSprint = s;
        sprintingTicksLeft = s ? 600 : 0;
    }

    /** {@code Entity.setAngles(yaw, pitch)}. */
    void setAngles(float y, float p) {
        yaw = (float) ((double) yaw + (double) y * 0.15);
        pitch = (float) ((double) pitch - (double) p * 0.15);
        if (pitch < -90.0f) {
            pitch = -90.0f;
        } else if (pitch > 90.0f) {
            pitch = 90.0f;
        }
    }

    /**
     * One client tick with these keys and mouse counts and nothing else: no
     * click, the right button up, no number key that matters to movement.
     * mouseF1 is what a count turns the view by, over 0.15 degrees.
     */
    public void tick(Blocks blocks, int forward, int strafe, boolean jump, boolean sprintKey, int dx, int dy, float mouseF1) {
        if (invOpen) {
            // A tick with the screen open has no key and no mouse; with
            // none asked of the inventory it shuts.
            invOpen = false;
            forward = 0;
            strafe = 0;
            jump = false;
            sprintKey = false;
            dx = 0;
            dy = 0;
        }
        if (dx != 0 || dy != 0) {
            setAngles((float) dx * mouseF1, -((float) dy * mouseF1));
        }
        // The right button is up: an item in use is let go of.
        usingItem = false;
        if (sprintingTicksLeft > 0) {
            sprintingTicksLeft--;
            if (sprintingTicksLeft == 0) {
                setSprinting(false);
            }
        }
        if (sprintToggleTimer > 0) {
            sprintToggleTimer--;
        }
        float f = 0.8f;
        boolean flag2 = moveForward >= f;
        float ms = 0.0f, mf = 0.0f;
        if (forward > 0) {
            mf += 1.0f;
        }
        if (forward < 0) {
            mf -= 1.0f;
        }
        if (strafe > 0) {
            ms += 1.0f;
        }
        if (strafe < 0) {
            ms -= 1.0f;
        }
        moveForward = mf;
        double w = (double) 0.6f * 0.35;
        double px = pos[0], py = min[1] + 0.5, pz = pos[2];
        pushOutOfBlocks(blocks, px - w, py, pz + w);
        pushOutOfBlocks(blocks, px - w, py, pz - w);
        pushOutOfBlocks(blocks, px + w, py, pz - w);
        pushOutOfBlocks(blocks, px + w, py, pz + w);
        boolean flag3 = (float) food > 6.0f;
        if (onGround && !flag2 && mf >= f && !sprinting && flag3 && !usingItem) {
            if (sprintToggleTimer <= 0 && !sprintKey) {
                sprintToggleTimer = 7;
            } else {
                setSprinting(true);
            }
        }
        if (!sprinting && mf >= f && flag3 && !usingItem && sprintKey) {
            setSprinting(true);
        }
        if (sprinting && (mf < f || collidedH || !flag3)) {
            setSprinting(false);
        }
        livingUpdate(blocks, ms, mf, jump);
        jumpFactor = 0.02f;
        if (sprinting) {
            jumpFactor = (float) ((double) jumpFactor + (double) 0.02f * 0.3);
        }
        // onUpdateWalkingPlayer
        if (sprinting != serverSprint) {
            serverSprint = sprinting;
        }
        double d0 = pos[0] - said[0], d1 = min[1] - said[1], d2 = pos[2] - said[2];
        double d3 = (double) (yaw - saidYaw), d4 = (double) (pitch - saidPitch);
        boolean moved = d0 * d0 + d1 * d1 + d2 * d2 > 9.0E-4 || positionUpdateTicks >= 20;
        boolean turned = d3 != 0.0 || d4 != 0.0;
        sentPos = moved;
        sentRot = turned;
        sentOnGround = onGround;
        positionUpdateTicks++;
        if (moved) {
            said[0] = pos[0];
            said[1] = min[1];
            said[2] = pos[2];
            positionUpdateTicks = 0;
        }
        if (turned) {
            saidYaw = yaw;
            saidPitch = pitch;
        }
    }

    /** A full cube: what {@code isOpenBlockSpace} looks for. */
    private static boolean solid(Blocks b, int x, int y, int z) {
        return b.height(x, y, z) == 1.0 && !b.web(x, y, z);
    }

    /** {@code EntityPlayerSP.pushOutOfBlocks}. */
    private void pushOutOfBlocks(Blocks b, double x, double y, double z) {
        int bx = Geo.floor(x), by = Geo.floor(y), bz = Geo.floor(z);
        if (open(b, bx, by, bz)) {
            return;
        }
        double d0 = x - (double) bx, d1 = z - (double) bz;
        int i = -1;
        double d2 = 9999.0;
        if (open(b, bx - 1, by, bz) && d0 < d2) {
            d2 = d0;
            i = 0;
        }
        if (open(b, bx + 1, by, bz) && 1.0 - d0 < d2) {
            d2 = 1.0 - d0;
            i = 1;
        }
        if (open(b, bx, by, bz - 1) && d1 < d2) {
            d2 = d1;
            i = 4;
        }
        if (open(b, bx, by, bz + 1) && 1.0 - d1 < d2) {
            i = 5;
        }
        float f = 0.1f;
        if (i == 0) {
            motion[0] = (double) -f;
        } else if (i == 1) {
            motion[0] = (double) f;
        } else if (i == 4) {
            motion[2] = (double) -f;
        } else if (i == 5) {
            motion[2] = (double) f;
        }
    }

    private static boolean open(Blocks b, int x, int by, int z) {
        return !solid(b, x, by, z) && !solid(b, x, by + 1, z);
    }

    /** The common part of {@code EntityLivingBase.onLivingUpdate}. */
    private void livingUpdate(Blocks blocks, float strafe, float forward, boolean jumping) {
        if (jumpTicks > 0) {
            jumpTicks--;
        }
        for (int a = 0; a < 3; a++) {
            if (Math.abs(motion[a]) < 0.005) {
                motion[a] = 0.0;
            }
        }
        if (jumping) {
            if (onGround && jumpTicks == 0) {
                jump();
                jumpTicks = 10;
            }
        } else {
            jumpTicks = 0;
        }
        moveEntityWithHeading(blocks, strafe * 0.98f, forward * 0.98f);
    }

    /** {@code EntityLivingBase.jump}. */
    private void jump() {
        motion[1] = (double) 0.42f;
        if (sprinting) {
            float f = yaw * 0.017453292f;
            motion[0] -= (double) (Geo.sin(f) * 0.2f);
            motion[2] += (double) (Geo.cos(f) * 0.2f);
        }
    }

    /** {@code Entity.moveFlying}. */
    private void moveFlying(float strafe, float forward, float friction) {
        float f = strafe * strafe + forward * forward;
        if (f >= 1.0E-4f) {
            f = (float) Math.sqrt((double) f);
            if (f < 1.0f) {
                f = 1.0f;
            }
            f = friction / f;
            strafe *= f;
            forward *= f;
            float f1 = Geo.sin(yaw * Geo.PI_F / 180.0f);
            float f2 = Geo.cos(yaw * Geo.PI_F / 180.0f);
            motion[0] += (double) (strafe * f2 - forward * f1);
            motion[2] += (double) (forward * f2 + strafe * f1);
        }
    }

    /** {@code EntityLivingBase.moveEntityWithHeading}, on land, every block of slipperiness 0.6. */
    private void moveEntityWithHeading(Blocks blocks, float strafe, float forward) {
        float f4 = 0.91f;
        if (onGround) {
            f4 = 0.6f * 0.91f;
        }
        float f = 0.16277136f / (f4 * f4 * f4);
        float f5 = onGround ? (float) speed() * f : jumpFactor;
        moveFlying(strafe, forward, f5);
        f4 = 0.91f;
        if (onGround) {
            f4 = 0.6f * 0.91f;
        }
        moveEntity(blocks, motion[0], motion[1], motion[2]);
        motion[1] -= 0.08;
        motion[1] *= 0.9800000190734863;
        motion[0] *= (double) f4;
        motion[2] *= (double) f4;
    }

    /** {@code World.getCollidingBoundingBoxes} for a box: the boxes of the blocks that overlap it, each min x, y, z then max x, y, z. */
    private static double[][] colliding(Blocks blocks, double[] bb) {
        int i = Geo.floor(bb[0]), j = Geo.floor(bb[3] + 1.0);
        int k = Geo.floor(bb[1]), l = Geo.floor(bb[4] + 1.0);
        int i1 = Geo.floor(bb[2]), j1 = Geo.floor(bb[5] + 1.0);
        double[][] out = new double[8][];
        int n = 0;
        for (int x = i; x < j; x++) {
            for (int z = i1; z < j1; z++) {
                for (int y = k - 1; y < l; y++) {
                    double h = blocks.height(x, y, z);
                    if (h > 0.0) {
                        double[] b = {(double) x, (double) y, (double) z, (double) x + 1.0, (double) y + h, (double) z + 1.0};
                        if (intersects(bb, b)) {
                            if (n == out.length) {
                                out = java.util.Arrays.copyOf(out, 2 * n);
                            }
                            out[n++] = b;
                        }
                    }
                }
            }
        }
        return java.util.Arrays.copyOf(out, n);
    }

    private static boolean intersects(double[] a, double[] o) {
        return o[3] > a[0] && o[0] < a[3] && o[4] > a[1] && o[1] < a[4] && o[5] > a[2] && o[2] < a[5];
    }

    private static double[] addCoord(double[] bb, double x, double y, double z) {
        double[] r = bb.clone();
        if (x < 0.0) {
            r[0] += x;
        } else if (x > 0.0) {
            r[3] += x;
        }
        if (y < 0.0) {
            r[1] += y;
        } else if (y > 0.0) {
            r[4] += y;
        }
        if (z < 0.0) {
            r[2] += z;
        } else if (z > 0.0) {
            r[5] += z;
        }
        return r;
    }

    private static double[] offset(double[] bb, double x, double y, double z) {
        return new double[] {bb[0] + x, bb[1] + y, bb[2] + z, bb[3] + x, bb[4] + y, bb[5] + z};
    }

    /** {@code s.calculateXOffset(o, off)}: s is the obstacle. */
    private static double xOffset(double[] s, double[] o, double off) {
        if (o[4] > s[1] && o[1] < s[4] && o[5] > s[2] && o[2] < s[5]) {
            if (off > 0.0 && o[3] <= s[0]) {
                double d = s[0] - o[3];
                if (d < off) {
                    off = d;
                }
            } else if (off < 0.0 && o[0] >= s[3]) {
                double d = s[3] - o[0];
                if (d > off) {
                    off = d;
                }
            }
        }
        return off;
    }

    private static double yOffset(double[] s, double[] o, double off) {
        if (o[3] > s[0] && o[0] < s[3] && o[5] > s[2] && o[2] < s[5]) {
            if (off > 0.0 && o[4] <= s[1]) {
                double d = s[1] - o[4];
                if (d < off) {
                    off = d;
                }
            } else if (off < 0.0 && o[1] >= s[4]) {
                double d = s[4] - o[1];
                if (d > off) {
                    off = d;
                }
            }
        }
        return off;
    }

    private static double zOffset(double[] s, double[] o, double off) {
        if (o[3] > s[0] && o[0] < s[3] && o[4] > s[1] && o[1] < s[4]) {
            if (off > 0.0 && o[5] <= s[2]) {
                double d = s[2] - o[5];
                if (d < off) {
                    off = d;
                }
            } else if (off < 0.0 && o[2] >= s[5]) {
                double d = s[5] - o[2];
                if (d > off) {
                    off = d;
                }
            }
        }
        return off;
    }

    /** {@code Entity.doBlockCollisions} for cobwebs. */
    private static boolean inWeb(Blocks blocks, double[] bb) {
        int x0 = Geo.floor(bb[0] + 0.001), x1 = Geo.floor(bb[3] - 0.001);
        int y0 = Geo.floor(bb[1] + 0.001), y1 = Geo.floor(bb[4] - 0.001);
        int z0 = Geo.floor(bb[2] + 0.001), z1 = Geo.floor(bb[5] - 0.001);
        for (int x = x0; x <= x1; x++) {
            for (int y = y0; y <= y1; y++) {
                for (int z = z0; z <= z1; z++) {
                    if (blocks.web(x, y, z)) {
                        return true;
                    }
                }
            }
        }
        return false;
    }

    /** {@code Entity.moveEntity} of a living entity without the sneak's edge stop (body.rs, {@code move_entity}). */
    private void moveEntity(Blocks blocks, double x, double y, double z) {
        if (inWeb) {
            inWeb = false;
            x *= 0.25;
            y *= 0.05000000074505806;
            z *= 0.25;
            motion[0] = 0.0;
            motion[1] = 0.0;
            motion[2] = 0.0;
        }
        double d3 = x, d4 = y, d5 = z;
        double[] bb = {min[0], min[1], min[2], max[0], max[1], max[2]};
        double[] start = bb.clone();
        double[][] list = colliding(blocks, addCoord(bb, x, y, z));
        for (double[] s : list) {
            y = yOffset(s, bb, y);
        }
        bb = offset(bb, 0.0, y, 0.0);
        boolean flag1 = onGround || d4 != y && d4 < 0.0;
        for (double[] s : list) {
            x = xOffset(s, bb, x);
        }
        bb = offset(bb, x, 0.0, 0.0);
        for (double[] s : list) {
            z = zOffset(s, bb, z);
        }
        bb = offset(bb, 0.0, 0.0, z);
        if (STEP_HEIGHT > 0.0 && flag1 && (d3 != x || d5 != z)) {
            double d11 = x, d7 = y, d8 = z;
            double[] bb3 = bb;
            bb = start;
            y = STEP_HEIGHT;
            list = colliding(blocks, addCoord(bb, d3, y, d5));
            double[] bb4 = bb;
            double[] bb5 = addCoord(bb4, d3, 0.0, d5);
            double d9 = y;
            for (double[] s : list) {
                d9 = yOffset(s, bb5, d9);
            }
            bb4 = offset(bb4, 0.0, d9, 0.0);
            double d15 = d3;
            for (double[] s : list) {
                d15 = xOffset(s, bb4, d15);
            }
            bb4 = offset(bb4, d15, 0.0, 0.0);
            double d16 = d5;
            for (double[] s : list) {
                d16 = zOffset(s, bb4, d16);
            }
            bb4 = offset(bb4, 0.0, 0.0, d16);
            double[] bb14 = bb;
            double d17 = y;
            for (double[] s : list) {
                d17 = yOffset(s, bb14, d17);
            }
            bb14 = offset(bb14, 0.0, d17, 0.0);
            double d18 = d3;
            for (double[] s : list) {
                d18 = xOffset(s, bb14, d18);
            }
            bb14 = offset(bb14, d18, 0.0, 0.0);
            double d19 = d5;
            for (double[] s : list) {
                d19 = zOffset(s, bb14, d19);
            }
            bb14 = offset(bb14, 0.0, 0.0, d19);
            double d20 = d15 * d15 + d16 * d16;
            double d10 = d18 * d18 + d19 * d19;
            if (d20 > d10) {
                x = d15;
                z = d16;
                y = -d9;
                bb = bb4;
            } else {
                x = d18;
                z = d19;
                y = -d17;
                bb = bb14;
            }
            for (double[] s : list) {
                y = yOffset(s, bb, y);
            }
            bb = offset(bb, 0.0, y, 0.0);
            if (d11 * d11 + d8 * d8 >= x * x + z * z) {
                x = d11;
                y = d7;
                z = d8;
                bb = bb3;
            }
        }
        min[0] = bb[0];
        min[1] = bb[1];
        min[2] = bb[2];
        max[0] = bb[3];
        max[1] = bb[4];
        max[2] = bb[5];
        pos[0] = (bb[0] + bb[3]) / 2.0;
        pos[1] = bb[1];
        pos[2] = (bb[2] + bb[5]) / 2.0;
        collidedH = d3 != x || d5 != z;
        collidedV = d4 != y;
        onGround = collidedV && d4 < 0.0;
        if (onGround) {
            fallDistance = 0.0f;
        } else if (y < 0.0) {
            fallDistance = (float) ((double) fallDistance - y);
        }
        if (d3 != x) {
            motion[0] = 0.0;
        }
        if (d5 != z) {
            motion[2] = 0.0;
        }
        if (d4 != y) {
            motion[1] = 0.0;
        }
        if (inWeb(blocks, bb)) {
            inWeb = true;
            fallDistance = 0.0f;
        }
    }
}
