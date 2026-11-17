"""Record duels of a checkpoint so they can be drawn with render.py.

    python record.py runs/stage1/latest.pt --against bot:4 --out duel.npz
    python record.py new.pt --against old.pt --out duel.npz
    python record.py latest.pt --against self --out duel.npz

Several duels are played at once and one is kept: by default the one with
the median hit difference, so the recording shows a typical duel rather
than the best one."""
import argparse

import numpy as np
import torch

from pvpai.env import BOT, SELF_PLAY, VIEW_GLOBAL, VIEW_PLAYER, VecEnv, sim_dims
from pvpai.model import load_policy


def load(path, lib=None):
    policy, ck, _ = load_policy(path, sim_dims(lib))
    policy.eval()
    return policy, ck.get("samples", 0)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("checkpoint")
    ap.add_argument("--against", default="bot:4", help="bot:LEVEL, self, or another checkpoint")
    ap.add_argument("--out", default="duel.npz")
    ap.add_argument("--seconds", type=float, default=40.0)
    ap.add_argument("--duels", type=int, default=15)
    ap.add_argument("--pick", default="median", choices=["median", "best", "worst", "first"])
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--greedy", action="store_true")
    ap.add_argument("--param", action="append", default=[])
    ap.add_argument("--lib", default=None, help="path to libpvpsim.so")
    args = ap.parse_args()

    torch.manual_seed(args.seed)
    policy, samples = load(args.checkpoint, args.lib)
    ticks = int(args.seconds * 20)
    bot = args.against.startswith("bot:")
    env = VecEnv(args.duels, BOT if bot else SELF_PLAY, args.seed, args.lib)
    params = {"ep_ticks_min": ticks + 5, "ep_ticks_max": ticks + 5, "start_dist_min": 5, "start_dist_max": 8,
              "start_yaw_noise": 25}
    if bot:
        level = int(args.against.split(":")[1])
        params.update(bot_level_min=level, bot_level_max=level)
        names = ["IA", f"Bot nivel {level}"]
        other = None
    elif args.against == "self":
        other, names = policy, ["IA", "IA (copia)"]
    else:
        other, osamples = load(args.against, args.lib)
        names = ["IA", f"IA anterior ({osamples/1e6:.0f}M)"]
    for kv in args.param:
        k, v = kv.split("=")
        params[k] = float(v)
    for k, v in params.items():
        env.set_param(k, v)

    obs, priv = env.reset()
    n = args.duels
    state = [policy.initial_state(n), (other or policy).initial_state(n)]
    views = np.zeros((ticks, n, len(env.view(0))), np.float32)
    t = lambda a: torch.from_numpy(a.copy())
    for i in range(ticks):
        act, _, _, state[0] = policy.act(t(obs), t(priv), state[0], args.greedy)
        env.step(act.numpy().astype(np.int32))
        if not bot:
            act, _, _, state[1] = other.act(t(env.obs), t(env.priv), state[1], args.greedy)
            env.step(act.numpy().astype(np.int32))
        obs, priv = env.obs, env.priv
        for j in range(n):
            views[i, j] = env.view(j)
    env.close()

    h0 = views[-1, :, VIEW_PLAYER.index("hits")]
    h1 = views[-1, :, len(VIEW_PLAYER) + VIEW_PLAYER.index("hits")]
    diff = h0 - h1
    order = np.argsort(diff)
    k = {"median": order[n // 2], "best": order[-1], "worst": order[0], "first": 0}[args.pick]
    print(f"{names[0]} against {names[1]}, {n} duels of {args.seconds:.0f} s")
    print(f"hits dealt, mean {h0.mean():.1f}; hits taken, mean {h1.mean():.1f}; difference from {diff.min():+.0f} to {diff.max():+.0f}")
    print(f"kept duel {k}: {h0[k]:.0f} dealt, {h1[k]:.0f} taken")
    np.savez_compressed(args.out, views=views[:, k], names=np.array(names), samples=samples,
                        player_fields=np.array(VIEW_PLAYER), global_fields=np.array(VIEW_GLOBAL),
                        all_dealt=h0, all_taken=h1, picked=k)
    print("saved", args.out)


if __name__ == "__main__":
    main()
