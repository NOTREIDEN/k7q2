"""ctypes wrapper around the Rust simulator (libchestsim)."""
import ctypes
import os
import numpy as np

STAT_NAMES = [
    "episodes", "ticks", "claims", "kills", "waves", "resets", "deaths", "dmg_dealt", "dmg_taken", "leash_tps",
    "hits", "clicks", "self_punch", "punch_boost", "gapples", "gapple_waste", "potions", "potion_waste", "pearls", "max_simul",
]

# Widths of the parts of an observation, in order (sim/src/env.rs).
K_MOBS, K_CHESTS, K_PROJ = 8, 8, 4
SELF_DIM, TERRAIN_DIM, MOB_DIM, CHEST_DIM, PROJ_DIM, OWN_DIM = 46, 162, 25, 28, 9, 8


def _lib_path():
    here = os.path.dirname(os.path.abspath(__file__))
    for p in [os.environ.get("CHESTSIM_LIB", ""), os.path.join(here, "..", "..", "chestsim", "target", "release", "libchestsim.so")]:
        if p and os.path.exists(p):
            return p
    raise FileNotFoundError("build the simulator first: cd chestsim && cargo build --release")


_lib = None


def lib():
    global _lib
    if _lib is None:
        L = ctypes.CDLL(_lib_path())
        L.cs_vec_new.restype = ctypes.c_void_p
        L.cs_vec_new.argtypes = [ctypes.c_int, ctypes.c_uint64]
        L.cs_vec_free.argtypes = [ctypes.c_void_p]
        L.cs_vec_set_param.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_double]
        L.cs_vec_set_param.restype = ctypes.c_int
        fp = ctypes.POINTER(ctypes.c_float)
        L.cs_vec_reset.argtypes = [ctypes.c_void_p, fp, fp]
        L.cs_vec_step.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_int32), fp, fp, fp, ctypes.POINTER(ctypes.c_uint8), fp, fp]
        L.cs_vec_take_stats.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_double)]
        L.cs_act_dims.argtypes = [ctypes.POINTER(ctypes.c_int32)]
        L.cs_vec_teacher.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_int32)]
        L.cs_vec_set_next_seed.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_uint64]
        L.cs_vec_json.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_char_p, ctypes.c_int64]
        L.cs_vec_json.restype = ctypes.c_int64
        L.cs_vec_env_stats.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.POINTER(ctypes.c_double)]
        L.cs_param_name.restype = ctypes.c_char_p
        L.cs_param_name.argtypes = [ctypes.c_int]
        _lib = L
    return _lib


def param_ids():
    L = lib()
    return {L.cs_param_name(i).decode(): i for i in range(L.cs_param_count())}


def sim_dims():
    L = lib()
    heads = L.cs_act_heads()
    buf = (ctypes.c_int32 * heads)()
    L.cs_act_dims(buf)
    return L.cs_obs_dim(), L.cs_priv_dim(), list(buf)


class VecEnv:
    def __init__(self, n, seed=0, params=None):
        L = lib()
        self.L, self.n = L, n
        self.obs_dim, self.priv_dim, self.act_dims = sim_dims()
        self.heads = len(self.act_dims)
        self.p = L.cs_vec_new(n, seed)
        self.ids = param_ids()
        for k, v in (params or {}).items():
            self.set_param(k, v)
        self.obs = np.zeros((n, self.obs_dim), np.float32)
        self.priv = np.zeros((n, self.priv_dim), np.float32)
        self.fobs = np.zeros((n, self.obs_dim), np.float32)
        self.fpriv = np.zeros((n, self.priv_dim), np.float32)
        self.rew = np.zeros(n, np.float32)
        self.done = np.zeros(n, np.uint8)
        self.stats = np.zeros(L.cs_stats_dim(), np.float64)

    def set_param(self, name, value):
        if name not in self.ids:
            raise KeyError(f"unknown simulator parameter {name}")
        self.L.cs_vec_set_param(self.p, self.ids[name], float(value))

    @staticmethod
    def _f(a):
        return a.ctypes.data_as(ctypes.POINTER(ctypes.c_float))

    def reset(self):
        self.L.cs_vec_reset(self.p, self._f(self.obs), self._f(self.priv))
        return self.obs, self.priv

    def step(self, actions):
        a = np.ascontiguousarray(actions, dtype=np.int32)
        self.L.cs_vec_step(self.p, a.ctypes.data_as(ctypes.POINTER(ctypes.c_int32)), self._f(self.obs), self._f(self.priv),
                           self._f(self.rew), self.done.ctypes.data_as(ctypes.POINTER(ctypes.c_uint8)), self._f(self.fobs), self._f(self.fpriv))
        return self.obs, self.priv, self.rew, self.done, self.fobs, self.fpriv

    def teacher(self):
        out = np.zeros((self.n, self.heads), np.int32)
        self.L.cs_vec_teacher(self.p, out.ctypes.data_as(ctypes.POINTER(ctypes.c_int32)))
        return out

    def take_stats(self):
        self.L.cs_vec_take_stats(self.p, self.stats.ctypes.data_as(ctypes.POINTER(ctypes.c_double)))
        return dict(zip(STAT_NAMES, self.stats.tolist()))

    def set_next_seed(self, i, seed):
        """The level environment i plays after its episode ends."""
        self.L.cs_vec_set_next_seed(self.p, int(i), int(seed))

    def json(self, i, what):
        """Environment i's map (what 0) or its state this tick (what 1), as JSON text."""
        cap = 1 << 16
        while True:
            buf = ctypes.create_string_buffer(cap)
            n = self.L.cs_vec_json(self.p, int(i), int(what), buf, cap)
            if n <= cap:
                return buf.raw[:n].decode()
            cap = int(n) + 1

    def env_stats(self, i):
        """Environment i's statistics so far, as an array in STAT_NAMES order."""
        out = np.zeros(len(STAT_NAMES), np.float64)
        self.L.cs_vec_env_stats(self.p, i, out.ctypes.data_as(ctypes.POINTER(ctypes.c_double)))
        return out

    def close(self):
        if self.p:
            self.L.cs_vec_free(self.p)
            self.p = None
