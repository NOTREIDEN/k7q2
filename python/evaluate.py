"""Play a checkpoint against each scripted bot level and print how it did.

    python evaluate.py runs/stage1/latest.pt
"""
import argparse

import numpy as np
import torch

from pvpai.env import BOT, VecEnv
from pvpai.model import Policy


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("checkpoint")
    ap.add_argument("--duels", type=int, default=64)
    ap.add_argument("--seconds", type=float, default=60.0)
    ap.add_argument("--levels", default="0,1,2,3,4")
    ap.add_argument("--greedy", action="store_true", help="always take the most likely action")
    ap.add_argument("--param", action="append", default=[])
    args = ap.parse_args()

    ck = torch.load(args.checkpoint, map_location="cpu", weights_only=False)
    hidden = ck["policy"]["lstm.weight_hh_l0"].shape[1]
    policy = Policy(ck["obs_dim"], ck["priv_dim"], ck["act_dims"], hidden)
    policy.load_state_dict({k: v.float() for k, v in ck["policy"].items()})
    policy.eval()
    ticks = int(args.seconds * 20)
    print(f"checkpoint with {ck.get('samples', 0)/1e6:.1f}M samples; {args.duels} duels of {args.seconds:.0f} s per level")
    print("level  hits/min  taken/min  sprint-hit  on-target  aim-err  clicks/s  best-combo")
    for level in [int(x) for x in args.levels.split(",")]:
        env = VecEnv(args.duels, BOT, 1234 + level)
        env.set_param("bot_level_min", level)
        env.set_param("bot_level_max", level)
        env.set_param("ep_ticks_min", ticks)
        env.set_param("ep_ticks_max", ticks)
        for kv in args.param:
            k, v = kv.split("=")
            env.set_param(k, float(v))
        obs, priv = env.reset()
        state = policy.initial_state(args.duels)
        for _ in range(ticks):
            act, _, _, state = policy.act(torch.from_numpy(obs.copy()), torch.from_numpy(priv.copy()), state, args.greedy)
            env.step(act.numpy().astype(np.int32))
            obs, priv = env.obs, env.priv
        st = env.take_stats()
        mins = st["ticks0"] / 1200.0
        print(f"{level:5d}  {st['hits0']/mins:8.1f}  {st['hits1']/mins:9.1f}  {st['sprint_hits0']/max(st['hits0'],1):10.2f}  "
              f"{st['on_target0']/st['ticks0']:9.2f}  {st['aim_err_sum0']/st['ticks0']:7.1f}  "
              f"{st['swings0']/st['ticks0']*20:8.1f}  {st['max_combo_sum0']/max(st['episodes'],1):10.1f}")
        env.close()


if __name__ == "__main__":
    main()
