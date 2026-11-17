"""Write a checkpoint's policy as the file the mod reads.

    python export_policy.py latest.pt policy.bin --check check.bin

The mod needs the part of the network that chooses actions: the two layers
that read the observation, the memory, and one layer per control. The file
is those weights as little-endian 32-bit floats after a short header (see
`write_policy`); `mod/core/src/main/java/pvpai/core/Policy.java` reads it.

`--check` also writes the observations of a short duel with the
preferences the network gives on each tick, for the mod's own arithmetic
to be compared with:

    java -cp mod/core/out pvpai.core.PolicyCheck policy.bin check.bin"""
import argparse
import struct

import numpy as np
import torch

from pvpai.env import BOT, VecEnv, sim_dims
from pvpai.model import NEVER, load_policy


def write_policy(policy, samples, path):
    """`PVPP`, format 1, observation size, memory size, number of controls,
    choices of each, samples trained on (64 bits); then, row by row, the
    weights and offsets of the two reading layers, of the memory (input
    weights, memory weights, both offsets, gates in the order in, forget,
    cell, out) and of each control."""
    sd = policy.state_dict()
    names = ["enc.0.weight", "enc.0.bias", "enc.2.weight", "enc.2.bias",
             "lstm.weight_ih_l0", "lstm.weight_hh_l0", "lstm.bias_ih_l0", "lstm.bias_hh_l0"]
    for i in range(len(policy.act_dims)):
        names += [f"heads.{i}.weight", f"heads.{i}.bias"]
    with open(path, "wb") as f:
        f.write(b"PVPP")
        f.write(struct.pack("<4i", 1, sd["enc.0.weight"].shape[1], policy.hidden, len(policy.act_dims)))
        f.write(struct.pack(f"<{len(policy.act_dims)}i", *policy.act_dims))
        f.write(struct.pack("<q", int(samples)))
        for n in names:
            f.write(sd[n].detach().cpu().numpy().astype("<f4").tobytes())


def write_check(policy, path, ticks=300, seed=3):
    """`PVPC`, format 1, ticks, observation size, preferences per tick;
    then for each tick 1 if a new duel starts with it and the memory is
    emptied first (0 if not), the observation, and the preferences of
    every control one after the other."""
    env = VecEnv(1, BOT, seed)
    for k, v in dict(ep_ticks_min=ticks // 3, ep_ticks_max=ticks // 3, infinite_health=0, health_prob=1,
                     start_hurt=1, bot_level_min=4, bot_level_max=4).items():
        env.set_param(k, v)
    obs, priv = env.reset()
    state = policy.initial_state(1)
    torch.manual_seed(seed)
    rows, fresh = [], 1.0
    with torch.no_grad():
        for _ in range(ticks):
            if fresh:
                state = policy.initial_state(1)
            o = torch.from_numpy(obs.copy())
            h, state = policy.core(o, state)
            logits = policy.logits(h)
            rows.append(np.concatenate([[fresh], obs[0]] + [lg[0].numpy() for lg in logits]).astype("<f4"))
            act = torch.stack([torch.distributions.Categorical(logits=lg).sample() for lg in logits], -1)
            env.step(act.numpy().astype(np.int32))
            fresh = float(env.done[0] != 0)
            obs, priv = env.obs, env.priv
    env.close()
    with open(path, "wb") as f:
        f.write(b"PVPC")
        f.write(struct.pack("<4i", 1, ticks, policy.enc[0].in_features, sum(policy.act_dims)))
        for r in rows:
            f.write(r.tobytes())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("checkpoint")
    ap.add_argument("out")
    ap.add_argument("--check", default=None)
    ap.add_argument("--rest", default="", help="controls, by number, that stay at their first choice in the game, for one the weights are still learning")
    args = ap.parse_args()
    # A control the weights were trained without stays at rest in the game.
    policy, ck, _ = load_policy(args.checkpoint, sim_dims(), still=True)
    policy.eval()
    with torch.no_grad():
        for h in [int(x) for x in args.rest.replace(",", " ").split()]:
            policy.heads[h].weight.zero_()
            policy.heads[h].bias.fill_(NEVER)
            policy.heads[h].bias[0] = 0.0
            print(f"control {h} stays at rest")
    write_policy(policy, ck.get("samples", 0), args.out)
    print(f"{args.out}: {ck.get('samples', 0) / 1e6:.0f}M samples, {policy.enc[0].in_features} inputs, controls {policy.act_dims}")
    if args.check:
        write_check(policy, args.check)
        print(f"{args.check}: 300 ticks of duels")


if __name__ == "__main__":
    main()
