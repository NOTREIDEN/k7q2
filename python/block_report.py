"""How a checkpoint uses the right mouse button against a bot: how long it
holds a block, what its hits do right after one, and how it is hit itself.

    python block_report.py runs/stage1/latest.pt 4

The second argument is the level of the bot; the rest are simulator
parameters."""
import sys

import numpy as np
import torch

from pvpai.env import BOT, VIEW_PLAYER, VecEnv, sim_dims
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


def runs(mask):
    """Lengths of the runs of True in a 1-d array."""
    d = np.diff(np.concatenate([[0], mask.astype(np.int8), [0]]))
    return np.flatnonzero(d == -1) - np.flatnonzero(d == 1)


def main():
    ckpt = sys.argv[1]
    level = int(sys.argv[2])
    extra = dict((kv.split("=")[0], float(kv.split("=")[1])) for kv in sys.argv[3:])
    n, seconds = 64, 60
    v, samples = roll(ckpt, n, seconds, level, 11, extra)
    P = len(VIEW_PLAYER)
    f = lambda name, who: v[:, :, who * P + VIEW_PLAYER.index(name)]
    T = v.shape[0]
    minutes = n * seconds / 60
    using = f("using", 0) > 0.5
    lens = np.concatenate([runs(using[:, j]) for j in range(n)])
    print(f"{ckpt}: {samples/1e6:.0f}M samples, bot level {level}, {extra}")
    print(f"  blocks started per minute {len(lens)/minutes:.1f}, share of ticks blocking {using.mean():.3f}")
    if len(lens):
        edges = [(1, 1), (2, 2), (3, 4), (5, 8), (9, 20), (21, 10**6)]
        print("  length of a block in ticks: " + ", ".join(
            f"{a if a == b else (str(a) + '-' + str(b) if b < 10**6 else str(a) + '+')}: {100*np.mean((lens >= a) & (lens <= b)):.0f} %"
            for a, b in edges) + f"; mean {lens.mean():.1f}, median {np.median(lens):.0f}")
        print(f"  share of blocking ticks that are in blocks of 5 ticks or more: {lens[lens >= 5].sum()/lens.sum():.2f}")
    # Hits dealt by the agent, by whether it had blocked just before.
    hits0, sp0 = f("hits", 0), f("sprint_hits", 0)
    hits1 = f("hits", 1)
    x0, z0, x1, z1 = f("x", 0), f("z", 0), f("x", 1), f("z", 1)
    cats = {"after a block (within 6 ticks)": [], "other": []}
    for j in range(n):
        ht = np.flatnonzero(np.diff(hits0[:, j]) > 0) + 1
        for t0 in ht:
            if t0 + 10 >= T or t0 < 8:
                continue
            was = using[t0 - 6:t0, j].any()
            sprint = sp0[t0, j] > sp0[t0 - 1, j]
            dx, dz = x1[t0 - 1, j] - x0[t0 - 1, j], z1[t0 - 1, j] - z0[t0 - 1, j]
            d = np.hypot(dx, dz) + 1e-9
            # How far the hit pushed the opponent along the line between them in ten ticks.
            push = ((x1[t0 + 9, j] - x1[t0 - 1, j]) * dx + (z1[t0 + 9, j] - z1[t0 - 1, j]) * dz) / d
            # Whether the agent got the next hit in too (no hit taken in between).
            later0 = np.flatnonzero(hits0[t0 + 1:, j] > hits0[t0, j])
            later1 = np.flatnonzero(hits1[t0 + 1:, j] > hits1[t0, j])
            nxt = (len(later0) > 0) and (len(later1) == 0 or later0[0] < later1[0])
            cats["after a block (within 6 ticks)" if was else "other"].append((sprint, push, nxt))
    for k, a in cats.items():
        if not a:
            continue
        a = np.array(a, float)
        print(f"  hits dealt {k}: {len(a)/minutes:.1f} per minute, with sprint {a[:,0].mean():.2f}, "
              f"pushed the opponent {a[:,1].mean():.2f} blocks, next hit is the agent's too {a[:,2].mean():.2f}")
    # Hits taken, by what the agent was doing.
    blk = f("blocking", 0) > 0.5
    taken_block = taken_free = 0
    combos = []
    for j in range(n):
        ht = np.flatnonzero(np.diff(hits1[:, j]) > 0) + 1
        for t0 in ht:
            if blk[t0 - 1, j] or blk[t0, j]:
                taken_block += 1
            else:
                taken_free += 1
        c = f("combo", 1)[:, j]
        ends = np.flatnonzero((c[1:] < c[:-1]))
        combos += list(c[ends])
        combos.append(c[-1])
    combos = np.array([c for c in combos if c > 0])
    # What a hit taken does to the agent, by whether it was blocking: how
    # far it is pushed in ten ticks, how far apart the two are then, and
    # whether the next hit is the opponent's again.
    took = {"blocking": [], "not blocking": []}
    for j in range(n):
        for t0 in np.flatnonzero(np.diff(hits1[:, j]) > 0) + 1:
            if t0 + 10 >= T or t0 < 2:
                continue
            dx, dz = x0[t0 - 1, j] - x1[t0 - 1, j], z0[t0 - 1, j] - z1[t0 - 1, j]
            d = np.hypot(dx, dz) + 1e-9
            push = ((x0[t0 + 9, j] - x0[t0 - 1, j]) * dx + (z0[t0 + 9, j] - z0[t0 - 1, j]) * dz) / d
            apart = np.hypot(x0[t0 + 9, j] - x1[t0 + 9, j], z0[t0 + 9, j] - z1[t0 + 9, j])
            later0 = np.flatnonzero(hits0[t0 + 1:, j] > hits0[t0, j])
            later1 = np.flatnonzero(hits1[t0 + 1:, j] > hits1[t0, j])
            again = (len(later1) > 0) and (len(later0) == 0 or later1[0] < later0[0])
            took["blocking" if blk[t0 - 1, j] or blk[t0, j] else "not blocking"].append((push, apart, again))
    for k, a in took.items():
        if a:
            a = np.array(a, float)
            print(f"  hits taken {k}: {len(a)/minutes:.1f} per minute, pushed the agent {a[:,0].mean():.2f} blocks, "
                  f"{a[:,1].mean():.2f} apart ten ticks later, next hit is the opponent's too {a[:,2].mean():.2f}")
    print(f"  hits dealt {hits0[-1].sum()/minutes:.1f} per minute, taken {hits1[-1].sum()/minutes:.1f}; taken while blocking {taken_block/max(1, taken_block+taken_free):.2f}")
    print(f"  hits taken per second while blocking {taken_block/max(1, blk.sum())*20:.2f}, while not {taken_free/max(1, (~blk).sum())*20:.2f}")
    print(f"  combos of the bot on the agent: mean length {combos.mean():.2f}, of 3 or more {np.mean(combos >= 3):.2f}, of 5 or more {np.mean(combos >= 5):.3f}, longest {combos.max():.0f}")
    c0 = []
    for j in range(n):
        c = f("combo", 0)[:, j]
        ends = np.flatnonzero((c[1:] < c[:-1]))
        c0 += list(c[ends])
        c0.append(c[-1])
    c0 = np.array([c for c in c0 if c > 0])
    print(f"  combos of the agent on the bot: mean length {c0.mean():.2f}, of 3 or more {np.mean(c0 >= 3):.2f}, of 5 or more {np.mean(c0 >= 5):.3f}, longest {c0.max():.0f}")


if __name__ == "__main__":
    main()
