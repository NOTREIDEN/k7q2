"""Train the stage-1 agent: sword duels with infinite health and Speed II.

    python train.py --minutes 30 --out runs/stage1

Half of the duels are self-play, half are against scripted bots of mixed
strength. Checkpoints are written every few minutes and can be resumed with
--init."""
import argparse
import json
import os
import time

import numpy as np
import torch

from pvpai.env import BOT, SELF_PLAY, VecEnv
from pvpai.model import Policy
from pvpai.ppo import RunningScale, Stream, ppo_update


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="runs/stage1")
    ap.add_argument("--init", default=None, help="checkpoint to resume from")
    ap.add_argument("--minutes", type=float, default=30.0)
    ap.add_argument("--n-selfplay", type=int, default=192)
    ap.add_argument("--n-bot", type=int, default=192)
    ap.add_argument("--horizon", type=int, default=64)
    ap.add_argument("--hidden", type=int, default=256)
    ap.add_argument("--lr", type=float, default=3e-4)
    ap.add_argument("--ent-coef", type=float, default=0.0015)
    ap.add_argument("--gamma", type=float, default=0.995)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--threads", type=int, default=0)
    ap.add_argument("--bot-min", type=int, default=0)
    ap.add_argument("--bot-max", type=int, default=4)
    ap.add_argument("--save-minutes", type=float, default=5.0)
    ap.add_argument("--log-every", type=int, default=10)
    ap.add_argument("--minibatches", type=int, default=4)
    ap.add_argument("--epochs", type=int, default=3)
    ap.add_argument("--param", action="append", default=[], help="name=value for the simulator")
    ap.add_argument("--worker-id", type=int, default=0)
    ap.add_argument("--workers", type=int, default=1, help="how many machines share weights")
    ap.add_argument("--sync-dir", default=None, help="folder to share weights through")
    ap.add_argument("--sync-release", default=None, help="release tag to share weights through")
    ap.add_argument("--sync-minutes", type=float, default=5.0)
    args = ap.parse_args()

    if args.threads > 0:
        torch.set_num_threads(args.threads)
    args.seed = args.seed + 7919 * args.worker_id
    torch.manual_seed(args.seed)
    np.random.seed(args.seed)
    os.makedirs(args.out, exist_ok=True)

    envs = {}
    if args.n_selfplay > 0:
        envs["sp"] = VecEnv(args.n_selfplay, SELF_PLAY, args.seed * 1000 + 1)
    if args.n_bot > 0:
        envs["bot"] = VecEnv(args.n_bot, BOT, args.seed * 1000 + 2)
    any_env = next(iter(envs.values()))
    for e in envs.values():
        e.set_param("bot_level_min", args.bot_min)
        e.set_param("bot_level_max", args.bot_max)
        for kv in args.param:
            k, v = kv.split("=")
            e.set_param(k, float(v))

    policy = Policy(any_env.obs_dim, any_env.priv_dim, any_env.act_dims, args.hidden)
    opt = torch.optim.Adam(policy.parameters(), lr=args.lr, eps=1e-5)
    vs = RunningScale()
    total_samples, iters = 0, 0
    if args.init:
        ck = torch.load(args.init, map_location="cpu", weights_only=False)
        policy.load_state_dict(ck["policy"])
        opt.load_state_dict(ck["opt"])
        vs.var, vs.count = ck["vs"]
        total_samples, iters = ck["samples"], ck["iters"]

    syncer = None
    if args.sync_dir or args.sync_release:
        from pvpai.sync import DirBackend, ReleaseBackend, Syncer
        backend = DirBackend(args.sync_dir) if args.sync_dir else ReleaseBackend(os.environ["GITHUB_REPOSITORY"], args.sync_release)
        syncer = Syncer(backend, args.worker_id, args.workers, args.sync_minutes)
        if not args.init:
            total_samples, iters = syncer.restore(policy, vs)

    cfg = dict(gamma=args.gamma, lam=0.95, clip=0.2, epochs=args.epochs, minibatches=args.minibatches, vf_coef=0.5,
               ent_coef=args.ent_coef, max_grad_norm=0.5)
    heads = len(any_env.act_dims)
    mk = lambda name, n: Stream(name, n, any_env.obs_dim, any_env.priv_dim, heads, args.horizon, args.hidden)
    streams = {}
    if "sp" in envs:
        streams["sp0"] = mk("sp0", args.n_selfplay)
        streams["sp1"] = mk("sp1", args.n_selfplay)
    if "bot" in envs:
        streams["bot"] = mk("bot", args.n_bot)

    # Player 0 of every duel is the first to act.
    acts = {}
    for k, e in envs.items():
        obs, priv = e.reset()
        acts[k if k == "bot" else "sp0"] = streams[k if k == "bot" else "sp0"].choose(policy, obs, priv)

    def save(path):
        torch.save({"policy": policy.state_dict(), "opt": opt.state_dict(), "vs": (vs.var, vs.count),
                    "samples": total_samples, "iters": iters, "args": vars(args),
                    "obs_dim": any_env.obs_dim, "priv_dim": any_env.priv_dim,
                    "act_dims": any_env.act_dims}, path)

    log = open(os.path.join(args.out, "log.jsonl"), "a")
    t_start = time.time()
    t_save = t_start
    window = {}
    g, vsc = cfg["gamma"], None
    while True:
        t0 = time.time()
        while not all(s.full() for s in streams.values()):
            vsc = vs.scale
            if "sp" in envs:
                e = envs["sp"]
                e.step(acts["sp0"])  # player 0 ticks; player 1 is next
                streams["sp1"].close(policy, e.rew, e.done, e.fobs, e.fpriv, g, vsc)
                a1 = streams["sp1"].choose(policy, e.obs, e.priv)
                e.step(a1)  # player 1 ticks; player 0 is next
                streams["sp0"].close(policy, e.rew, e.done, e.fobs, e.fpriv, g, vsc)
                acts["sp0"] = streams["sp0"].choose(policy, e.obs, e.priv)
            if "bot" in envs:
                e = envs["bot"]
                e.step(acts["bot"])
                streams["bot"].close(policy, e.rew, e.done, e.fobs, e.fpriv, g, vsc)
                acts["bot"] = streams["bot"].choose(policy, e.obs, e.priv)
        t1 = time.time()
        info = ppo_update(policy, opt, list(streams.values()), vs, cfg)
        t2 = time.time()
        iters += 1
        total_samples += info["samples"]
        if syncer is not None:
            n_peers = syncer.maybe_sync(policy, vs, total_samples, iters)
            if n_peers is not None:
                print(f"sync: averaged with {n_peers} other machines, all machines together "
                      f"{syncer.total_samples(total_samples)/1e6:.1f}M samples", flush=True)
        row = dict(info)
        row.update(iter=iters, samples=total_samples, minutes=(t2 - t_start) / 60,
                   sps=info["samples"] / (t2 - t0), rollout_s=t1 - t0, update_s=t2 - t1)
        for k, e in envs.items():
            st = e.take_stats()
            acc = window.setdefault(k, {})
            for name, v in st.items():
                acc[name] = acc.get(name, 0.0) + v
        if iters % args.log_every == 0 or iters == 1:
            for k, st in window.items():
                if st.get("episodes", 0) > 0:
                    mins0 = st["ticks0"] / 1200.0
                    row[k] = dict(
                        episodes=st["episodes"],
                        hits_per_min=st["hits0"] / mins0,
                        taken_per_min=st["hits1"] / mins0,
                        sprint_hit_frac=st["sprint_hits0"] / max(st["hits0"], 1.0),
                        clicks_per_s=st["swings0"] / st["ticks0"] * 20,
                        click_on_target=st["attacks0"] / max(st["swings0"], 1.0),
                        on_target=st["on_target0"] / st["ticks0"],
                        aim_err_deg=st["aim_err_sum0"] / st["ticks0"],
                        max_combo=st["max_combo_sum0"] / st["episodes"],
                    )
            window.clear()
        log.write(json.dumps(row) + "\n")
        log.flush()
        if iters % args.log_every == 0 or iters == 1:
            b = row.get("bot", {})
            s = row.get("sp", {})
            print(f"it {iters:4d} {total_samples/1e6:7.2f}M {row['sps']:6.0f}/s ent {info['entropy']:.2f} "
                  f"kl {info['kl']:.4f} r {info['mean_step_reward']:+.4f} | bot: hit/min {b.get('hits_per_min', 0):5.1f} "
                  f"taken {b.get('taken_per_min', 0):5.1f} on-target {b.get('on_target', 0):.2f} aim {b.get('aim_err_deg', 0):5.1f} "
                  f"sprint {b.get('sprint_hit_frac', 0):.2f} | self: hit/min {s.get('hits_per_min', 0):5.1f} combo {s.get('max_combo', 0):.1f} "
                  f"aim {s.get('aim_err_deg', 0):5.1f}", flush=True)
        now = time.time()
        if now - t_save > args.save_minutes * 60:
            save(os.path.join(args.out, "latest.pt"))
            t_save = now
        if (now - t_start) / 60 >= args.minutes:
            break
    save(os.path.join(args.out, "latest.pt"))
    print("saved", os.path.join(args.out, "latest.pt"), "samples", total_samples)


if __name__ == "__main__":
    main()
