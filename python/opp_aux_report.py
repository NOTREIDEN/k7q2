"""How well a checkpoint's memory foresees the opponent (`pvpai/opp_aux.py`),
on duels it never learnt from.

    python opp_aux_report.py latest.pt [more.pt ...] [--rival other.pt] [--param name=value ...]

Each checkpoint plays two sets of duels with real health, potions and pearls
(against the scripted bots, or with `--rival` against that checkpoint): the
first to fit a probe, a fresh head of the same shape on the memory held
fixed; the second to judge both the probe and, where the checkpoint has one,
its own head. The probe says how much of the targets the memory holds
whether or not it was trained to, so checkpoints trained with and without
`--opp-aux-coef` compare on the same footing."""
import argparse
import json

import numpy as np
import torch
import torch.nn as nn

from pvpai import opp_aux
from pvpai.env import BOT, SELF_PLAY, VecEnv, sim_dims
from pvpai.model import OPP_AUX_HIDDEN, load_policy

# Duels as in the regression suite's common part: real health, potions and
# refills, pearls, Speed and hunger, ping 60-260 ms.
BASE = {"ep_ticks_min": 1200, "ep_ticks_max": 2400, "infinite_health": 0, "health_prob": 1, "start_hurt": 0,
        "refill_prob": 1, "inv_potions_min": 20, "inv_potions_max": 27, "potions_min": 6, "potions_max": 7,
        "pearl_prob": 1, "agent_ping_min_ms": 60, "agent_ping_max_ms": 260, "w_damage": 1, "kb_random": 0.7,
        "throw_carry_prob": 0.5, "hunger_prob": 1, "speed_end_prob": 0.5, "food_full": 20,
        "bot_level_min": 2, "bot_level_max": 4, "terrain_prob": 0, "retry_prob": 0}


@torch.no_grad()
def play(policy, rival, n, ticks, seed, params):
    """The memory's output, the value function's inputs, the ends of duels
    and the own head's guesses (or None) of player 0, [ticks, n, ...]."""
    env = VecEnv(n, SELF_PLAY if rival is not None else BOT, seed)
    for k, v in {**BASE, **params}.items():
        env.set_param(k, v)
    obs, priv = env.reset()
    st = policy.initial_state(n)
    rst = rival.initial_state(n) if rival is not None else None
    H, P, D, G = [], [], [], []
    torch.manual_seed(seed)
    own = getattr(policy, "opp_aux_dim", 0) > 0
    for _ in range(ticks):
        o = torch.from_numpy(obs.copy())
        h, st = policy.core(o, st)
        lp = policy.padded_logp(h)
        u = torch.rand_like(lp).clamp_(1e-20, 1.0)
        a = (lp - torch.log(-torch.log(u))).argmax(-1)
        H.append(h.half())
        P.append(torch.from_numpy(priv.copy()))
        if own:
            G.append(policy.opp_aux(h))
        env.step(a.numpy().astype(np.int32))
        if rival is not None:
            # The rival's tick (its memory restarts with a new duel); player 0 is next.
            keep1 = torch.from_numpy(env.done == 0).float().unsqueeze(1)
            rst = (rst[0] * keep1, rst[1] * keep1)
            ra, _, _, rst = rival.act(torch.from_numpy(env.obs.copy()), torch.from_numpy(env.priv.copy()), rst)
            env.step(ra.numpy().astype(np.int32))
        d = torch.from_numpy(env.done != 0)
        D.append(d)
        keep = (~d).float().unsqueeze(1)
        st = (st[0] * keep, st[1] * keep)
        obs, priv = env.obs, env.priv
    stats = env.take_stats()
    env.close()
    return torch.stack(H), torch.stack(P), torch.stack(D), (torch.stack(G) if own else None), stats


def fit_probe(h, y, m, hidden, iters, seed, batch=4096, lr=1e-3):
    torch.manual_seed(seed)
    g = 2 ** 0.5
    head = nn.Sequential(nn.Linear(hidden, OPP_AUX_HIDDEN), nn.ReLU(), nn.Linear(OPP_AUX_HIDDEN, opp_aux.DIM))
    nn.init.orthogonal_(head[0].weight, g)
    nn.init.zeros_(head[0].bias)
    nn.init.orthogonal_(head[2].weight, 0.01)
    nn.init.zeros_(head[2].bias)
    opt = torch.optim.Adam(head.parameters(), lr=lr)
    norm = opp_aux.norms(y, m)
    hf, yf, mf = h.reshape(-1, h.shape[-1]), y.reshape(-1, opp_aux.DIM), m.reshape(-1, opp_aux.DIM)
    N = hf.shape[0]
    for i in range(iters):
        idx = torch.randint(0, N, (batch,))
        loss, _ = opp_aux.loss(head(hf[idx].float()), yf[idx], mf[idx], norm)
        opt.zero_grad()
        loss.backward()
        opt.step()
    return head


@torch.no_grad()
def apply(head, h, chunk=65536):
    hf = h.reshape(-1, h.shape[-1])
    out = torch.cat([head(hf[i:i + chunk].float()) for i in range(0, hf.shape[0], chunk)])
    return out.view(*h.shape[:-1], opp_aux.DIM)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("checkpoints", nargs="+")
    ap.add_argument("--rival", default=None, help="checkpoint to play against instead of the bots")
    ap.add_argument("--n", type=int, default=64)
    ap.add_argument("--seconds", type=float, default=120.0)
    ap.add_argument("--seed", type=int, default=31)
    ap.add_argument("--probe-iters", type=int, default=3000)
    ap.add_argument("--threads", type=int, default=1)
    ap.add_argument("--param", action="append", default=[], help="name=value for the simulator")
    ap.add_argument("--json", default=None, help="write the numbers here too")
    args = ap.parse_args()
    torch.set_num_threads(args.threads)
    params = {k: float(v) for k, v in (kv.split("=") for kv in args.param)}
    dims = sim_dims()
    rival = load_policy(args.rival, dims, still=True)[0].eval() if args.rival else None
    ticks = int(args.seconds * 20)
    report = {}
    for path in args.checkpoints:
        policy = load_policy(path, dims)[0].eval()
        h_a, p_a, d_a, _, _ = play(policy, rival, args.n, ticks, args.seed, params)
        h_b, p_b, d_b, g_b, st = play(policy, rival, args.n, ticks, args.seed + 1, params)
        y_a, m_a = opp_aux.targets(p_a, d_a)
        y_b, m_b = opp_aux.targets(p_b, d_b)
        head = fit_probe(h_a, y_a, m_a, policy.hidden, args.probe_iters, args.seed)
        rows = {"probe": opp_aux.metrics(apply(head, h_b), y_b, m_b)}
        if g_b is not None:
            rows["own"] = opp_aux.metrics(g_b, y_b, m_b)
        mins = st["ticks0"] / 1200.0
        rows["play"] = dict(hits_per_min=st["hits0"] / max(mins, 1e-9), taken_per_min=st["hits1"] / max(mins, 1e-9),
                            deaths0=st["deaths0"], deaths1=st["deaths1"], episodes=st["episodes"])
        report[path] = rows
        print(f"== {path}  (play: hit/min {rows['play']['hits_per_min']:.1f} taken {rows['play']['taken_per_min']:.1f}, "
              f"{st['episodes']:.0f} duels ended)")
        for name, _, kind in opp_aux.GROUPS:
            for who in ("own", "probe"):
                r = rows.get(who, {}).get(name)
                if r is None:
                    continue
                if kind == "bce":
                    print(f"  {name:8s} {who:5s} share {r['share']:.3f} acc {r['acc']:.3f} (base {r['acc_base']:.3f}) "
                          f"auc {r['auc']:.3f} ce/base {r['ce_rel']:.3f}  n {r['n']}")
                else:
                    print(f"  {name:8s} {who:5s} mae {r['mae']:.4f} (mean guess {r['mae_base']:.4f}) r2 {r['r2']:.3f}  n {r['n']}")
    if args.json:
        with open(args.json, "w") as f:
            json.dump(report, f, indent=1)


if __name__ == "__main__":
    main()
