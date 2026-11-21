package chestai.client;

import java.io.File;
import java.io.IOException;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.function.BooleanSupplier;
import java.util.function.Predicate;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.settings.KeyBinding;
import net.minecraft.entity.Entity;
import net.minecraft.entity.EntityLivingBase;
import net.minecraft.entity.item.EntityArmorStand;
import net.minecraft.entity.passive.EntityHorse;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.item.ItemStack;
import net.minecraft.util.MovingObjectPosition;
import chestai.core.Pilot;

/**
 * The farmer's blows through Minecadia's SilentAim (VapeSilentAim), reached
 * by reflection: while the farmer is on, SilentAim aims at the one mob the
 * farmer fights, and the left button is pressed only when the view SilentAim
 * sends the server is on that mob, within reach, with a weapon in the hand.
 * The modules that would fight on their own are turned off meanwhile, and
 * everything SilentAim had (its values, its hooks, which modules were on) is
 * put back as it was when the farmer is turned off.
 */
final class VapeCombat {
    private static final String CLIENT = "net.minecadia.client.";
    private static final String MODULES = CLIENT + "features.module.modules.";
    private static final boolean DEBUG = Boolean.getBoolean("chestai.debug");

    private final Minecraft mc;
    private final File config;
    private final Reader reader;
    private final Object aim, rotation;
    private final Method getState, setState, getValue, readValue, writeValue;
    private final Method enable, disable, reset, getTarget;
    private final Method getController, setController, rayTrace, reportedRay;
    /** The modules turned off for the farmer (SilentAim too), and whether each was on. */
    private final Map<Object, Boolean> previousModules = new LinkedHashMap<Object, Boolean>();
    /** SilentAim's values the farmer set, and what each was before. */
    private final Map<Object, Object> previousValues = new LinkedHashMap<Object, Object>();
    /** SilentAim's hooks for Chest AI: which mobs it may aim at, and whether it may hit now. */
    private final Field targetFilter, attackGate;
    private final Object previousFilter, previousAttackGate;
    /** The mob SilentAim is aiming at, or null. */
    private EntityLivingBase target;
    private boolean aiming;
    private double lastAttackTick = Double.NEGATIVE_INFINITY;
    /** Ticks between blows, at the farmer's clicks per second. */
    private final double ticksPerAttack;
    /** Whether the profile leaves SilentAim on (an angle over 0). */
    private boolean aimEnabled;

    VapeCombat(Minecraft mc, double cps, File config, Reader reader) throws ReflectiveOperationException {
        this.mc = mc;
        this.config = config;
        this.reader = reader;
        this.ticksPerAttack = 20.0 / cps;
        Object modules = Class.forName(CLIENT + "LiquidBounce").getField("moduleManager").get(null);
        if (modules == null) {
            throw new IllegalStateException("Minecadia todavía no cargó sus módulos");
        }
        Method getModule = modules.getClass().getMethod("getModule", Class.class);
        Class<?> module = Class.forName(CLIENT + "features.module.Module");
        Class<?> value = Class.forName(CLIENT + "value.Value");
        Class<?> silentAim = Class.forName(MODULES + "vape.VapeSilentAim");
        Class<?> rotationManager = Class.forName(MODULES + "vape.VapeRotationManager");
        aim = getModule.invoke(modules, silentAim);
        if (aim == null) {
            throw new IllegalStateException("SilentAim no está registrado en Minecadia");
        }
        rotation = rotationManager.getField("INSTANCE").get(null);
        getState = module.getMethod("getState");
        setState = module.getMethod("setState", boolean.class);
        getValue = module.getMethod("getValue", String.class);
        readValue = value.getMethod("get");
        writeValue = value.getMethod("set", Object.class);
        enable = silentAim.getMethod("onEnable");
        disable = silentAim.getMethod("onDisable");
        reset = silentAim.getMethod("resetTargeting");
        getTarget = silentAim.getMethod("getTarget");
        getController = rotationManager.getMethod("getActiveController");
        setController = rotationManager.getMethod("setController", Class.forName(MODULES + "vape.VapeRotation$Controller"));
        rayTrace = rotationManager.getMethod("rayTraceUsingManagedRotation", double.class, float.class, boolean.class);
        reportedRay = silentAim.getMethod("serverRayTarget", EntityPlayerSP.class, Predicate.class);
        targetFilter = silentAim.getField("chestAiTargetFilter");
        attackGate = silentAim.getField("chestAiAttackGate");
        previousFilter = targetFilter.get(null);
        previousAttackGate = attackGate.get(null);
        // The modules that would hit, click or farm on their own.
        for (String name : new String[] {"automation.AutoFarm", "vape.VapeSilentAura", "vape.VapeAutoClicker",
                "combat.TriggerBot", "combat.HitSelect", "combat.AutoPunchShot"}) {
            Object other = getModule.invoke(modules, Class.forName(MODULES + name));
            if (other != null) {
                previousModules.put(other, (Boolean) getState.invoke(other));
            }
        }
        previousModules.put(aim, (Boolean) getState.invoke(aim));
    }

    /**
     * Sets SilentAim up for the farmer: the other modules off, mobs only
     * (never players), the nearest one in sight, always on with no button
     * held, and the profile's values; it aims only at the farmer's mob, and
     * hits only when {@code attackGate} says so.
     */
    void open(BooleanSupplier gate) throws ReflectiveOperationException, IOException {
        for (Object module : previousModules.keySet()) {
            setState.invoke(module, false);
        }
        configure("Players", false);
        configure("Mobs", true);
        configure("Peaceful", true);
        configure("Ignore invisible", true);
        configure("Ignore behind walls", true);
        configure("Target mode", "Distance");
        configure("Switch", false);
        configure("Activation", "Always");
        configure("Require mouse down", false);
        configure("Melee weapon", false);
        configure("Limit to items", false);
        configure("Disable on death", false);
        reloadProfile();
        Predicate<EntityLivingBase> onlyTarget = e -> e == target;
        targetFilter.set(null, onlyTarget);
        attackGate.set(null, gate);
        clearAim();
    }

    /** Reads silentaim.properties again and sets its values on SilentAim. */
    void reloadProfile() throws ReflectiveOperationException, IOException {
        SilentAimProfile profile = new SilentAimProfile(config);
        aimEnabled = ((Number) profile.get(SilentAimProfile.Option.ANGLE)).intValue() > 0;
        for (SilentAimProfile.Option option : SilentAimProfile.Option.values()) {
            configure(option.moduleValue, profile.get(option));
        }
    }

    /** Sets SilentAim's value {@code name}, keeping what it was before the first change. */
    private void configure(String name, Object v) throws ReflectiveOperationException {
        Object value = getValue.invoke(aim, name);
        if (value == null) {
            throw new NoSuchFieldException("SilentAim: " + name);
        }
        if (!previousValues.containsKey(value)) {
            previousValues.put(value, readValue.invoke(value));
        }
        writeValue.invoke(value, v);
    }

    /**
     * Points SilentAim at the mob {@code mobId} for this tick; false (and
     * SilentAim let go) with no mob to fight, the sword not in the hand, the
     * right button busy (an apple, a pearl, a potion, a chest), or the
     * profile leaving SilentAim off. The rider is the one fought on a horse.
     */
    boolean prepare(int mobId, Pilot.Keys k, Layout lay) throws ReflectiveOperationException {
        if (!aimEnabled || mobId < 0 || k.slot != Pilot.SWORD || lay.real[Pilot.SWORD] < 0 || k.useHeld || k.throwIt || k.claim
                || mc.thePlayer.isUsingItem()) {
            suspend();
            return false;
        }
        Entity e = mc.theWorld.getEntityByID(mobId);
        if (e instanceof EntityHorse && e.riddenByEntity instanceof EntityLivingBase) {
            e = e.riddenByEntity;
        }
        if (!(e instanceof EntityLivingBase) || e instanceof EntityArmorStand || e instanceof EntityPlayer
                || Reader.excludedFromCombat(e) || reader.isChestMarker(e) || e.isDead) {
            suspend();
            return false;
        }
        target = (EntityLivingBase) e;
        if (!aiming) {
            enable.invoke(aim);
            aiming = true;
        }
        return true;
    }

    /**
     * The blow of the tick {@code tick}: the left button pressed, through the
     * game's own key handling, only when the clicks allow it again, the window
     * has the focus, a weapon is in the hand, SilentAim aims at the farmer's
     * mob, it is in sight, the managed view's ray and the one the server is
     * told about both get to it within 3 blocks, and the crosshair the game
     * works out is on it. The mob hit, or null.
     */
    EntityLivingBase queueAttack(long tick) throws ReflectiveOperationException {
        if (!aiming || target == null || mc.thePlayer == null || mc.theWorld == null || mc.currentScreen != null
                || mc.playerController == null || tick - lastAttackTick < ticksPerAttack || mc.thePlayer.isUsingItem()) {
            return null;
        }
        if (!mc.inGameHasFocus) {
            return blocked(tick, "window");
        }
        ItemStack held = mc.thePlayer.getHeldItem();
        if (!Layout.isMeleeWeapon(held)) {
            return blocked(tick, "weapon");
        }
        if (target != getTarget.invoke(aim)) {
            return blocked(tick, "aim target");
        }
        if (!isCurrentTarget(target)) {
            return blocked(tick, "visibility");
        }
        MovingObjectPosition ray = (MovingObjectPosition) rayTrace.invoke(rotation, 3.0, 0.0F, false);
        if (ray == null || ray.entityHit != target || ray.hitVec.distanceTo(mc.thePlayer.getPositionEyes(1.0F)) > 3.0) {
            return blocked(tick, "aim ray");
        }
        Predicate<EntityLivingBase> onlyTarget = e -> e == target;
        if (reportedRay.invoke(null, mc.thePlayer, onlyTarget) != target) {
            return blocked(tick, "reported ray");
        }
        mc.entityRenderer.getMouseOver(1.0F);
        if (!allowNativeAttack(target)) {
            return blocked(tick, "mouse over");
        }
        KeyBinding.onTick(mc.gameSettings.keyBindAttack.getKeyCode());
        lastAttackTick = tick;
        return target;
    }

    /** Whether the game's own blow, from where the crosshair is, lands on the farmer's mob {@code e} within 3 blocks. */
    boolean allowNativeAttack(EntityLivingBase e) {
        MovingObjectPosition over = mc.objectMouseOver;
        return aiming && target == e && mc.thePlayer != null && mc.theWorld != null && mc.currentScreen == null
            && !mc.thePlayer.isUsingItem() && isCurrentTarget(e)
            && over != null && over.typeOfHit == MovingObjectPosition.MovingObjectType.ENTITY && over.entityHit == e
            && over.hitVec != null && over.hitVec.distanceTo(mc.thePlayer.getPositionEyes(1.0F)) <= 3.0;
    }

    /** No blow, and why (once a second, with chestai.debug). */
    private EntityLivingBase blocked(long tick, String why) {
        if (DEBUG && tick % 20 == 0) {
            System.out.println("CHESTAI aim t=" + tick + " target=" + target.getEntityId() + " blocked=" + why);
        }
        return null;
    }

    /** Alive, a mob of the fight (no chest's marker), still in the world and in sight. */
    private boolean isCurrentTarget(EntityLivingBase e) {
        return !e.isDead && !Reader.excludedFromCombat(e) && !reader.isChestMarker(e) && e.getHealth() > 0
            && mc.theWorld.getEntityByID(e.getEntityId()) == e && mc.thePlayer.canEntityBeSeen(e);
    }

    /** SilentAim let go, until the next mob. */
    void suspend() throws ReflectiveOperationException {
        if (aiming) {
            clearAim();
        }
    }

    private void clearAim() throws ReflectiveOperationException {
        target = null;
        reset.invoke(aim);
        // The rotation manager let go too, if SilentAim's controller has it.
        Object controller = getController.invoke(rotation);
        if (controller != null && controller.getClass().getEnclosingClass() == aim.getClass()) {
            setController.invoke(rotation, (Object) null);
            reset.invoke(aim);
        }
        disable.invoke(aim);
        aiming = false;
    }

    /** Everything back as it was before {@link #open}, as far as it goes. */
    void close() {
        try {
            clearAim();
        } catch (ReflectiveOperationException e) {
            e.printStackTrace();
        }
        try {
            targetFilter.set(null, previousFilter);
            attackGate.set(null, previousAttackGate);
        } catch (ReflectiveOperationException e) {
            e.printStackTrace();
        }
        for (Map.Entry<Object, Object> value : previousValues.entrySet()) {
            try {
                writeValue.invoke(value.getKey(), value.getValue());
            } catch (ReflectiveOperationException e) {
                e.printStackTrace();
            }
        }
        for (Map.Entry<Object, Boolean> module : previousModules.entrySet()) {
            try {
                setState.invoke(module.getKey(), module.getValue());
            } catch (ReflectiveOperationException e) {
                e.printStackTrace();
            }
        }
    }
}
