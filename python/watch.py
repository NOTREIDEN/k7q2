"""Print what a checkpoint does tick by tick in one duel against a bot.

    python watch.py runs/stage1/latest.pt --level 2 --ticks 200

Each line shows the keys of both players (W/S, A/D, Jump, sprint Ctrl,
attack X), whether they stand on the ground, sprint and are hurt, and three
distances: the true one, the one the agent sees and the one the bot sees.
Reading a few exchanges this way shows habits that the averages hide, for
example pressing into the opponent and trading hit for hit."""
import argparse

import numpy as np
import torch

from pvpai.env import BOT, VIEW_PLAYER, VecEnv
from record import load


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("checkpoint")
    ap.add_argument("--level", type=int, default=2)
    ap.add_argument("--ticks", type=int, default=200)
    ap.add_argument("--seed", type=int, default=3)
    ap.add_argument("--greedy", action="store_true")
    ap.add_argument("--param", action="append", default=[])
    args = ap.parse_args()

    policy, _ = load(args.checkpoint)
    env = VecEnv(1, BOT, args.seed)
    env.set_param("bot_level_min", args.level)
    env.set_param("bot_level_max", args.level)
    for kv in args.param:
        k, v = kv.split("=")
        env.set_param(k, float(v))
    obs, priv = env.reset()
    state = policy.initial_state(1)
    n = len(VIEW_PLAYER)
    f = {k: i for i, k in enumerate(VIEW_PLAYER)}

    def keys(p):
        fw, st = p[f["forward"]], p[f["strafe"]]
        return (("W" if fw > 0 else "S" if fw < 0 else ".") + ("A" if st > 0 else "D" if st < 0 else ".")
                + ("J" if p[f["jump"]] else ".") + ("C" if p[f["sprint_key"]] else ".")
                + ("X" if p[f["attack"]] else "."))

    def flat(p, q):
        return float(np.hypot(p[0] - q[0], p[2] - q[2]))

    hits = [0.0, 0.0]
    for t in range(args.ticks):
        act, _, _, state = policy.act(torch.from_numpy(obs.copy()), torch.from_numpy(priv.copy()), state, args.greedy)
        env.step(act.numpy().astype(np.int32))
        obs, priv = env.obs, env.priv
        v = env.view(0)
        a, b = v[:n], v[n:2 * n]
        sx = f["seen_x"]
        note = ""
        if a[f["hits"]] != hits[0]:
            note += "  AGENT HITS"
        if b[f["hits"]] != hits[1]:
            note += "  bot hits"
        hits = [a[f["hits"]], b[f["hits"]]]
        print(f"{t:4d} agent {keys(a)} ground {int(a[f['on_ground']])} sprint {int(a[f['sprinting']])} "
              f"hurt {int(a[f['hurt_time']]):2d} | true {flat(a, b):5.2f} agent sees {flat(a, a[sx:sx + 3]):5.2f} "
              f"bot sees {flat(b, b[sx:sx + 3]):5.2f} | bot {keys(b)} ground {int(b[f['on_ground']])} "
              f"hurt {int(b[f['hurt_time']]):2d}{note}")
    env.close()


if __name__ == "__main__":
    main()
