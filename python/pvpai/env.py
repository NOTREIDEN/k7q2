"""ctypes wrapper around the Rust simulator (libpvpsim)."""
import ctypes
import os
import numpy as np

# Parameter numbers of Params::set in sim/src/env.rs.
PARAM = {
    "arena_half": 0, "walls": 1, "speed_amp": 2, "ping_min_ms": 3, "ping_max_ms": 4,
    "jitter_max_ms": 5, "start_dist_min": 6, "start_dist_max": 7, "start_yaw_noise": 8,
    "ep_ticks_min": 9, "ep_ticks_max": 10, "w_hit": 11, "w_taken": 12, "w_aim": 13,
    "w_ontarget": 14, "w_range": 15, "w_swing_miss": 16, "mouse_delay_fix": 17,
    "infinite_health": 18, "fall_damage": 19, "kb_random": 20, "kb_h_min": 21,
    "kb_h_max": 22, "kb_v_min": 23, "kb_v_max": 24, "kb_friction_min": 25,
    "kb_friction_max": 26, "kb_extra_h_min": 27, "kb_extra_h_max": 28,
    "kb_extra_v_min": 29, "kb_extra_v_max": 30, "bot_level_min": 31, "bot_level_max": 32,
    "sensitivity": 33, "damage": 34, "kb_ench": 35, "floor_y": 36,
    "w_sprint_hit": 37, "w_reach_adv": 38,
    "w_combo": 39, "w_close": 40, "close_dist": 41,
    "agent_ping_min_ms": 42, "agent_ping_max_ms": 43,
    "cps_min": 44, "cps_max": 45, "opp_cps_min": 46, "opp_cps_max": 47,
    "opp_edge_prob": 48, "opp_reach_min": 49, "opp_reach_max": 50,
    "opp_kb_taken_min": 51, "opp_kb_taken_max": 52,
    "opp_backtrack_min_ms": 53, "opp_backtrack_max_ms": 54,
    "kb_plugin": 55, "kb_preset": 56,
    "w_damage": 57, "opp_autoblock": 58, "sword_wear": 59, "bot_block": 60,
    "pearls": 61, "pearl_prob": 62, "pearl_cd_min_s": 63, "pearl_cd_max_s": 64, "bot_pearl": 65,
    "bot_pearl_lead": 66,
    "health_prob": 67, "prot_min": 68, "prot_max": 69, "sharp_min": 70, "sharp_max": 71, "feather": 72,
    "potions_min": 73, "potions_max": 74, "start_hurt": 75, "start_health_min": 76, "start_health_max": 77,
    "w_win": 78, "w_heal": 79, "w_waste": 80, "bot_potion": 81, "hunger": 82,
    "hurt_who": 83, "pearls_who": 84,
    "speed_end_prob": 85, "speed_left_min_s": 86, "speed_left_max_s": 87,
    "drinks_min": 88, "drinks_max": 89, "hunger_prob": 90, "start_food_min": 91, "start_food_max": 92,
    "speed_who": 93, "bot_drink": 94, "w_speed": 95, "w_fed": 96,
    "refill_prob": 97, "inv_potions_min": 98, "inv_potions_max": 99, "inv_drinks_min": 100, "inv_drinks_max": 101,
    "refill_who": 102, "bot_refill": 103, "w_stock": 104,
    "lesson_prob": 105, "lessons": 106, "lesson_dist_min": 107, "lesson_dist_max": 108,
    "drink_window_s": 109, "w_miss": 110, "w_pot_time": 111, "bot_runner": 112,
    "extreme_share": 113, "w_shake": 114, "pot_strict_prob": 115, "pot_strict": 116,
    "w_gift": 117, "w_hungry": 118, "w_slow": 119, "drinks_stowed": 120, "bot_presser": 121, "w_inv_near": 122, "inv_near_dist": 123, "league_steal": 124, "throw_carry_prob": 125, "teach": 126, "w_speed_refill": 127, "insane_prob": 128, "w_pearl_next": 129, "terrain_prob": 130, "terrain_amount": 131, "retry_prob": 132, "terrain_gap_prob": 133, "w_hp_pot": 134, "hp_pot_scale": 135, "opp_strength_prob": 136, "opp_strength_max": 137, "gank_prob": 138, "w_death": 139, "gank_max": 140, "w_pearl_cost": 141, "pearl_kill_s": 142, "drop_prob": 143, "drop_min": 144, "drop_max": 145, "drill_prob": 146, "drill_self": 147, "drill_gankers": 148, "drill_strength_max": 149, "drill_seconds": 150, "combo_cap": 151, "web_prob": 152, "web_amount": 153, "web_trap_prob": 154, "food_full": 155, "w_full": 156, "w_pearl_low": 157, "pearl_low_hp": 158, "insane_reach_max": 159, "w_sip_hit": 160, "fall_start_prob": 161, "fall_start_min": 162, "fall_start_max": 163, "tiny_kb_prob": 164, "tiny_kb_h_min": 165, "tiny_kb_h_max": 166, "tiny_kb_v_min": 167, "tiny_kb_v_max": 168, "tool_prob": 169, "tool_rod": 170, "tool_snowball": 171, "tool_egg": 172, "tool_balls": 173, "tool_who": 174, "zero_hits_prob": 175, "w_drop": 176, "small_arena_prob": 177, "arena_half_min": 178, "w_pot_early": 184, "w_pot_far": 185,
    "pot_turn_prob": 189, "pot_turn_run_s": 190, "pot_turn_hurt": 191, "w_pearl_pot": 192, "w_pearl_front": 193,
    "bt_prob": 194, "bt_ms_min": 195, "bt_ms_max": 196, "timer_prob": 197, "timer_min": 198, "timer_max": 199,
    "timer_burst_min_s": 200, "timer_burst_max_s": 201, "timer_gap_s": 202, "timer_blink": 203,
    "pearl_spam_prob": 204, "pearl_spam_pearls": 205,
    "pot_relief_hp": 206, "pot_relief": 207, "insane_lessons": 208, "w_sip_near": 209, "sip_near_dist": 210, "bot_trader": 211, "trade_block": 212, "trade_crit": 213, "trade_heal": 214,
    "w_comboed": 179, "comboed_free": 180, "comboed_cap": 181, "combo_gap": 182, "bot_spacer": 183,
    "w_pot_stock": 215, "pot_stock_cap": 216, "extras_bot_only": 217, "crit_prob": 218, "opp_kb_v": 219,
    "pearl_punish": 230, "pearl_punish_hp": 231, "pearl_punish_far": 232, "pearl_punish_rush": 233, "pearl_punish_noise": 234,
}

STAT_NAMES = [
    "episodes", "ticks0", "ticks1", "hits0", "hits1", "sprint_hits0", "sprint_hits1",
    "swings0", "swings1", "attacks0", "attacks1", "on_target0", "on_target1",
    "aim_err_sum0", "max_combo_sum0", "reward_sum0",
    "use_ticks0", "use_ticks1", "blocked0", "blocked1", "damage0", "damage1", "crits0", "crits1",
    "pearls0", "pearls1", "landings0", "landings1", "pearl_won0", "pearl_won1", "pearl_lost0", "pearl_lost1",
    "blocks0", "blocks1", "kb0", "kb1", "tap_hits0", "tap_hits1",
    "deaths0", "deaths1", "potions0", "potions1", "splashes0", "splashes1",
    "heal_self0", "heal_self1", "heal_gift0", "heal_gift1", "pot_offer0", "pot_offer1",
    "pot_perfect0", "pot_perfect1", "pot_double0", "pot_double1", "pot_health0", "pot_health1",
    "pot_away0", "pot_away1", "pot_hit0", "pot_hit1",
    "drinks0", "drinks1", "meals0", "meals1", "sips0", "sips1", "sip_ticks0", "sip_ticks1",
    "sip_hit0", "sip_hit1", "speed_on0", "speed_on1", "fed0", "fed1",
    "pot_after0", "pot_after1", "off0", "off1",
    "refills0", "refills1", "inv_opens0", "inv_opens1", "inv_ticks0", "inv_ticks1", "inv_hit0", "inv_hit1",
    "shake0", "shake1", "speed_waste0", "speed_waste1", "inv_hurt0", "inv_hurt1", "pearl_sense0", "pearl_sense1", "void0", "void1", "terrain_t0", "terrain_t1", "retries", "drills", "drill_deaths", "pearl_low0", "pearl_low1", "not_full0", "not_full1",
    "drops0", "drops1",
    "tool_uses0", "tool_uses1", "tool_hits0", "tool_hits1",
    "pot_given0", "pot_given1", "pot_far0", "pot_far1", "pot_over0", "pot_over1", "pot_early0", "pot_early1",
    "pearl_front0", "pearl_front1", "pearl_side0", "pearl_side1", "pearl_back0", "pearl_back1", "pearl_faced0", "pearl_faced1", "pearl_faced_lost0", "pearl_faced_lost1", "pearl_on_pot0", "pearl_on_pot1", "pearl_pots0", "pearl_pots1", "pearl_deny0", "pearl_deny1",
    "combo3_taken0", "combo3_taken1", "combo_hits_taken0", "combo_hits_taken1",
    "combo_breaks0", "combo_breaks1", "combo_escapes0", "combo_escapes1",
    "timer_ticks", "long_bt", "timer_duels", "spam_duels", "trader_duels",
]

# Per-player fields of VecEnv.view(), then the fields that follow both players.
VIEW_PLAYER = [
    "x", "y", "z", "yaw", "pitch", "on_ground", "sprinting", "hurt_time", "ticks_since_swing",
    "forward", "strafe", "jump", "sprint_key", "attack", "hits", "combo", "sprint_hits",
    "seen_x", "seen_y", "seen_z", "using", "blocking", "blocked", "damage",
    "held", "pearls", "pearl_on", "pearl_x", "pearl_y", "pearl_z",
    "health", "potions", "potion_on", "potion_x", "potion_y", "potion_z",
    "item", "consuming", "speed_s", "food", "drinks", "speed_fx",
    "inv_open", "inv_potions", "inv_drinks",
]
VIEW_GLOBAL = ["ticks_left", "ep_len", "arena_half", "floor_y", "ping0", "ping1", "cps0", "cps1",
               "reach1", "kb_taken1", "backtrack1_ms",
               "kb_kind", "kb_h", "kb_v", "kb_friction", "hit_delay", "autoblock1", "sword_wear",
               "pearl_cd_s", "bot_pearl_kind", "bot_pearl_x", "bot_pearl_z",
               "real_health", "hit_health"]

SELF_PLAY = 0
BOT = 1


def default_lib_path():
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.normpath(os.path.join(here, "..", "..", "sim", "target", "release", "libpvpsim.so"))


def _load(path):
    lib = ctypes.CDLL(path)
    f32p = ctypes.POINTER(ctypes.c_float)
    lib.pvp_vec_new.restype = ctypes.c_void_p
    lib.pvp_vec_new.argtypes = [ctypes.c_int32, ctypes.c_int32, ctypes.c_uint64]
    lib.pvp_vec_free.argtypes = [ctypes.c_void_p]
    lib.pvp_vec_set_param.argtypes = [ctypes.c_void_p, ctypes.c_int32, ctypes.c_double]
    lib.pvp_vec_set_param.restype = ctypes.c_int32
    lib.pvp_vec_reset.argtypes = [ctypes.c_void_p, f32p, f32p]
    lib.pvp_vec_step.argtypes = [
        ctypes.c_void_p, ctypes.POINTER(ctypes.c_int32), f32p, f32p, f32p,
        ctypes.POINTER(ctypes.c_uint8), f32p, f32p,
    ]
    lib.pvp_vec_step.restype = ctypes.c_int32
    lib.pvp_vec_take_stats.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_double)]
    lib.pvp_act_dims.argtypes = [ctypes.POINTER(ctypes.c_int32)]
    lib.pvp_vec_view.argtypes = [ctypes.c_void_p, ctypes.c_int32, f32p]
    lib.pvp_vec_overridden.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint8)]
    lib.pvp_vec_teach.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_int32)]
    return lib


def sim_dims(lib_path=None):
    """What the simulator library gives and takes: (observation values,
    values for the value function only, choices per control)."""
    lib = _load(lib_path or default_lib_path())
    dims = (ctypes.c_int32 * lib.pvp_act_heads())()
    lib.pvp_act_dims(dims)
    return int(lib.pvp_obs_dim()), int(lib.pvp_priv_dim()), [int(d) for d in dims]


class VecEnv:
    """A batch of duels. In SELF_PLAY mode calls alternate between the two
    players of every duel; in BOT mode every call is one tick of player 0."""

    def __init__(self, n, mode, seed, lib_path=None):
        self.lib = _load(lib_path or default_lib_path())
        self.n = n
        self.mode = mode
        self.obs_dim = self.lib.pvp_obs_dim()
        self.priv_dim = self.lib.pvp_priv_dim()
        heads = self.lib.pvp_act_heads()
        dims = (ctypes.c_int32 * heads)()
        self.lib.pvp_act_dims(dims)
        self.act_dims = [int(d) for d in dims]
        self.h = ctypes.c_void_p(self.lib.pvp_vec_new(n, mode, seed))
        self.obs = np.zeros((n, self.obs_dim), np.float32)
        self.priv = np.zeros((n, self.priv_dim), np.float32)
        self.fobs = np.zeros((n, self.obs_dim), np.float32)
        self.fpriv = np.zeros((n, self.priv_dim), np.float32)
        self.rew = np.zeros(n, np.float32)
        self.done = np.zeros(n, np.uint8)
        self._stats = np.zeros(self.lib.pvp_stats_dim(), np.float64)

    @staticmethod
    def _f(a):
        return a.ctypes.data_as(ctypes.POINTER(ctypes.c_float))

    def set_param(self, name, value):
        if not self.lib.pvp_vec_set_param(self.h, PARAM[name], float(value)):
            raise KeyError(name)

    def reset(self):
        self.lib.pvp_vec_reset(self.h, self._f(self.obs), self._f(self.priv))
        return self.obs, self.priv

    def step(self, actions):
        """actions: int32 array [n, heads] for the pending player.
        Returns the index of the player that acts next. self.obs/self.priv
        hold that player's view; self.rew/self.done close that player's
        previous action; self.fobs/self.fpriv hold the last view of an
        episode wherever self.done is non-zero."""
        a = np.ascontiguousarray(actions, dtype=np.int32)
        return self.lib.pvp_vec_step(
            self.h, a.ctypes.data_as(ctypes.POINTER(ctypes.c_int32)),
            self._f(self.obs), self._f(self.priv), self._f(self.rew),
            self.done.ctypes.data_as(ctypes.POINTER(ctypes.c_uint8)),
            self._f(self.fobs), self._f(self.fpriv),
        )

    def overridden(self):
        """Per duel, whether the keys of the action just made were laid
        over by others (player 1 going for the potions, see the parameter
        league_steal): that tick was not the policy's."""
        out = np.zeros(self.n, np.uint8)
        self.lib.pvp_vec_overridden(self.h, out.ctypes.data_as(ctypes.POINTER(ctypes.c_uint8)))
        return out

    def teach(self):
        """Per duel and control, what the teacher of the agent that made
        the last tick would have chosen on it, -1 where it has no opinion
        (only with the parameter teach on)."""
        out = np.zeros((self.n, len(self.act_dims)), np.int32)
        self.lib.pvp_vec_teach(self.h, out.ctypes.data_as(ctypes.POINTER(ctypes.c_int32)))
        return out

    def view(self, index=0):
        """Snapshot of one duel for drawing it: VIEW_PLAYER fields for each
        player, then VIEW_GLOBAL."""
        out = np.zeros(self.lib.pvp_view_dim(), np.float32)
        self.lib.pvp_vec_view(self.h, index, self._f(out))
        return out

    def take_stats(self):
        self.lib.pvp_vec_take_stats(self.h, self._stats.ctypes.data_as(ctypes.POINTER(ctypes.c_double)))
        return dict(zip(STAT_NAMES, self._stats.tolist()))

    def close(self):
        if self.h:
            self.lib.pvp_vec_free(self.h)
            self.h = None
