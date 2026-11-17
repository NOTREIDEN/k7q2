"""How a checkpoint throws its healing potions against one bot level: where
it looks at the throw, how fast it is moving, whether the potion breaks on
the ground or is caught in the air (a potion that has flown five ticks
breaks on its own thrower, which gets all eight points), what it got out of
it and how long its crosshair takes to be back on the opponent; and for
every time it heals, with one potion or several in a row, how long its
crosshair is off the opponent before, between and after the throws.

    python potion_report.py runs/stage1/latest.pt 4
    python potion_report.py latest.pt 4 speed_end_prob=1 speed_left_max_s=-1 speed_who=1

The second argument is the level of the bot; the rest are simulator
parameters, laid over an even start with real health and seven potions."""
import math
import sys

import numpy as np
import torch

from pvpai.env import BOT, VIEW_GLOBAL, VIEW_PLAYER, VecEnv, sim_dims
from pvpai.model import load_policy

# The crosshair counts as on the opponent within this many degrees.
ON = 12.0


def roll(ckpt, n, seconds, level, seed, extra):
    torch.manual_seed(seed)
    policy, ck, _ = load_policy(ckpt, sim_dims())
    policy.eval()
    ticks = int(seconds * 20)
    env = VecEnv(n, BOT, seed)
    p = {"ep_ticks_min": ticks, "ep_ticks_max": ticks, "bot_level_min": level, "bot_level_max": level,
         "infinite_health": 0, "potions_min": 7, "potions_max": 7, "start_hurt": 0, "start_yaw_noise": 25}
    p.update(extra)
    for k, v in p.items():
        env.set_param(k, v)
    obs, priv = env.reset()
    state = policy.initial_state(n)
    views = np.zeros((ticks, n, len(env.view(0))), np.float32)
    t = lambda a: torch.from_numpy(a.copy())
    for i in range(ticks):
        act, _, _, state = policy.act(t(obs), t(priv), state, False)
        env.step(act.numpy().astype(np.int32))
        obs, priv = env.obs, env.priv
        for j in range(n):
            views[i, j] = env.view(j)
    env.close()
    return views, ck


def throws(views):
    """One row per potion the agent threw: pitch, own speed in m/s, distance
    to the opponent, health, whether it was caught in the air, the health it
    gave, the health that was missing, and the ticks until the crosshair was
    back on the opponent."""
    n_p = len(VIEW_PLAYER)
    ix = {k: i for i, k in enumerate(VIEW_PLAYER)}
    floor = views[0, 0, 2 * n_p + VIEW_GLOBAL.index("floor_y")]
    ticks, n = views.shape[:2]
    a, b = views[:, :, :n_p], views[:, :, n_p:2 * n_p]

    def off(t, i):
        dx = b[t, i, ix["x"]] - a[t, i, ix["x"]]
        dy = (b[t, i, ix["y"]] + 0.9) - (a[t, i, ix["y"]] + 1.62)
        dz = b[t, i, ix["z"]] - a[t, i, ix["z"]]
        yaw = math.degrees(math.atan2(-dx, dz))
        pitch = -math.degrees(math.atan2(dy, math.hypot(dx, dz)))
        return math.hypot((yaw - a[t, i, ix["yaw"]] + 180) % 360 - 180, pitch - a[t, i, ix["pitch"]])

    rows = []
    for i in range(n):
        left = a[:, i, ix["potions"]]
        for t in range(2, ticks - 45):
            if left[t] >= left[t - 1] or a[t, i, ix["health"]] <= 0:
                continue
            # The flight: from the first tick the potion is seen to the last.
            k = t
            while k < t + 10 and a[k, i, ix["potion_on"]] == 0:
                k += 1
            j = k
            while j < t + 44 and a[j, i, ix["potion_on"]] > 0:
                j += 1
            if j == k:
                continue
            me = a[t, i]
            speed = 20 * math.hypot(me[ix["x"]] - a[t - 1, i, ix["x"]], me[ix["z"]] - a[t - 1, i, ix["z"]])
            dist = math.hypot(b[t, i, ix["x"]] - me[ix["x"]], b[t, i, ix["z"]] - me[ix["z"]])
            caught = a[j - 1, i, ix["potion_y"]] - floor > 0.45
            got = a[j + 1, i, ix["health"]] - a[j - 1, i, ix["health"]]
            back = next((d for d in range(1, 40) if off(t + d, i) < ON), 40)
            rows.append((me[ix["pitch"]], speed, dist, me[ix["health"]], caught, got, min(20 - me[ix["health"]], 8), back))
    return np.array(rows, np.float64)


def sessions(views):
    """One row per run of potions thrown within a second of each other:
    how many, the ticks the crosshair was off the opponent before the
    first throw, between the first and the last and after the last, the
    health at the first throw, the ticks since the agent was last hit and
    the hits it took from looking away to looking back."""
    n_p = len(VIEW_PLAYER)
    ix = {k: i for i, k in enumerate(VIEW_PLAYER)}
    ticks, n = views.shape[:2]
    a, b = views[:, :, :n_p], views[:, :, n_p:2 * n_p]

    def off(t, i):
        dx = b[t, i, ix["x"]] - a[t, i, ix["x"]]
        dy = (b[t, i, ix["y"]] + 0.9) - (a[t, i, ix["y"]] + 1.62)
        dz = b[t, i, ix["z"]] - a[t, i, ix["z"]]
        yaw = math.degrees(math.atan2(-dx, dz))
        pitch = -math.degrees(math.atan2(dy, math.hypot(dx, dz)))
        return math.hypot((yaw - a[t, i, ix["yaw"]] + 180) % 360 - 180, pitch - a[t, i, ix["pitch"]])

    rows = []
    for i in range(n):
        left, hp, taken = a[:, i, ix["potions"]], a[:, i, ix["health"]], b[:, i, ix["hits"]]
        thrown = [t for t in range(2, ticks - 60) if left[t] < left[t - 1] and hp[t] > 0]
        k = 0
        while k < len(thrown):
            j = k
            while j + 1 < len(thrown) and thrown[j + 1] - thrown[j] <= 20:
                j += 1
            first, last = thrown[k], thrown[j]
            s = first
            while s > max(first - 80, 1) and off(s, i) >= ON:
                s -= 1
            e = last + 1
            while e < min(last + 60, ticks - 1) and off(e, i) >= ON:
                e += 1
            h = first
            while h > max(first - 200, 1) and taken[h] == taken[h - 1]:
                h -= 1
            rows.append((j - k + 1, first - s, last - first, e - last, hp[first], first - h, taken[e] - taken[s]))
            k = j + 1
    return np.array(rows, np.float64)


def main():
    ckpt, level = sys.argv[1], int(sys.argv[2])
    extra = {k: float(v) for k, v in (kv.split("=") for kv in sys.argv[3:])}
    views, ck = roll(ckpt, 64, 60, level, 5, extra)
    r = throws(views)
    print(f"checkpoint with {ck.get('samples', 0)/1e6:.1f}M samples against bot level {level}: {len(r)} potions in 64 duels of 60 s")
    if not len(r):
        return
    print(f"  thrown at {r[:, 3].mean():.1f} health, {r[:, 2].mean():.1f} blocks from the opponent, moving at {r[:, 1].mean():.1f} m/s")
    print(f"  caught in the air {100 * r[:, 4].mean():.0f} %, the rest broke on the ground; "
          f"crosshair back on the opponent after {r[:, 7].mean():.1f} ticks (half of them within {np.median(r[:, 7]):.0f})")
    q = sessions(views)
    print(f"  {len(q)} times it healed, with {q[:, 0].mean():.2f} potions at {q[:, 4].mean():.1f} health, {q[:, 5].mean():.0f} ticks after it was last hit: "
          f"crosshair off the opponent {q[:, 1].mean():.1f} ticks before the first throw, {q[:, 2].mean():.1f} between throws and "
          f"{q[:, 3].mean():.1f} after the last, {q[:, 1:4].sum(1).mean():.1f} in all (half of them within {np.median(q[:, 1:4].sum(1)):.0f}); "
          f"hits taken meanwhile {q[:, 6].mean():.2f}")
    print("  by where it looked (pitch, down is positive):")
    for lo, hi in ((-90, 0), (0, 10), (10, 20), (20, 35), (35, 60), (60, 91)):
        s = r[(r[:, 0] >= lo) & (r[:, 0] < hi)]
        if len(s) >= 5:
            print(f"    {lo:3d} to {hi:2d}: {100 * len(s) / len(r):3.0f} % of the potions, caught {100 * s[:, 4].mean():3.0f} %, "
                  f"got {s[:, 5].mean():.1f} of the {s[:, 6].mean():.1f} missing, back in {s[:, 7].mean():4.1f} ticks, at {s[:, 1].mean():.1f} m/s")


if __name__ == "__main__":
    main()
