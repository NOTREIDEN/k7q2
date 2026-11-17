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
]

# Per-player fields of VecEnv.view(), then the fields that follow both players.
VIEW_PLAYER = [
    "x", "y", "z", "yaw", "pitch", "on_ground", "sprinting", "hurt_time", "ticks_since_swing",
    "forward", "strafe", "jump", "sprint_key", "attack", "hits", "combo", "sprint_hits",
    "seen_x", "seen_y", "seen_z", "using", "blocking", "blocked", "damage",
    "held", "pearls", "pearl_on", "pearl_x", "pearl_y", "pearl_z",
    "health", "potions", "potion_on", "potion_x", "potion_y", "potion_z",
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
