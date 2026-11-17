"""How a checkpoint deals with a bot that throws pearls at it: for each kind
of pearl of the bot, where it lands, how fast the agent has its crosshair
on the bot again and who hits whom in the two seconds that follow.

    python pearl_report.py runs/stage1/latest.pt 4
    python pearl_report.py latest.pt 4 pearl_cd_min_s=5 pearl_cd_max_s=5

The second argument is the level of the bot; the rest are simulator
parameters."""
import math
import sys

import numpy as np
import torch

from pvpai.env import BOT, VIEW_GLOBAL, VIEW_PLAYER, VecEnv, sim_dims
from pvpai.model import load_policy


def roll(ckpt, n, seconds, level, seed, extra):
    torch.manual_seed(seed)
    policy, ck, _ = load_policy(ckpt, sim_dims())
    policy.eval()
    ticks = int(seconds * 20)
    env = VecEnv(n, BOT, seed)
    p = {"ep_ticks_min": ticks + 5, "ep_ticks_max": ticks + 5, "bot_level_min": level, "bot_level_max": level}
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
    return views, ck.get("samples", 0)


def main():
    ckpt, level = sys.argv[1], int(sys.argv[2])
    extra = dict((kv.split("=")[0], float(kv.split("=")[1])) for kv in sys.argv[3:])
    n, seconds = 64, 60
    v, samples = roll(ckpt, n, seconds, level, 21, extra)
    P = len(VIEW_PLAYER)
    f = lambda name, who: v[:, :, who * P + VIEW_PLAYER.index(name)]
    T = v.shape[0]
    minutes = n * seconds / 60
    x0, y0, z0, yaw0 = f("x", 0), f("y", 0), f("z", 0), f("yaw", 0)
    sx, sz = f("seen_x", 0), f("seen_z", 0)  # the bot as the agent's client shows it
    bx, bz = f("x", 1), f("z", 1)
    hits0, hits1 = f("hits", 0), f("hits", 1)
    held1 = f("pearls", 1)
    rows = []
    G = 2 * P
    on1 = f("pearl_on", 1)
    px, pz = f("pearl_x", 1), f("pearl_z", 1)
    for j in range(n):
        # A pearl of the bot that was in flight a tick ago and is gone now has landed.
        ends = np.flatnonzero((on1[:-1, j] > 0) & (on1[1:, j] == 0)) + 1
        for t0 in ends:
            if t0 < 20 or t0 + 45 >= T:
                continue
            land = (px[t0 - 1, j], pz[t0 - 1, j])
            # It did put the bot there (a throw into a wall is taken back).
            if math.hypot(bx[t0 + 6, j] - land[0], bz[t0 + 6, j] - land[1]) > 4.0:
                continue
            def err(t):
                want = math.degrees(math.atan2(-(sx[t, j] - x0[t, j]), sz[t, j] - z0[t, j]))
                return abs((want - yaw0[t, j] + 180) % 360 - 180)
            e = [err(t0 + k) for k in range(0, 41)]
            # From when the agent's client shows the bot at the new place (ping and a slide of three ticks).
            seen = next((k for k in range(0, 30) if math.hypot(sx[t0 + k, j] - land[0], sz[t0 + k, j] - land[1]) < 1.5), 30)
            back = next((k for k in range(seen, 41) if e[k] < 12.0), 41) - seen
            near = math.hypot(land[0] - x0[t0, j], land[1] - z0[t0, j])
            before = math.hypot(bx[t0 - 1, j] - x0[t0 - 1, j], bz[t0 - 1, j] - z0[t0 - 1, j])
            dealt = hits0[t0 + 40, j] - hits0[t0, j]
            taken = hits1[t0 + 40, j] - hits1[t0, j]
            a = np.flatnonzero(hits0[t0:t0 + 41, j] > hits0[t0, j])
            b = np.flatnonzero(hits1[t0:t0 + 41, j] > hits1[t0, j])
            first = 1.0 if len(a) and (not len(b) or a[0] < b[0]) else 0.0
            # The longest run of hits the bot got in without one coming back.
            run, best, h0 = 0, 0, hits0[t0, j]
            for t in range(t0 + 1, t0 + 41):
                if hits0[t, j] > h0:
                    run, h0 = 0, hits0[t, j]
                if hits1[t, j] > hits1[t - 1, j]:
                    run += 1
                    best = max(best, run)
            kind = v[t0, j, G + VIEW_GLOBAL.index("bot_pearl_kind")]
            tx, tz = v[t0, j, G + VIEW_GLOBAL.index("bot_pearl_x")], v[t0, j, G + VIEW_GLOBAL.index("bot_pearl_z")]
            miss = math.hypot(land[0] - tx, land[1] - tz)             # landing against the spot aimed at
            plan = math.hypot(tx - x0[t0, j], tz - z0[t0, j])          # the spot aimed at against the agent
            k5, k10 = min(seen + 5, 40), min(seen + 10, 40)
            rows.append((near, e[min(seen, 40)], e[k5], e[k10], back, dealt, taken, first, best, before, kind, miss, plan, seen))
    r = np.array(rows, float)
    print(f"{ckpt}: {samples/1e6:.0f}M samples, bot level {level}, {extra}")
    print(f"  hits dealt {hits0[-1].sum()/minutes:.1f} per minute, taken {hits1[-1].sum()/minutes:.1f}")
    if not len(r):
        print("  no pearls of the bot landed")
        return
    for name, m in (("to attack", r[:, 10] == 1), ("out of a combo, next to the agent", r[:, 10] == 2), ("to get away", r[:, 10] == 3)):
        q = r[m]
        if not len(q):
            continue
        print(f"  pearls of the bot {name}: {len(q)/minutes:.1f} per minute, thrown from {np.median(q[:,9]):.1f} blocks, landing {np.median(q[:,0]):.1f} blocks from the agent (median), within 4 blocks {np.mean(q[:,0] < 4):.2f}")
        print(f"    landing {np.median(q[:,11]):.1f} blocks from the spot aimed at, which was {np.median(q[:,12]):.1f} blocks from where the agent then was (medians)")
        print(f"    the agent sees the bot there {np.median(q[:,13]):.0f} ticks later; crosshair off it by {np.median(q[:,1]):.0f} degrees then, {np.median(q[:,2]):.0f} after 5 more ticks, {np.median(q[:,3]):.0f} after 10 (medians)")
        print(f"    ticks from then until the crosshair is within 12 degrees: median {np.median(q[:,4]):.0f}, within 5 ticks {np.mean(q[:,4] <= 5):.2f}, within 10 ticks {np.mean(q[:,4] <= 10):.2f}")
        print(f"    in the two seconds after: agent hits {q[:,5].mean():.2f}, bot hits {q[:,6].mean():.2f}, agent hits first {q[:,7].mean():.2f}")
        print(f"    longest run of bot hits without an answer: mean {q[:,8].mean():.2f}, 3 or more {np.mean(q[:,8] >= 3):.2f}")

if __name__ == "__main__":
    main()
