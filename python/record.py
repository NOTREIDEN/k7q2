"""Record duels of a checkpoint so they can be drawn with render.py.

    python record.py runs/stage1/latest.pt --against bot:4 --out duel.npz
    python record.py new.pt --against old.pt --out duel.npz
    python record.py latest.pt --against self --out duel.npz

Several duels are played at once and one is kept: by default the one with
the median hit difference, so the recording shows a typical duel rather
than the best one. With --health the players have armour, real health and
healing potions, a duel is recorded until one of them dies, and the one
kept is the median by who won and with how much health left."""
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
    ap.add_argument("--health", action="store_true", help="real health, armour and healing potions")
    args = ap.parse_args()

    torch.manual_seed(args.seed)
    policy, samples = load(args.checkpoint, args.lib)
    ticks = int(args.seconds * 20)
    bot = args.against.startswith("bot:")
    env = VecEnv(args.duels, BOT if bot else SELF_PLAY, args.seed, args.lib)
    params = {"ep_ticks_min": ticks + 5, "ep_ticks_max": ticks + 5, "start_dist_min": 5, "start_dist_max": 8,
              "start_yaw_noise": 25}
    if args.health:
        params.update(infinite_health=0, start_hurt=0)
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
    # A duel is recorded until it ends: `length` ticks, and who won it (-1
    # when nobody died while it was recorded).
    length = np.full(n, ticks)
    winner = np.full(n, -1)
    t = lambda a: torch.from_numpy(a.copy())
    for i in range(ticks):
        act, _, _, state[0] = policy.act(t(obs), t(priv), state[0], args.greedy)
        env.step(act.numpy().astype(np.int32))
        if not bot:
            act, _, _, state[1] = other.act(t(env.obs), t(env.priv), state[1], args.greedy)
            env.step(act.numpy().astype(np.int32))
        obs, priv = env.obs, env.priv
        for j in range(n):
            # What closes here is the first player's last action: a death
            # is worth far more than anything else in a tick, to the one
            # that lives and against the one that dies.
            if length[j] == ticks and env.done[j]:
                length[j] = i
                if env.done[j] == 1:
                    winner[j] = 0 if env.rew[j] > 0 else 1
            if i < length[j]:
                views[i, j] = env.view(j)
    env.close()

    last = views[length - 1, np.arange(n)]
    field = lambda p, name: last[:, p * len(VIEW_PLAYER) + VIEW_PLAYER.index(name)]
    h0, h1 = field(0, "hits"), field(1, "hits")
    diff = h0 - h1
    print(f"{names[0]} against {names[1]}, {n} duels of {args.seconds:.0f} s")
    print(f"hits dealt, mean {h0.mean():.1f}; hits taken, mean {h1.mean():.1f}; difference from {diff.min():+.0f} to {diff.max():+.0f}")
    score = diff
    if args.health:
        # Won or lost first, then the health the two were left with.
        score = np.where(winner == 0, 100.0, np.where(winner == 1, -100.0, 0.0)) + field(0, "health") - field(1, "health")
        print(f"won {int((winner == 0).sum())}, lost {int((winner == 1).sum())}, not over {int((winner < 0).sum())}; "
              f"a duel lasted {length.mean() / 20:.1f} s; potions left {field(0, 'potions').mean():.1f} and {field(1, 'potions').mean():.1f}")
    order = np.argsort(score, kind="stable")
    k = {"median": order[n // 2], "best": order[-1], "worst": order[0], "first": 0}[args.pick]
    print(f"kept duel {k}: {h0[k]:.0f} dealt, {h1[k]:.0f} taken" + (f", {length[k] / 20:.1f} s, winner {winner[k]}" if args.health else ""))
    np.savez_compressed(args.out, views=views[:length[k], k], names=np.array(names), samples=samples,
                        player_fields=np.array(VIEW_PLAYER), global_fields=np.array(VIEW_GLOBAL),
                        all_dealt=h0, all_taken=h1, picked=k, winner=winner[k])
    print("saved", args.out)


if __name__ == "__main__":
    main()
