package pvpai.plugin;

import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.Locale;
import java.util.Map;
import java.util.Random;
import java.util.UUID;
import org.bukkit.Bukkit;
import org.bukkit.ChatColor;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.command.Command;
import org.bukkit.command.CommandSender;
import org.bukkit.configuration.ConfigurationSection;
import org.bukkit.enchantments.Enchantment;
import org.bukkit.entity.EnderPearl;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.block.Action;
import org.bukkit.event.entity.EntityDamageByEntityEvent;
import org.bukkit.event.entity.EntityDamageEvent;
import org.bukkit.event.entity.ProjectileLaunchEvent;
import org.bukkit.event.player.PlayerInteractEvent;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.event.player.PlayerMoveEvent;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.event.player.PlayerRespawnEvent;
import org.bukkit.event.player.PlayerTeleportEvent;
import org.bukkit.event.player.PlayerToggleSprintEvent;
import org.bukkit.event.player.PlayerVelocityEvent;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.PlayerInventory;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.plugin.java.JavaPlugin;
import org.bukkit.potion.PotionEffect;
import org.bukkit.potion.PotionEffectType;
import org.bukkit.util.Vector;

/**
 * A test server's knockback: profiles of the two kinds the simulator
 * trains with, switched by command for everybody or for one player, the
 * wait between ender pearls, and the kit an agent's hotbar is laid out
 * for. The velocity of a hit is put in place of the game's in the event
 * the server raises before it sends it; nothing else of a hit is touched.
 */
public final class KbTest extends JavaPlugin implements Listener {
    /** What is kept about a player. */
    private static final class State {
        /** Where it last stood and whether it went over the height limit since. */
        double lastGroundY;
        boolean touchedTop;
        /** Whether it sprints, as its client last said; a hit does not end that. */
        boolean clientSprint;
        /** The tick from which it can throw a pearl again, and the tick it was last told to wait. */
        long pearlReady, pearlTold = -1;
        /** A profile of its own, for the hits it takes. */
        Profile own;
        /** The hit whose velocity the server is about to send. */
        Hit pending;
    }

    /** A hit as it was when the server worked out its damage. */
    private static final class Hit {
        long tick;
        double ax, az, vx, vy, vz;
        float yaw;
        boolean ground, serverSprint, clientSprint;
        int ench;
        Profile profile;
    }

    private static final Profile GAME = new Profile();
    private final Map<String, Profile> profiles = new LinkedHashMap<String, Profile>();
    private final Map<UUID, State> states = new HashMap<UUID, State>();
    private final Random random = new Random();
    private Profile current = new Profile();
    private boolean verify, logHits;
    private int pearlCooldown;
    /** Server ticks, counted after the packets of each. */
    private long now;
    private long verified, differed, replaced;

    @Override
    public void onEnable() {
        saveDefaultConfig();
        load();
        Bukkit.getPluginManager().registerEvents(this, this);
        Bukkit.getScheduler().runTaskTimer(this, new Runnable() {
            public void run() {
                now++;
            }
        }, 1L, 1L);
        for (Player p : Bukkit.getOnlinePlayers()) {
            enter(p);
        }
    }

    private void load() {
        reloadConfig();
        profiles.clear();
        Profile vanilla = new Profile();
        vanilla.name = "vanilla";
        profiles.put(vanilla.name, vanilla);
        ConfigurationSection all = getConfig().getConfigurationSection("profiles");
        if (all != null) {
            for (String name : all.getKeys(false)) {
                ConfigurationSection s = all.getConfigurationSection(name);
                if (s == null) {
                    continue;
                }
                Profile p = new Profile();
                p.name = name.toLowerCase(Locale.ROOT);
                String kind = s.getString("kind", "vanilla");
                p.kind = kind.equalsIgnoreCase("game") ? Profile.GAME : kind.equalsIgnoreCase("hit") ? Profile.HIT : Profile.VANILLA;
                for (String key : s.getKeys(false)) {
                    if (!key.equals("kind")) {
                        set(p, key, s.getString(key));
                    }
                }
                profiles.put(p.name, p);
            }
        }
        Profile chosen = profiles.get(getConfig().getString("profile", "vanilla").toLowerCase(Locale.ROOT));
        current = chosen != null ? chosen : vanilla;
        verify = getConfig().getBoolean("verify", false);
        logHits = getConfig().getBoolean("log-hits", false);
        pearlCooldown = (int) Math.round(getConfig().getDouble("pearl-cooldown", 16.0) * 20.0);
    }

    /** Set one number of a profile by its name in the configuration. False if there is no such number or it cannot be read. */
    private static boolean set(Profile p, String key, String value) {
        try {
            if (key.equals("stop-sprint")) {
                p.stopSprint = value.equalsIgnoreCase("true") || value.equals("1");
                return true;
            }
            double x = Double.parseDouble(value);
            if (Double.isNaN(x) || Double.isInfinite(x)) {
                return false;
            }
            if (key.equals("hit-delay")) {
                p.hitDelay = (int) Math.max(0L, Math.min(100L, Math.round(x)));
            } else if (key.equals("friction")) {
                if (x == 0.0) {
                    return false;
                }
                p.friction = x;
            } else if (key.equals("horizontal")) {
                p.horizontal = x;
            } else if (key.equals("vertical")) {
                p.vertical = x;
            } else if (key.equals("vertical-limit")) {
                p.verticalLimit = x;
            } else if (key.equals("extra-horizontal")) {
                p.extraHorizontal = (float) x;
            } else if (key.equals("extra-vertical")) {
                p.extraVertical = x;
            } else if (key.equals("ground-horizontal")) {
                p.hGround = x;
            } else if (key.equals("air-horizontal")) {
                p.hAir = x;
            } else if (key.equals("sprint-horizontal")) {
                p.hSprint = x;
            } else if (key.equals("ground-vertical")) {
                p.vGround = x;
            } else if (key.equals("air-vertical")) {
                p.vAir = x;
            } else if (key.equals("sprint-vertical")) {
                p.vSprint = x;
            } else if (key.equals("height-limit")) {
                p.yLimit = x;
            } else if (key.equals("vertical-after-limit")) {
                p.vAfterLimit = x;
            } else {
                return false;
            }
            return true;
        } catch (NumberFormatException e) {
            return false;
        }
    }

    private static String show(Profile p) {
        if (p.kind == Profile.GAME) {
            return String.format(Locale.ROOT, "%s (game): friction %.3f, horizontal %.3f, vertical %.3f, vertical-limit %.3f, extra-horizontal %.3f, extra-vertical %.3f, hit-delay %d",
                p.name, p.friction, p.horizontal, p.vertical, p.verticalLimit, p.extraHorizontal, p.extraVertical, p.hitDelay);
        }
        if (p.kind == Profile.HIT) {
            return String.format(Locale.ROOT, "%s (hit): ground %.3f / %.3f, air %.3f / %.3f, sprint %.3f / %.3f, stop-sprint %s, height-limit %.3f, vertical-after-limit %.3f, hit-delay %d",
                p.name, p.hGround, p.vGround, p.hAir, p.vAir, p.hSprint, p.vSprint, p.stopSprint, p.yLimit, p.vAfterLimit, p.hitDelay);
        }
        return p.name + " (vanilla): el del juego, hit-delay " + p.hitDelay;
    }

    /** A profile drawn from the ranges the agents are trained with. */
    private Profile drawn() {
        Profile p = new Profile();
        p.name = "azar";
        if (random.nextBoolean()) {
            p.kind = Profile.HIT;
            p.hGround = between(0.28, 0.46);
            p.hAir = p.hGround * between(0.8, 1.1);
            p.hSprint = between(0.3, 0.6);
            p.vGround = between(0.30, 0.40);
            p.vAir = p.vGround * between(0.6, 1.0);
            p.vSprint = between(0.0, 0.12);
            p.stopSprint = random.nextDouble() < 0.7;
            p.yLimit = random.nextDouble() < 0.6 ? between(0.5, 1.2) : -1.0;
            p.vAfterLimit = between(-0.15, 0.0);
        } else {
            p.kind = Profile.GAME;
            p.friction = between(1.6, 2.4);
            p.horizontal = between(0.3, 0.5);
            p.vertical = between(0.3, 0.45);
            p.verticalLimit = Math.max(p.vertical, GAME.verticalLimit);
            p.extraHorizontal = (float) between(0.3, 0.6);
            p.extraVertical = between(0.05, 0.12);
        }
        p.hitDelay = random.nextDouble() < 0.25 ? 16 + random.nextInt(4) : 20;
        return p;
    }

    private double between(double lo, double hi) {
        return lo + random.nextDouble() * (hi - lo);
    }

    private State state(Player p) {
        State s = states.get(p.getUniqueId());
        if (s == null) {
            s = new State();
            s.lastGroundY = p.getLocation().getY();
            s.clientSprint = p.isSprinting();
            states.put(p.getUniqueId(), s);
        }
        return s;
    }

    private Profile profileOf(Player p) {
        State s = state(p);
        return s.own != null ? s.own : current;
    }

    private void enter(Player p) {
        p.setMaximumNoDamageTicks(profileOf(p).hitDelay);
    }

    private void enterAll() {
        for (Player p : Bukkit.getOnlinePlayers()) {
            enter(p);
        }
    }

    @EventHandler
    public void onJoin(PlayerJoinEvent e) {
        states.remove(e.getPlayer().getUniqueId());
        enter(e.getPlayer());
    }

    @EventHandler
    public void onQuit(PlayerQuitEvent e) {
        states.remove(e.getPlayer().getUniqueId());
    }

    @EventHandler
    public void onRespawn(PlayerRespawnEvent e) {
        final Player p = e.getPlayer();
        // The player is a new one to the server once it is back.
        Bukkit.getScheduler().runTask(this, new Runnable() {
            public void run() {
                if (p.isOnline()) {
                    enter(p);
                }
            }
        });
    }

    @EventHandler
    public void onSprint(PlayerToggleSprintEvent e) {
        state(e.getPlayer()).clientSprint = e.isSprinting();
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onMove(PlayerMoveEvent e) {
        // The player as its last report left it, before the one that is being handled.
        Player p = e.getPlayer();
        if (p.isOnGround()) {
            State s = state(p);
            s.lastGroundY = p.getLocation().getY();
            s.touchedTop = false;
        }
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onTeleport(PlayerTeleportEvent e) {
        state(e.getPlayer()).lastGroundY = e.getTo().getY();
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onDamage(EntityDamageByEntityEvent e) {
        if (e.getCause() != EntityDamageEvent.DamageCause.ENTITY_ATTACK || !(e.getEntity() instanceof Player) || !(e.getDamager() instanceof Player)) {
            return;
        }
        Player v = (Player) e.getEntity(), a = (Player) e.getDamager();
        State s = state(v);
        s.pending = null;
        // A hit in the time a player cannot be hit again knocks nobody back.
        if ((float) v.getNoDamageTicks() > (float) v.getMaximumNoDamageTicks() / 2.0F) {
            return;
        }
        Location al = a.getLocation(), vl = v.getLocation();
        Hit h = new Hit();
        h.tick = now;
        h.ax = al.getX();
        h.az = al.getZ();
        h.yaw = al.getYaw();
        h.vx = vl.getX();
        h.vy = vl.getY();
        h.vz = vl.getZ();
        h.ground = v.isOnGround();
        h.serverSprint = a.isSprinting();
        h.clientSprint = state(a).clientSprint;
        ItemStack held = a.getItemInHand();
        h.ench = held == null ? 0 : held.getEnchantmentLevel(Enchantment.KNOCKBACK);
        h.profile = profileOf(v);
        if (h.ground) {
            s.lastGroundY = h.vy;
            s.touchedTop = false;
        }
        s.pending = h;
    }

    @EventHandler(priority = EventPriority.HIGHEST, ignoreCancelled = true)
    public void onVelocity(PlayerVelocityEvent e) {
        Player v = e.getPlayer();
        State s = states.get(v.getUniqueId());
        if (s == null || s.pending == null) {
            return;
        }
        Hit h = s.pending;
        s.pending = null;
        double dx = h.ax - h.vx, dz = h.az - h.vz;
        // Two players on the very same spot: the game draws a direction, and that is left as it is.
        if (h.tick != now || dx * dx + dz * dz < 1.0E-4) {
            return;
        }
        Profile p = h.profile;
        // The event carries the motion the victim had before the hit; the victim itself has the game's answer by now.
        Vector before = e.getVelocity();
        double[] motion = {before.getX(), before.getY(), before.getZ()};
        double[] out;
        int level;
        if (p.kind == Profile.HIT) {
            level = h.ench + ((p.stopSprint ? h.serverSprint : h.clientSprint) ? 1 : 0);
            boolean[] top = {s.touchedTop};
            out = Knock.hit(p, h.ax, h.az, h.vx, h.vy, h.vz, h.ground, h.yaw, level, s.lastGroundY, top);
            s.touchedTop = top[0];
        } else {
            level = h.ench + (h.serverSprint ? 1 : 0);
            out = Knock.game(p.kind == Profile.GAME ? p : GAME, motion, h.ax, h.az, h.vx, h.vz, h.yaw, level);
        }
        if (p.kind == Profile.VANILLA) {
            if (verify) {
                // The same arithmetic with the game's own numbers has to give the game's own answer.
                Vector game = v.getVelocity();
                boolean same = game.getX() == out[0] && game.getY() == out[1] && game.getZ() == out[2];
                verified++;
                differed += same ? 0 : 1;
                if (!same || logHits) {
                    getLogger().info(String.format(Locale.ROOT, "KBVERIFY %s %s level %d to %s game %s %s %s plugin %s %s %s", same ? "same" : "DIFFERS",
                        h.ground ? "ground" : "air", level, v.getName(), game.getX(), game.getY(), game.getZ(), out[0], out[1], out[2]));
                }
            }
            return;
        }
        Vector vel = new Vector(out[0], out[1], out[2]);
        e.setVelocity(vel);
        // The server takes the event's velocity only if it is not the one it put in; this way it always does.
        v.setVelocity(vel);
        replaced++;
        if (logHits) {
            int[] sent = Knock.sent(out);
            getLogger().info(String.format(Locale.ROOT, "KBHIT %s level %d to %s profile %s sent %d %d %d", h.ground ? "ground" : "air", level, v.getName(), p.name, sent[0], sent[1], sent[2]));
        }
    }

    @EventHandler(priority = EventPriority.HIGH)
    public void onInteract(PlayerInteractEvent e) {
        if (pearlCooldown <= 0 || (e.getAction() != Action.RIGHT_CLICK_AIR && e.getAction() != Action.RIGHT_CLICK_BLOCK)) {
            return;
        }
        ItemStack item = e.getItem();
        if (item == null || item.getType() != Material.ENDER_PEARL) {
            return;
        }
        Player p = e.getPlayer();
        State s = state(p);
        if (now < s.pearlReady) {
            e.setCancelled(true);
            // The client has taken the pearl out of its hand already: it gets its inventory again.
            p.updateInventory();
            if (s.pearlTold != now) {
                s.pearlTold = now;
                p.sendMessage(ChatColor.RED + String.format(Locale.ROOT, "Perla en %.1f s", (double) (s.pearlReady - now) / 20.0));
            }
        }
    }

    /** The wait starts when a pearl is really thrown: a click can come to nothing for other reasons. */
    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onLaunch(ProjectileLaunchEvent e) {
        if (pearlCooldown > 0 && e.getEntity() instanceof EnderPearl && e.getEntity().getShooter() instanceof Player) {
            state((Player) e.getEntity().getShooter()).pearlReady = now + (long) pearlCooldown;
        }
    }

    // Commands.

    @Override
    public boolean onCommand(CommandSender sender, Command command, String label, String[] args) {
        if (command.getName().equalsIgnoreCase("kit")) {
            Player to = args.length > 0 ? Bukkit.getPlayerExact(args[0]) : sender instanceof Player ? (Player) sender : null;
            if (to == null) {
                sender.sendMessage(ChatColor.RED + "Uso: /kit [jugador]");
                return true;
            }
            kit(to);
            sender.sendMessage(ChatColor.GREEN + "Kit para " + to.getName() + ".");
            return true;
        }
        String sub = args.length > 0 ? args[0].toLowerCase(Locale.ROOT) : "ver";
        if (sub.equals("ver")) {
            sender.sendMessage(ChatColor.YELLOW + "KB de todos: " + show(current));
            for (Player p : Bukkit.getOnlinePlayers()) {
                State s = states.get(p.getUniqueId());
                if (s != null && s.own != null) {
                    sender.sendMessage(ChatColor.YELLOW + "KB que recibe " + p.getName() + ": " + show(s.own));
                }
            }
            sender.sendMessage(ChatColor.YELLOW + String.format(Locale.ROOT, "Espera de perlas %.1f s. Golpes cambiados %d. Verificados %d, distintos %d.",
                (double) pearlCooldown / 20.0, replaced, verified, differed));
        } else if (sub.equals("list")) {
            for (Profile p : profiles.values()) {
                sender.sendMessage(ChatColor.YELLOW + show(p));
            }
        } else if (sub.equals("use") && args.length >= 2) {
            Profile p = args[1].equalsIgnoreCase("azar") ? drawn() : profiles.get(args[1].toLowerCase(Locale.ROOT));
            if (p == null) {
                sender.sendMessage(ChatColor.RED + "No hay un perfil " + args[1] + ". /kb list los muestra.");
                return true;
            }
            if (args.length >= 3) {
                Player to = Bukkit.getPlayerExact(args[2]);
                if (to == null) {
                    sender.sendMessage(ChatColor.RED + "No hay un jugador " + args[2] + ".");
                    return true;
                }
                state(to).own = p;
                enter(to);
                sender.sendMessage(ChatColor.GREEN + "KB que recibe " + to.getName() + ": " + show(p));
            } else {
                current = p;
                enterAll();
                sender.sendMessage(ChatColor.GREEN + "KB de todos: " + show(p));
            }
        } else if (sub.equals("clear") && args.length >= 2) {
            Player to = Bukkit.getPlayerExact(args[1]);
            if (to != null) {
                state(to).own = null;
                enter(to);
            }
            sender.sendMessage(ChatColor.GREEN + args[1] + " vuelve al KB de todos.");
        } else if (sub.equals("set") && args.length >= 3) {
            if (current.kind == Profile.VANILLA || !set(current, args[1].toLowerCase(Locale.ROOT), args[2])) {
                sender.sendMessage(ChatColor.RED + "No se puede poner " + args[1] + " en " + args[2] + " en este perfil.");
                return true;
            }
            enterAll();
            sender.sendMessage(ChatColor.GREEN + show(current));
        } else if (sub.equals("pearl") && args.length >= 2) {
            try {
                pearlCooldown = (int) Math.max(0L, Math.round(Double.parseDouble(args[1]) * 20.0));
            } catch (NumberFormatException ex) {
                sender.sendMessage(ChatColor.RED + "Uso: /kb pearl SEGUNDOS");
                return true;
            }
            sender.sendMessage(ChatColor.GREEN + String.format(Locale.ROOT, "Espera de perlas: %.1f s.", (double) pearlCooldown / 20.0));
        } else if (sub.equals("verify") && args.length >= 2) {
            verify = args[1].equalsIgnoreCase("on");
            sender.sendMessage(ChatColor.GREEN + "Verificar el KB del juego: " + (verify ? "si" : "no") + ".");
        } else if (sub.equals("log") && args.length >= 2) {
            logHits = args[1].equalsIgnoreCase("on");
            sender.sendMessage(ChatColor.GREEN + "Anotar cada golpe en la consola: " + (logHits ? "si" : "no") + ".");
        } else if (sub.equals("reload")) {
            load();
            for (State s : states.values()) {
                s.own = null;
            }
            enterAll();
            sender.sendMessage(ChatColor.GREEN + "Configuracion leida. KB de todos: " + show(current));
        } else {
            sender.sendMessage(ChatColor.YELLOW + "/kb ver | list | use PERFIL [JUGADOR] | use azar | clear JUGADOR | set CAMPO VALOR | pearl SEGUNDOS | verify on|off | log on|off | reload");
        }
        return true;
    }

    // The kit.

    private ItemStack enchanted(Material m, Enchantment e, int level, boolean unbreakable) {
        ItemStack s = new ItemStack(m);
        if (level > 0) {
            s.addUnsafeEnchantment(e, level);
        }
        if (unbreakable) {
            ItemMeta meta = s.getItemMeta();
            meta.spigot().setUnbreakable(true);
            s.setItemMeta(meta);
        }
        return s;
    }

    /** Sword first, pearls second, healing potions, Speed to drink, steak last; more potions in the rows. */
    private void kit(Player p) {
        ConfigurationSection k = getConfig().getConfigurationSection("kit");
        if (k == null) {
            k = getConfig().createSection("kit");
        }
        boolean unbreakable = k.getBoolean("unbreakable", true);
        int prot = k.getInt("protection", 2);
        PlayerInventory inv = p.getInventory();
        inv.clear();
        inv.setHelmet(enchanted(Material.DIAMOND_HELMET, Enchantment.PROTECTION_ENVIRONMENTAL, prot, unbreakable));
        inv.setChestplate(enchanted(Material.DIAMOND_CHESTPLATE, Enchantment.PROTECTION_ENVIRONMENTAL, prot, unbreakable));
        inv.setLeggings(enchanted(Material.DIAMOND_LEGGINGS, Enchantment.PROTECTION_ENVIRONMENTAL, prot, unbreakable));
        ItemStack boots = enchanted(Material.DIAMOND_BOOTS, Enchantment.PROTECTION_ENVIRONMENTAL, prot, unbreakable);
        int feather = k.getInt("feather-falling", 4);
        if (feather > 0) {
            boots.addUnsafeEnchantment(Enchantment.PROTECTION_FALL, feather);
        }
        inv.setBoots(boots);
        inv.setItem(0, enchanted(Material.DIAMOND_SWORD, Enchantment.DAMAGE_ALL, k.getInt("sharpness", 2), unbreakable));
        int pearls = k.getInt("pearls", 16);
        if (pearls > 0) {
            inv.setItem(1, new ItemStack(Material.ENDER_PEARL, Math.min(pearls, 16)));
        }
        // Splash potion of healing II, and Speed II for a minute and a half to drink.
        int slot = 2;
        for (int i = 0; i < k.getInt("hotbar-potions", 5) && slot < 8; i++) {
            inv.setItem(slot++, new ItemStack(Material.POTION, 1, (short) 16421));
        }
        for (int i = 0; i < k.getInt("hotbar-speed", 1) && slot < 8; i++) {
            inv.setItem(slot++, new ItemStack(Material.POTION, 1, (short) 8226));
        }
        int steak = k.getInt("steak", 64);
        if (steak > 0) {
            inv.setItem(8, new ItemStack(Material.COOKED_BEEF, Math.min(steak, 64)));
        }
        slot = 9;
        for (int i = 0; i < k.getInt("inventory-potions", 12) && slot < 36; i++) {
            inv.setItem(slot++, new ItemStack(Material.POTION, 1, (short) 16421));
        }
        for (int i = 0; i < k.getInt("inventory-speed", 2) && slot < 36; i++) {
            inv.setItem(slot++, new ItemStack(Material.POTION, 1, (short) 8226));
        }
        for (PotionEffect e : p.getActivePotionEffects()) {
            p.removePotionEffect(e.getType());
        }
        int speed = k.getInt("speed-seconds", 90);
        if (speed > 0) {
            p.addPotionEffect(new PotionEffect(PotionEffectType.SPEED, speed * 20, 1), true);
        }
        p.setHealth(p.getMaxHealth());
        p.setFoodLevel(20);
        p.setSaturation(5.0F);
        p.setFireTicks(0);
        p.updateInventory();
    }
}
