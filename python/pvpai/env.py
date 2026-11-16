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
    "w_sprint_hit": 37,
}

STAT_NAMES = [
    "episodes", "ticks0", "ticks1", "hits0", "hits1", "sprint_hits0", "sprint_hits1",
    "swings0", "swings1", "attacks0", "attacks1", "on_target0", "on_target1",
    "aim_err_sum0", "max_combo_sum0", "reward_sum0",
]

# Per-player fields of VecEnv.view(), then the fields that follow both players.
VIEW_PLAYER = [
    "x", "y", "z", "yaw", "pitch", "on_ground", "sprinting", "hurt_time", "ticks_since_swing",
    "forward", "strafe", "jump", "sprint_key", "attack", "hits", "combo", "sprint_hits",
    "seen_x", "seen_y", "seen_z",
]
VIEW_GLOBAL = ["ticks_left", "ep_len", "arena_half", "floor_y", "ping0", "ping1"]

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
