"""A fixed test of a policy (or of the teacher): the same episodes every
time, from seeds that training never draws, in five kinds of game.

    python evaluate.py runs/a/last.pt [--episodes 48] [--teacher]

For each kind: claims per minute (mean and the mean of the worst tenth of
the episodes, CVaR 10 %), share of waves that reset, deaths per episode,
kills per minute and damage taken per minute.
"""
import argparse
import json
import os
import sys

import numpy as np
import torch

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from chestai.env import VecEnv, STAT_NAMES  # noqa: E402
from chestai.model import load_policy  # noqa: E402
from train import STAGES, stage_params  # noqa: E402

SUITE = {
    "zombies": (0, {}),
    "mixed": (1, {}),
    "full": (2, {"extreme_share": 0}),
    "extreme": (3, {"extreme_share": 1}),
    "rivals": (2, {"extreme_share": 0, "rival_prob": 1, "rivals_max": 3}),
}
I = {k: i for i, k in enumerate(STAT_NAMES)}


def run(policy, kind, episodes, seed, teacher=False):
    st, extra = SUITE[kind]
    env = VecEnv(episodes, seed=seed)
    for k, v in stage_params(st, extra).items():
        env.set_param(k, v)
    obs, priv = env.reset()
    env.take_stats()
    finished = [None] * episodes
    prev = [env.env_stats(i) for i in range(episodes)]
    state = policy.initial_state(episodes) if policy is not None else None
    while any(f is None for f in finished):
        if teacher:
            act = env.teacher()
        else:
            a, _, _, state = policy.act(torch.from_numpy(obs).clone(), torch.from_numpy(priv).clone(), state)
            act = a.numpy().astype(np.int32)
        obs, priv, rew, done, _, _ = env.step(act)
        for i in np.nonzero(done)[0]:
            now = env.env_stats(int(i))
            if finished[i] is None:
                finished[i] = now - prev[i]
            prev[i] = now
            if state is not None:
                state[0][i] = 0
                state[1][i] = 0
    s = np.stack(finished)
    mins = s[:, I["ticks"]] / 1200.0
    cpm = s[:, I["claims"]] / mins
    worst = np.sort(cpm)[: max(1, episodes // 10)]
    return {
        "claims_per_min": round(float(cpm.mean()), 3),
        "cvar10": round(float(worst.mean()), 3),
        "reset_share": round(float(s[:, I["resets"]].sum() / max(s[:, I["waves"]].sum(), 1)), 3),
        "deaths_per_ep": round(float(s[:, I["deaths"]].mean()), 3),
        "kills_per_min": round(float((s[:, I["kills"]] / mins).mean()), 2),
        "taken_per_min": round(float((s[:, I["dmg_taken"]] / mins).mean()), 1),
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("ckpt", nargs="?", default="")
    ap.add_argument("--episodes", type=int, default=48)
    ap.add_argument("--teacher", action="store_true")
    ap.add_argument("--kinds", default=",".join(SUITE))
    ap.add_argument("--threads", type=int, default=0)
    args = ap.parse_args()
    if args.threads:
        torch.set_num_threads(args.threads)
    pol = None
    if not args.teacher:
        pol, _ = load_policy(args.ckpt)
        pol.eval()
    out = {}
    for j, kind in enumerate(args.kinds.split(",")):
        # Seeds far from the ones training draws from the clock.
        out[kind] = run(pol, kind, args.episodes, seed=900000 + 17 * j, teacher=args.teacher)
        print(kind, json.dumps(out[kind]), flush=True)
    print(json.dumps(out))


if __name__ == "__main__":
    main()
