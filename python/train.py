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
from pvpai.model import Policy, load_policy
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
    ap.add_argument("--worker-id", type=int, default=0, help="process number inside one machine")
    ap.add_argument("--workers", type=int, default=1, help="processes on this machine that share weights")
    ap.add_argument("--sync-dir", default=None, help="folder the processes of this machine share weights through")
    ap.add_argument("--sync-minutes", type=float, default=0.1)
    ap.add_argument("--sync-wait", type=float, default=5.0, help="seconds to wait for the others each round")
    ap.add_argument("--machine-id", type=int, default=0)
    ap.add_argument("--machines", type=int, default=1, help="machines that share weights")
    ap.add_argument("--machine-out", default=None, help="folder this machine publishes its weights in")
    ap.add_argument("--machine-in", default=None, help="folder the other machines' weights arrive in")
    ap.add_argument("--round-origin", type=float, default=0.0,
                    help="clock time (seconds) the machines count their rounds from")
    ap.add_argument("--machine-minutes", type=float, default=5.0)
    ap.add_argument("--machine-wait", type=float, default=60.0)
    ap.add_argument("--machine-mode", default="average", choices=["average", "best"],
                    help="average all machines, or let each train its own network and adopt the best when far behind")
    args = ap.parse_args()

    if args.threads > 0:
        torch.set_num_threads(args.threads)
    # Every machine starts from the same network (weights can only be
    # averaged between networks that share a starting point) but plays
    # different duels.
    base_seed = args.seed
    args.seed = args.seed + 7919 * args.worker_id + 104729 * args.machine_id
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

    torch.manual_seed(base_seed)
    policy = Policy(any_env.obs_dim, any_env.priv_dim, any_env.act_dims, args.hidden)
    torch.manual_seed(args.seed)
    opt = torch.optim.Adam(policy.parameters(), lr=args.lr, eps=1e-5)
    vs = RunningScale()
    total_samples, iters = 0, 0

    # Two levels of sharing: often between the processes of one machine,
    # every few minutes between machines. Only process 0 talks to the other
    # machines; what it brings back reaches the rest through the folder.
    local = remote = None
    loaded = False
    if args.sync_dir or args.machine_out:
        from pvpai.sync import DirBackend, Syncer
    if args.sync_dir:
        local = Syncer(DirBackend(args.sync_dir), args.worker_id, args.workers, args.sync_minutes, args.sync_wait)
    if args.machine_out:
        group = Syncer(DirBackend(args.machine_out, args.machine_in), args.machine_id, args.machines,
                       args.machine_minutes, args.machine_wait, args.machine_mode, origin=args.round_origin)
        # Weights the machines left behind last time come before --init.
        total_samples, iters, loaded = group.restore(policy, vs)
        total_samples //= max(args.workers, 1)
        if args.worker_id == 0:
            remote = group
            print(f"start: {'continuing from the weights of the last run' if loaded else 'no earlier weights'}, "
                  f"{total_samples/1e6:.1f}M samples so far", flush=True)
    if args.init and not loaded:
        old, ck, grown = load_policy(args.init, (any_env.obs_dim, any_env.priv_dim, any_env.act_dims))
        policy.load_state_dict(old.state_dict())
        if not grown and "opt" in ck:
            opt.load_state_dict(ck["opt"])
        vs.var, vs.count = ck["vs"]
        # Every process counts its own share, so that the shares add up to
        # the checkpoint's count plus what is trained from here on.
        total_samples = ck["samples"] // (max(args.workers, 1) * max(args.machines, 1))
        iters = ck["iters"]

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

    def machine_samples():
        return local.total_samples(total_samples) if local is not None else total_samples

    def save(path):
        # The count is that of everybody training this network, as far as this process knows.
        everybody = remote.total_samples(machine_samples()) if remote is not None else machine_samples()
        torch.save({"policy": policy.state_dict(), "opt": opt.state_dict(), "vs": (vs.var, vs.count),
                    "samples": everybody, "iters": iters, "args": vars(args),
                    "obs_dim": any_env.obs_dim, "priv_dim": any_env.priv_dim,
                    "act_dims": any_env.act_dims}, path)

    log = open(os.path.join(args.out, "log.jsonl"), "a")
    t_start = time.time()
    t_save = t_start
    window = {}
    score, sync_log = 0.0, {}
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
        for name, sy in (("machines", remote), ("processes", local)):
            if sy is None:
                continue
            # Other machines are told this machine's total, other processes this process's share.
            count = machine_samples() if sy is remote else total_samples
            did = sy.maybe_sync(policy, vs, count, iters, score)
            if did is None:
                continue
            what, changed = did
            if changed == "replaced":
                # An unrelated network: the optimiser's running averages no longer apply.
                opt = torch.optim.Adam(policy.parameters(), lr=args.lr, eps=1e-5)
                if sy is remote:
                    score = remote.adopted_score
            if changed and sy is remote and local is not None:
                local.new_generation(changed == "replaced")
            if sy is remote and not changed and what.startswith("averaged") and local is not None:
                local.new_generation(False)
            if sy is remote:
                # Rare enough to report each one as it happens.
                print(f"sync between machines, round {remote.round}: {what}; together "
                      f"{remote.total_samples(machine_samples())/1e6:.1f}M samples", flush=True)
            else:
                sync_log.setdefault(name, []).append(what)
        row = dict(info)
        row.update(iter=iters, samples=total_samples, minutes=(t2 - t_start) / 60,
                   sps=info["samples"] / (t2 - t0), rollout_s=t1 - t0, update_s=t2 - t1)
        for k, e in envs.items():
            st = e.take_stats()
            acc = window.setdefault(k, {})
            for name, v in st.items():
                acc[name] = acc.get(name, 0.0) + v
            if k == "bot" and st.get("ticks0", 0) > 0:
                # Hits landed minus hits taken per minute against the bots,
                # smoothed: how machines compare themselves with each other.
                score += 0.05 * ((st["hits0"] - st["hits1"]) / (st["ticks0"] / 1200.0) - score)
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
                        # Share of own ticks spent blocking, share of the hits taken
                        # that landed on a block, and damage dealt and taken per minute.
                        block_frac=st["use_ticks0"] / st["ticks0"],
                        blocked_frac=st["blocked0"] / max(st["hits1"], 1.0),
                        crit_frac=st["crits0"] / max(st["hits0"], 1.0),
                        damage_per_min=st["damage0"] / mins0,
                        damage_taken_per_min=st["damage1"] / mins0,
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
                  f"sprint {b.get('sprint_hit_frac', 0):.2f} block {b.get('block_frac', 0):.2f} blocked {b.get('blocked_frac', 0):.2f} "
                  f"dmg {b.get('damage_per_min', 0):4.0f}/{b.get('damage_taken_per_min', 0):4.0f} "
                  f"| self: hit/min {s.get('hits_per_min', 0):5.1f} combo {s.get('max_combo', 0):.1f} "
                  f"aim {s.get('aim_err_deg', 0):5.1f}", flush=True)
            for name, whats in sync_log.items():
                print(f"sync between {name}: {len(whats)} rounds, last: {whats[-1]}; together "
                      f"{local.total_samples(total_samples)/1e6:.1f}M samples", flush=True)
            sync_log.clear()
        now = time.time()
        if now - t_save > args.save_minutes * 60:
            save(os.path.join(args.out, "latest.pt"))
            t_save = now
        if (now - t_start) / 60 >= args.minutes:
            break
    save(os.path.join(args.out, "latest.pt"))
    if remote is not None:
        # What the next run continues from.
        remote.publish(policy, vs, machine_samples(), iters, score)
    print("saved", os.path.join(args.out, "latest.pt"), "samples", total_samples)


if __name__ == "__main__":
    main()
