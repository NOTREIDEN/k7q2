"""One episode of a policy (or of the teacher), written out for the viewer:
the map from above and the true state of every other tick.

    python watch.py runs/a/last.pt --kind full --seed 3 --out episode.json
"""
import argparse
import json
import os
import sys

import numpy as np
import torch

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from chestai.env import VecEnv  # noqa: E402
from chestai.model import load_policy  # noqa: E402
from evaluate import SUITE  # noqa: E402
from train import stage_params  # noqa: E402


def episode(policy, kind, seed, every=2, max_ticks=4000, teacher=False):
    st, extra = SUITE[kind]
    env = VecEnv(1, seed=seed)
    for k, v in stage_params(st, extra).items():
        env.set_param(k, v)
    obs, priv = env.reset()
    world = json.loads(env.json(0, 0))
    frames = [json.loads(env.json(0, 1))]
    state = policy.initial_state(1) if policy is not None else None
    for t in range(max_ticks):
        if teacher:
            act = env.teacher()
        else:
            a, _, _, state = policy.act(torch.from_numpy(obs).clone(), torch.from_numpy(priv).clone(), state)
            act = a.numpy().astype(np.int32)
        last = json.loads(env.json(0, 1))
        obs, priv, rew, done, _, _ = env.step(act)
        if done[0]:
            frames.append(last)
            break
        if t % every == 0:
            frames.append(json.loads(env.json(0, 1)))
    return {"kind": kind, "seed": seed, "every": every, "map": world, "frames": frames}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("ckpt", nargs="?", default="")
    ap.add_argument("--kind", default="full")
    ap.add_argument("--seed", type=int, default=3)
    ap.add_argument("--teacher", action="store_true")
    ap.add_argument("--out", default="episode.json")
    args = ap.parse_args()
    torch.set_num_threads(1)
    pol = None
    if not args.teacher:
        pol, _ = load_policy(args.ckpt)
        pol.eval()
    ep = episode(pol, args.kind, args.seed, teacher=args.teacher)
    with open(args.out, "w") as f:
        json.dump(ep, f, separators=(",", ":"))
    last = ep["frames"][-1]
    print(f"{len(ep['frames'])} frames, {last['claims']:.0f} claims, {last['kills']:.0f} kills, {last['resets']:.0f} resets -> {args.out}")


if __name__ == "__main__":
    main()
