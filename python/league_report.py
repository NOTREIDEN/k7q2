"""Play a checkpoint against every earlier version in a pool and print how
often it wins, oldest version first: whether the agent is still getting
better, and against which of its former selves it is not.

    python league_report.py runs/stage1/latest.pt pool/
    python league_report.py latest.pt pool/ --fair --duels 128

The checkpoint is the first player, with the connection and the clicks the
agent trains with; the earlier version is the second, with the opponent's.
`--fair` gives both the same connection and clicks and nobody an edge. The
last row is the checkpoint against itself."""
import argparse
import os

import numpy as np
import torch

from pvpai.env import SELF_PLAY, VecEnv, sim_dims
from pvpai.model import load_policy

FAIR = {"agent_ping_min_ms": -1, "opp_cps_min": -1, "opp_edge_prob": 0}


def duel(a, b, n, seconds, seed, params):
    """`n` duels of `a` against `b`; the simulator's counts."""
    env = VecEnv(n, SELF_PLAY, seed)
    ticks = int(seconds * 20)
    p = {"ep_ticks_min": ticks, "ep_ticks_max": ticks, "infinite_health": 0, "potions_min": 7, "potions_max": 7,
         "pearl_prob": 1, "pearl_cd_min_s": 8, "pearl_cd_max_s": 8, "opp_edge_prob": 0.5, "kb_random": 0.7}
    p.update(params)
    for k, v in p.items():
        env.set_param(k, v)
    obs, priv = env.reset()
    t = lambda x: torch.from_numpy(x).clone()
    sa, sb = a.initial_state(n), b.initial_state(n)

    def fresh(state):
        # A duel that has just started over starts without memories.
        keep = torch.from_numpy(env.done == 0).float().unsqueeze(1)
        return state[0] * keep, state[1] * keep

    for _ in range(ticks):
        act, _, _, sa = a.act(t(obs), t(priv), sa)
        env.step(act.numpy().astype(np.int32))
        act, _, _, sb = b.act(t(env.obs), t(env.priv), fresh(sb))
        env.step(act.numpy().astype(np.int32))
        obs, priv, sa = env.obs, env.priv, fresh(sa)
    st = env.take_stats()
    env.close()
    return st


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("checkpoint")
    ap.add_argument("pool")
    ap.add_argument("--duels", type=int, default=64)
    ap.add_argument("--seconds", type=float, default=60.0)
    ap.add_argument("--fair", action="store_true")
    ap.add_argument("--param", action="append", default=[])
    args = ap.parse_args()

    torch.manual_seed(3)
    dims = sim_dims()
    agent, ck, _ = load_policy(args.checkpoint, dims)
    agent.eval()
    params = dict(FAIR) if args.fair else {}
    params.update({k: float(v) for k, v in (kv.split("=") for kv in args.param)})
    names = sorted(f for f in os.listdir(args.pool) if f.endswith(".pt"))
    print(f"checkpoint with {ck.get('samples', 0)/1e6:.1f}M samples, {args.duels} duels at a time for {args.seconds:.0f} s"
          + (", same connection and clicks for both" if args.fair else ""))
    print("version    wins  duels  hits/min  taken/min  dmg/min  taken  potions/min  theirs  pearls/min  theirs")
    for i, name in enumerate(names + [None]):
        other = agent if name is None else load_policy(os.path.join(args.pool, name), dims, still=True)[0].eval()
        st = duel(agent, other, args.duels, args.seconds, 77 + i, params)
        ended = st["deaths0"] + st["deaths1"]
        mins = st["ticks0"] / 1200.0
        print(f"{(name or 'itself.pt')[:-3]:9s}  {st['deaths1']/max(ended, 1):4.2f}  {ended:5.0f}  {st['hits0']/mins:8.1f}  "
              f"{st['hits1']/mins:9.1f}  {st['damage0']/mins:7.0f}  {st['damage1']/mins:5.0f}  {st['potions0']/mins:11.1f}  "
              f"{st['potions1']/mins:6.1f}  {st['pearls0']/mins:10.1f}  {st['pearls1']/mins:6.1f}")


if __name__ == "__main__":
    main()
