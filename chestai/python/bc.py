"""Start the network off by imitating the hand-written teacher.

    python bc.py --minutes 20 --out runs/bc/last.pt

DAgger: the environments are driven partly by the teacher and partly by
the network, and the teacher labels every state the network visits, so
the network also learns what to do after its own mistakes. The stages of
train.py are cycled so that it sees every kind of game.
"""
import argparse
import os
import sys
import time

import numpy as np
import torch

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from chestai.env import VecEnv  # noqa: E402
from chestai.model import Policy  # noqa: E402
from chestai.ppo import RunningScale  # noqa: E402
from train import STAGES, stage_params, pack  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--minutes", type=float, default=20)
    ap.add_argument("--out", default="runs/bc/last.pt")
    ap.add_argument("--n-envs", type=int, default=128)
    ap.add_argument("--T", type=int, default=64)
    ap.add_argument("--lr", type=float, default=5e-4)
    ap.add_argument("--threads", type=int, default=0)
    ap.add_argument("--stages", default="0,1,2")
    args = ap.parse_args()
    if args.threads:
        torch.set_num_threads(args.threads)
    stages = [int(x) for x in args.stages.split(",")]
    env = VecEnv(args.n_envs, seed=int(time.time()) % 100000)
    for k, v in stage_params(stages[0], {}).items():
        env.set_param(k, v)
    pol = Policy(env.obs_dim, env.priv_dim, env.act_dims)
    opt = torch.optim.Adam(pol.parameters(), lr=args.lr)
    obs, priv = env.reset()
    state = pol.initial_state(args.n_envs)
    t0 = time.time()
    it = 0
    beta = 1.0
    while time.time() - t0 < args.minutes * 60:
        frac = (time.time() - t0) / (args.minutes * 60)
        beta = max(0.3, 1.0 - frac)
        st_i = stages[it // 20 % len(stages)]
        if it % 20 == 0:
            for k, v in stage_params(st_i, {}).items():
                env.set_param(k, v)
        O = torch.zeros(args.T, args.n_envs, env.obs_dim)
        A = torch.zeros(args.T, args.n_envs, env.heads, dtype=torch.long)
        D = torch.zeros(args.T, args.n_envs, dtype=torch.bool)
        h0 = (state[0].clone(), state[1].clone())
        for t in range(args.T):
            o = torch.from_numpy(obs).clone()
            ta = env.teacher()
            pa, _, _, state = pol.act(o, torch.from_numpy(priv), state)
            pa = pa.numpy().astype(np.int32)
            use_t = np.random.rand(args.n_envs) < beta
            act = np.where(use_t[:, None], ta, pa)
            O[t], A[t] = o, torch.from_numpy(ta.astype(np.int64))
            obs, priv, rew, done, _, _ = env.step(act)
            d = torch.from_numpy(done != 0)
            D[t] = d
            if d.any():
                h, c = state
                h, c = h.clone(), c.clone()
                h[d] = 0
                c[d] = 0
                state = (h, c)
        hs = pol.core_seq(O, h0, D)
        loss = 0.0
        accs = []
        for i, lg in enumerate(pol.logits(hs)):
            loss = loss + torch.nn.functional.cross_entropy(lg.reshape(-1, lg.shape[-1]), A[..., i].reshape(-1))
            accs.append((lg.argmax(-1) == A[..., i]).float().mean().item())
        opt.zero_grad()
        loss.backward()
        torch.nn.utils.clip_grad_norm_(pol.parameters(), 1.0)
        opt.step()
        it += 1
        if it % 20 == 0:
            s = env.take_stats()
            mins = max(s["ticks"] / 1200.0, 1e-9)
            print(f"it {it} stage {STAGES[st_i]['name']} beta {beta:.2f} loss {loss.item():.3f} acc " + " ".join(f"{a:.2f}" for a in accs)
                  + f" | claims/min {s['claims']/mins:.2f} kills/min {s['kills']/mins:.1f} resets {s['resets']:.0f}/{s['waves']:.0f}", flush=True)
    os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)
    vs = RunningScale()

    class A_:
        pass
    torch.save(pack(pol, vs, 0, 0, stages[0], A_()), args.out)
    print("saved", args.out)


if __name__ == "__main__":
    main()
