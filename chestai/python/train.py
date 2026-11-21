"""Train the chest farmer with PPO.

    python train.py --minutes 30 --out runs/a

Stages make the game harder as the agent gets better at it: weak zombies
around a few close chests first; at the end every kind of mob, tougher
than any server's, with Slowness X, blindness, arrows, more knockback,
pings up to 300 ms and mountains, lakes and walls everywhere.

With --machines > 1 several trainers (one per machine) average their
weights every --round-minutes: each writes its weights to --share-dir,
runs --sync-cmd (which carries the files between machines), and averages
what it finds there from the same round.
"""
import argparse
import glob
import json
import os
import subprocess
import sys
import time

import numpy as np
import torch

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from chestai.env import VecEnv  # noqa: E402
from chestai.model import Policy, load_policy  # noqa: E402
from chestai.ppo import Stream, RunningScale, ppo_update  # noqa: E402
from chestai.levels import LevelBuffer  # noqa: E402

DENSE = dict(chests_min=4, chests_max=10, cluster_gap_min=6, cluster_gap_max=26, ready_share=0.6, init_timer_max_s=40,
             reset_cd_max_s=30, ep_ticks_min=3600, ep_ticks_max=6000, map_size=112, relief=20, clutter=7, mob_hp_max=50)

# Settings of each stage, on top of the simulator's defaults.
STAGES = [
    dict(name="zombies", chests_min=2, chests_max=4, cluster_gap_max=14, relief=5, water_prob=0.2, clutter=3,
         w_horseman=0, w_golem=0, w_rider=0, mob_hp_min=16, mob_hp_max=30, mob_dmg_max=1.0, mob_kbr_max=0.2,
         mob_kb_extra_max=0.2, slow_prob=0, blind_prob=0, extreme_share=0, ready_share=0.5, init_timer_max_s=40,
         ep_ticks_min=2400, ep_ticks_max=3600),
    dict(name="mixed", chests_min=3, chests_max=7, relief=10, water_prob=0.4, w_horseman=0.4, w_golem=0.4, w_rider=0.25,
         mob_hp_min=20, mob_hp_max=50, mob_dmg_max=2.0, slow_prob=0.08, blind_prob=0.05, extreme_share=0.1),
    # A real farm has many chests close together and most of them ready, so a
    # good player claims one every 15 to 20 seconds.
    dict(name="full", **DENSE),
    dict(DENSE, name="extreme", extreme_share=0.5, mob_hp_max=120, mob_dmg_max=4.0, mob_kb_extra_max=1.2, mob_kbr_max=0.8,
         slow_prob=0.25, blind_prob=0.2, proj_dmg_max=3.5, proj_punch_max=1.5),
]
# To move on from a stage: claims per minute of play at least this, and at
# most this share of the waves reset. (Every chest is claimed about once an
# episode at best, its cooldown being 3 minutes.)
PROMOTE = [(0.6, 0.45), (0.5, 0.55), (0.45, 0.6), None]


def stage_params(i, extra):
    p = {k: v for k, v in STAGES[i].items() if k != "name"}
    p.update(extra)
    return p


def pack(policy, vs, samples, iters, stage, args):
    return {
        "policy": {k: v.half() for k, v in policy.state_dict().items()},
        "obs_dim": policy.obs_dim,
        "priv_dim": policy.penc[0].in_features,
        "act_dims": policy.act_dims,
        "hidden": policy.hidden,
        "vs": (vs.var, vs.count),
        "samples": samples,
        "iters": iters,
        "stage": stage,
        "time": time.time(),
        "args": vars(args) if hasattr(args, "__dict__") and vars(args) else {},
    }


def average_into(policy, paths):
    sds = []
    for p in paths:
        try:
            ck = torch.load(p, map_location="cpu", weights_only=False)
            sds.append({k: v.float() for k, v in ck["policy"].items()})
        except Exception as e:  # a file half written by the sync
            print("skip", p, e, flush=True)
    if not sds:
        return 0
    own = policy.state_dict()
    sds.append({k: v.clone() for k, v in own.items()})
    policy.load_state_dict({k: torch.stack([sd[k] for sd in sds]).mean(0) for k in own})
    return len(sds)


EVAL_KINDS = ("full", "extreme", "rivals")


def evaluate_round(policy, args, rnd, samples, best, log, save):
    """The fixed test (evaluate.py) on the averaged network; the best one so far is kept as best.pt."""
    from evaluate import run
    t = time.time()
    policy.eval()
    res = {k: run(policy, k, args.eval_episodes, seed=900000 + 17 * j) for j, k in enumerate(EVAL_KINDS)}
    policy.train()
    score = sum(r["claims_per_min"] for r in res.values()) / len(res)
    rep = {"eval_round": rnd, "samples_M": round(samples / 1e6, 2), "score": round(score, 3), "secs": round(time.time() - t), **res}
    print(json.dumps(rep), flush=True)
    log.write(json.dumps(rep) + "\n")
    log.flush()
    if score > best:
        save(os.path.join(args.out, "best.pt"))
        return score
    return best


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--minutes", type=float, default=30)
    ap.add_argument("--out", default="runs/a")
    ap.add_argument("--init", default="")
    ap.add_argument("--n-envs", type=int, default=256)
    ap.add_argument("--horizon", type=int, default=128)
    ap.add_argument("--lr", type=float, default=3e-4)
    ap.add_argument("--gamma", type=float, default=0.995)
    ap.add_argument("--lam", type=float, default=0.95)
    ap.add_argument("--epochs", type=int, default=3)
    ap.add_argument("--minibatches", type=int, default=4)
    ap.add_argument("--clip", type=float, default=0.2)
    ap.add_argument("--ent", type=float, default=0.01)
    ap.add_argument("--vf", type=float, default=0.5)
    ap.add_argument("--threads", type=int, default=0)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--stage", type=int, default=-1, help="start stage (-1: from the checkpoint, else 0)")
    ap.add_argument("--max-stage", type=int, default=len(STAGES) - 1)
    ap.add_argument("--param", action="append", default=[], help="simulator parameter name=value, on top of the stage")
    ap.add_argument("--log-every", type=int, default=10)
    ap.add_argument("--save-minutes", type=float, default=5)
    ap.add_argument("--machine-id", type=int, default=0)
    ap.add_argument("--machines", type=int, default=1)
    ap.add_argument("--round-minutes", type=float, default=15)
    ap.add_argument("--share-dir", default="")
    ap.add_argument("--sync-cmd", default="", help="shell command run at each exchange; {round} is the round number")
    ap.add_argument("--vf-warmup", type=int, default=0, help="iterations at the start of this run that train only the value")
    ap.add_argument("--ref", default="", help="checkpoint the policy is kept near (KL), e.g. the imitation start")
    ap.add_argument("--ref-coef", type=float, default=0.0, help="weight of KL(reference || policy); halves every --ref-half iterations")
    ap.add_argument("--ref-half", type=float, default=0.0, help="iterations of this run over which the KL weight halves (0: constant)")
    ap.add_argument("--eval-episodes", type=int, default=0, help="machine 0 runs the fixed test with this many episodes a kind at each round (0: off)")
    ap.add_argument("--plr", type=float, default=0.0, help="share of episodes replayed from the level buffer (0: off)")
    args = ap.parse_args()

    if args.threads:
        torch.set_num_threads(args.threads)
    torch.manual_seed(args.seed + 1000 * args.machine_id)
    os.makedirs(args.out, exist_ok=True)
    extra = {}
    for kv in args.param:
        k, v = kv.split("=")
        extra[k] = float(v)

    vs = RunningScale()
    samples, iters, stage = 0, 0, 0
    if args.init and os.path.exists(args.init):
        policy, ck = load_policy(args.init)
        vs.var, vs.count = ck["vs"]
        samples, iters, stage = ck.get("samples", 0), ck.get("iters", 0), ck.get("stage", 0)
        print(f"start from {args.init}: {samples/1e6:.1f}M samples, stage {stage}", flush=True)
    else:
        policy = None
    # The KL weight halves, and the value warm-up counts, with the iterations of
    # this run, not with those of the checkpoint (2865 in start2.pt, where
    # --ref-half 400 had the weight at 0.03 / 2^7 from the start).
    iters0 = iters
    if args.stage >= 0:
        stage = args.stage
    stage = min(stage, args.max_stage)

    env = VecEnv(args.n_envs, seed=args.seed * 7919 + args.machine_id * 104729 + int(time.time()) % 100000)
    for k, v in stage_params(stage, extra).items():
        env.set_param(k, v)
    env.set_param("gamma", args.gamma)
    if policy is None:
        policy = Policy(env.obs_dim, env.priv_dim, env.act_dims)
    opt = torch.optim.Adam(policy.parameters(), lr=args.lr, eps=1e-5)
    cfg = dict(gamma=args.gamma, lam=args.lam, epochs=args.epochs, minibatches=args.minibatches, clip=args.clip,
               ent_coef=args.ent, vf_coef=args.vf, max_grad_norm=0.5)
    if args.ref and args.ref_coef > 0 and os.path.exists(args.ref):
        ref, _ = load_policy(args.ref)
        ref.eval()
        for q in ref.parameters():
            q.requires_grad_(False)
        cfg["ref"] = ref
    stream = Stream("main", args.n_envs, env.obs_dim, env.priv_dim, env.heads, args.horizon, policy.hidden)
    obs, priv = env.reset()
    n = args.n_envs
    plr = LevelBuffer(replay_prob=args.plr, seed=args.seed * 31 + args.machine_id) if args.plr > 0 else None
    cur = np.zeros(n, np.uint64)
    pending = np.zeros(n, np.uint64)
    seed_at = np.zeros((args.horizon, n), np.uint64)
    acc_s, acc_n = np.zeros(n), np.zeros(n)
    if plr is not None:
        for i in range(n):
            pending[i] = plr.sample()
            env.set_next_seed(i, pending[i])

    t0 = time.time()
    end = t0 + args.minutes * 60
    last_save = t0
    next_round = t0 + args.round_minutes * 60
    rnd = 0
    acc = {}
    log = open(os.path.join(args.out, "log.jsonl"), "a")

    def save(path):
        tmp = path + ".tmp"
        torch.save(pack(policy, vs, samples, iters, stage, args), tmp)
        os.replace(tmp, path)

    best = -1.0
    while time.time() < end:
        t_it = time.time()
        t = 0
        while not stream.full():
            act = stream.choose(policy, obs, priv)
            obs, priv, rew, done, fobs, fpriv = env.step(act)
            stream.close(policy, rew, done, fobs, fpriv, args.gamma, vs.scale)
            if plr is not None:
                seed_at[t] = cur
                for i in np.nonzero(done)[0]:
                    cur[i] = pending[i]
                    pending[i] = plr.sample()
                    env.set_next_seed(i, pending[i])
            t += 1
        # The last action is chosen so that its value closes the sums.
        stream.choose(policy, obs, priv)
        t_roll = time.time() - t_it
        cfg["pi_coef"] = 0.0 if iters - iters0 < args.vf_warmup else 1.0
        cfg["ref_coef"] = args.ref_coef * (0.5 ** ((iters - iters0) / args.ref_half) if args.ref_half > 0 else 1.0)
        st = ppo_update(policy, opt, [stream], vs, cfg)
        if plr is not None:
            # Score of a level: its mean positive advantage over the episode.
            pos = st["adv_raw"].clamp(min=0).numpy()
            dn = st["done"].numpy()
            for tt in range(pos.shape[0]):
                acc_s += pos[tt]
                acc_n += 1
                for i in np.nonzero(dn[tt])[0]:
                    if seed_at[tt, i] != 0:
                        plr.update(int(seed_at[tt, i]), acc_s[i] / acc_n[i])
                    acc_s[i] = acc_n[i] = 0
        # The pending action was only for its value: drop it, and keep the
        # memory from before it.
        stream.state = stream.pend[5]
        stream.pend = None
        samples += st["samples"]
        iters += 1
        for k, v in env.take_stats().items():
            if k == "max_simul":
                acc[k] = max(acc.get(k, 0), v)
            else:
                acc[k] = acc.get(k, 0) + v
        if iters % args.log_every == 0:
            mins = acc["ticks"] / 1200.0
            eps = max(acc["episodes"], 1)
            rep = {
                "iter": iters, "samples_M": round(samples / 1e6, 2), "stage": STAGES[stage]["name"],
                "claims_per_min": round(acc["claims"] / max(mins, 1e-9), 3),
                "kills_per_min": round(acc["kills"] / max(mins, 1e-9), 2),
                "reset_share": round(acc["resets"] / max(acc["waves"], 1), 3),
                "deaths_per_ep": round(acc["deaths"] / eps, 3),
                "taken_per_min": round(acc["dmg_taken"] / max(mins, 1e-9), 1),
                "tp_per_kill": round(acc["leash_tps"] / max(acc["kills"], 1), 2),
                "punch_per_min": round(acc["self_punch"] / max(mins, 1e-9), 2),
                "punch_paid_per_min": round(acc.get("punch_paid", 0) / max(mins, 1e-9), 2),
                "gapple_waste": round(acc["gapple_waste"] / max(acc["gapples"], 1), 2),
                "max_simul": acc.get("max_simul", 0),
                "sps": round(st["samples"] / (time.time() - t_it)), "roll_s": round(t_roll, 1),
                "ent": round(st["entropy"], 3), "kl": round(st["kl"], 4), "kl_ref": round(st["kl_ref"], 3), "ref_coef": round(cfg.get("ref_coef", 0.0), 5), "v_loss": round(st["v_loss"], 3),
                "rew": round(st["mean_step_reward"], 4),
            }
            if plr is not None:
                rep.update(plr.report())
            print(json.dumps(rep), flush=True)
            log.write(json.dumps(rep) + "\n")
            log.flush()
            pr = PROMOTE[stage]
            if pr is not None and stage < args.max_stage and rep["claims_per_min"] >= pr[0] and rep["reset_share"] <= pr[1] and rep["deaths_per_ep"] < 0.3:
                stage += 1
                for k, v in stage_params(stage, extra).items():
                    env.set_param(k, v)
                print(f"== stage {stage}: {STAGES[stage]['name']}", flush=True)
            acc = {}
        if time.time() - last_save > args.save_minutes * 60:
            save(os.path.join(args.out, "last.pt"))
            last_save = time.time()
        if args.machines > 1 and args.share_dir and (time.time() >= next_round or time.time() >= end):
            rnd += 1
            os.makedirs(args.share_dir, exist_ok=True)
            save(os.path.join(args.share_dir, f"m{args.machine_id}_r{rnd}.pt"))
            if args.sync_cmd:
                try:
                    subprocess.run(args.sync_cmd.format(round=rnd), shell=True, timeout=300)
                except subprocess.TimeoutExpired:
                    print("sync timed out", flush=True)
            others = [p for p in glob.glob(os.path.join(args.share_dir, f"m*_r{rnd}.pt")) if not os.path.basename(p).startswith(f"m{args.machine_id}_")]
            n = average_into(policy, others)
            # Stages move together: the furthest any machine got.
            for p in others:
                try:
                    stage = max(stage, min(torch.load(p, map_location="cpu", weights_only=False).get("stage", 0), args.max_stage))
                except Exception:
                    pass
            for k, v in stage_params(stage, extra).items():
                env.set_param(k, v)
            print(f"== round {rnd}: averaged {n} networks, stage {stage}", flush=True)
            if args.eval_episodes > 0 and args.machine_id == 0:
                best = evaluate_round(policy, args, rnd, samples, best, log, lambda p: save(p))
            next_round = time.time() + args.round_minutes * 60
    save(os.path.join(args.out, "last.pt"))
    if args.eval_episodes > 0 and args.machine_id == 0:
        evaluate_round(policy, args, rnd + 1, samples, best, log, lambda p: save(p))
    print("done", flush=True)


if __name__ == "__main__":
    main()
