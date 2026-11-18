"""Train the agent: sword duels with Speed II, with infinite health or
with armour, real health and healing potions.

    python train.py --minutes 30 --out runs/stage1

Half of the duels are self-play, half are against scripted bots of mixed
strength; with --n-league some more are against earlier versions of the
agent (see pvpai/league.py). Checkpoints are written every few minutes and
can be resumed with --init."""
import argparse
import json
import os
import time

import numpy as np
import torch

from pvpai.env import BOT, SELF_PLAY, VecEnv
from pvpai.league import League
from pvpai.model import Policy, load_policy, restart_choices
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
    ap.add_argument("--teach-coef", type=float, default=0.0,
                    help="weight of imitating the teacher of the potions thrown with a turn (the sim's parameter teach)")
    ap.add_argument("--teach-full-until", type=float, default=0.0, help="samples, in millions, up to which that weight is whole")
    ap.add_argument("--teach-until", type=float, default=0.0, help="samples, in millions, at which it has come down to nothing")
    ap.add_argument("--turn-drill", type=float, default=0.0,
                    help="share of the agent's potions asked for that are thrown with a turn behind instead, to practise them")
    ap.add_argument("--n-league", type=int, default=0, help="duels against earlier versions of the agent")
    ap.add_argument("--league-dir", default=None, help="folder with the weights of those versions")
    ap.add_argument("--league-groups", type=int, default=4, help="versions played at the same time")
    ap.add_argument("--league-param", action="append", default=[], help="name=value for the duels against earlier versions only")
    ap.add_argument("--league-swap", type=int, default=40, help="iterations until the versions played are drawn again")
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
    # Against earlier versions the agent is the first player and the only
    # one that learns.
    groups = args.league_groups if args.n_league > 0 else 0
    per_group = max(args.n_league // max(groups, 1), 1)
    for gi in range(groups):
        envs[f"lg{gi}"] = VecEnv(per_group, SELF_PLAY, args.seed * 1000 + 10 + gi)
    any_env = next(iter(envs.values()))
    for e in envs.values():
        e.set_param("bot_level_min", args.bot_min)
        e.set_param("bot_level_max", args.bot_max)
        for kv in args.param:
            k, v = kv.split("=")
            e.set_param(k, float(v))
    for k, e in envs.items():
        if k.startswith("lg"):
            for kv in args.league_param:
                name, v = kv.split("=")
                e.set_param(name, float(v))

    def teach_coef(samples):
        """The weight of imitating the teacher at this many samples: whole,
        then down in a straight line to nothing."""
        m = samples / 1e6
        if args.teach_coef <= 0 or m >= args.teach_until:
            return 0.0
        if m <= args.teach_full_until:
            return args.teach_coef
        return args.teach_coef * (args.teach_until - m) / max(args.teach_until - args.teach_full_until, 1e-9)
    teaching = False

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
    marks = list(group.marks) if loaded else []
    if args.init and not loaded:
        old, ck, grown = load_policy(args.init, (any_env.obs_dim, any_env.priv_dim, any_env.act_dims))
        marks = list(ck.get("marks", []))
        policy.load_state_dict(old.state_dict())
        if not grown and "opt" in ck:
            opt.load_state_dict(ck["opt"])
        vs.var, vs.count = ck["vs"]
        # Every process counts its own share, so that the shares add up to
        # the checkpoint's count plus what is trained from here on.
        total_samples = ck["samples"] // (max(args.workers, 1) * max(args.machines, 1))
        iters = ck["iters"]

    # Choices that an earlier run let die start over, once: the weights say
    # which of these restarts they have had.
    fresh = restart_choices(policy, marks)
    if fresh and args.worker_id == 0:
        print("start: choices started over:", ", ".join(fresh), flush=True)
    for sy in (local, remote):
        if sy is not None:
            sy.marks = list(marks)

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
    league = None
    if groups:
        league = League(args.league_dir, (any_env.obs_dim, any_env.priv_dim, any_env.act_dims), groups, per_group, args.seed)
        league.refresh(policy)
        league.assign()
        for gi in range(groups):
            streams[f"lg{gi}"] = mk(f"lg{gi}", per_group)
        if args.worker_id == 0:
            print(f"start: league of {len(league.members)} earlier versions:", " ".join(n for n, _ in league.table()), flush=True)

    drill_rng = np.random.default_rng(args.seed * 1000 + args.machine_id * 37 + args.worker_id)

    def pick(k, obs, priv):
        """The agent's next action; player 1 of self-play is never drilled."""
        a = streams[k].choose(policy, obs, priv)
        if args.turn_drill > 0 and k != "sp1":
            a = streams[k].drill(a, args.turn_drill, drill_rng)
        return a

    # Player 0 of every duel is the first to act.
    acts = {}
    for k, e in envs.items():
        obs, priv = e.reset()
        k = "sp0" if k == "sp" else k
        acts[k] = pick(k, obs, priv)

    def machine_samples():
        return local.total_samples(total_samples) if local is not None else total_samples

    def save(path):
        # The count is that of everybody training this network, as far as this process knows.
        everybody = remote.total_samples(machine_samples()) if remote is not None else machine_samples()
        torch.save({"policy": policy.state_dict(), "opt": opt.state_dict(), "vs": (vs.var, vs.count),
                    "samples": everybody, "iters": iters, "args": vars(args), "marks": list(marks),
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
                if teaching:
                    streams["sp0"].taught(e.teach())
                streams["sp1"].close(policy, e.rew, e.done, e.fobs, e.fpriv, g, vsc, e.obs)
                a1 = streams["sp1"].choose(policy, e.obs, e.priv)
                e.step(a1)  # player 1 ticks; player 0 is next
                if teaching:
                    streams["sp1"].taught(e.teach())
                streams["sp1"].disown(e.overridden())
                streams["sp0"].close(policy, e.rew, e.done, e.fobs, e.fpriv, g, vsc, e.obs)
                acts["sp0"] = pick("sp0", e.obs, e.priv)
            if "bot" in envs:
                e = envs["bot"]
                e.step(acts["bot"])
                if teaching:
                    streams["bot"].taught(e.teach())
                streams["bot"].close(policy, e.rew, e.done, e.fobs, e.fpriv, g, vsc, e.obs)
                acts["bot"] = pick("bot", e.obs, e.priv)
            for gi in range(groups):
                k = f"lg{gi}"
                e = envs[k]
                e.step(acts[k])  # the agent ticks; the earlier version is next
                if teaching:
                    streams[k].taught(e.teach())
                e.step(league.act(gi, e.obs, e.priv, e.done))  # it ticks; the agent is next
                streams[k].close(policy, e.rew, e.done, e.fobs, e.fpriv, g, vsc, e.obs)
                acts[k] = pick(k, e.obs, e.priv)
        t1 = time.time()
        info = ppo_update(policy, opt, list(streams.values()), vs, cfg)
        # The teacher's weight for the next round of samples, and its
        # teacher on in the duels while it has any.
        cfg["teach_coef"] = teach_coef(remote.total_samples(machine_samples()) if remote is not None else machine_samples())
        if (cfg["teach_coef"] > 0) != teaching:
            teaching = cfg["teach_coef"] > 0
            for e in envs.values():
                e.set_param("teach", 1.0 if teaching else 0.0)
            if args.worker_id == 0:
                print(f"teacher {'on' if teaching else 'off'}", flush=True)
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
            if k.startswith("lg"):
                league.result(int(k[2:]), st["deaths1"], st["deaths0"])
                k = "lg"
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
                        # Degrees a second of turning the view that the next turn took back.
                        shake_deg_per_s=st["shake0"] / st["ticks0"] * 20,
                        max_combo=st["max_combo_sum0"] / st["episodes"],
                        # Share of own ticks spent blocking, share of the hits taken
                        # that landed on a block, and damage dealt and taken per minute.
                        block_frac=st["use_ticks0"] / st["ticks0"],
                        blocked_frac=st["blocked0"] / max(st["hits1"], 1.0),
                        crit_frac=st["crits0"] / max(st["hits0"], 1.0),
                        damage_per_min=st["damage0"] / mins0,
                        damage_taken_per_min=st["damage1"] / mins0,
                        # Ticks a block is held, share of the hits dealt that came with
                        # sprint right after a block, and the speed a hit gives the opponent.
                        block_ticks=st["use_ticks0"] / max(st["blocks0"], 1.0),
                        block_tap_frac=st["tap_hits0"] / max(st["hits0"], 1.0),
                        kb_per_hit=st["kb0"] / max(st["hits0"], 1.0),
                        # Pearls thrown per minute by each side, and who hit first after
                        # one landed: the thrower's share for player 0, the other's for player 1.
                        pearls_per_min=st["pearls0"] / mins0,
                        opp_pearls_per_min=st["pearls1"] / mins0,
                        pearl_first_hit=st["pearl_won0"] / max(st["pearl_won0"] + st["pearl_lost0"], 1.0),
                        opp_pearl_answered=st["pearl_lost1"] / max(st["pearl_won1"] + st["pearl_lost1"], 1.0),
                        # With real health: the share of the duels that ended with a death
                        # which player 0 won, deaths of either side per minute, potions per
                        # minute, and per potion the health it gave its thrower (eight at
                        # best) and the other player.
                        win_frac=st["deaths1"] / max(st["deaths0"] + st["deaths1"], 1.0),
                        # Survival drills: the share of them in which player 0 died.
                        drill_died=st.get("drill_deaths", 0.0) / max(st.get("drills", 0.0), 1.0),
                        drills=st.get("drills", 0.0),
                        deaths_per_min=(st["deaths0"] + st["deaths1"]) / mins0,
                        potions_per_min=st["splashes0"] / mins0,
                        opp_potions_per_min=st["splashes1"] / mins0,
                        potion_heal=st["heal_self0"] / max(st["splashes0"], 1.0),
                        potion_reach=st["pot_offer0"] / max(st["splashes0"], 1.0),
                        potion_gift=st["heal_gift0"] / max(st["splashes0"], 1.0),
                        opp_potion_heal=st["heal_self1"] / max(st["splashes1"], 1.0),
                        opp_potion_gift=st["heal_gift1"] / max(st["splashes1"], 1.0),
                        # How a potion is thrown: the share that offered all eight
                        # points, the share thrown right after another one, the health
                        # the thrower had, the ticks its crosshair left the opponent for
                        # one, and the hits it took in the second after.
                        potion_perfect=st["pot_perfect0"] / max(st["splashes0"], 1.0),
                        potion_double=st["pot_double0"] / max(st["potions0"], 1.0),
                        potion_at_health=st["pot_health0"] / max(st["potions0"], 1.0),
                        potion_away_ticks=st["pot_away0"] / max(st["potions0"], 1.0),
                        # Of those, the ticks after the throw until the crosshair is back,
                        # and the share of all ticks it is that far off anyway.
                        potion_back_ticks=st["pot_after0"] / max(st["potions0"], 1.0),
                        off_frac=st["off0"] / st["ticks0"],
                        potion_hits_taken=st["pot_hit0"] / max(st["potions0"], 1.0),
                        # Speed that runs out and hunger: drinks and meals finished per
                        # duel by each side, the share of the ones begun that were
                        # finished, the hits taken per one begun, and the share of the
                        # time with Speed and with food enough to sprint.
                        drinks_per_duel=st["drinks0"] / st["episodes"],
                        opp_drinks_per_duel=st["drinks1"] / st["episodes"],
                        meals_per_duel=st["meals0"] / st["episodes"],
                        opp_meals_per_duel=st["meals1"] / st["episodes"],
                        sip_done=(st["drinks0"] + st["meals0"]) / max(st["sips0"], 1.0),
                        sip_hits_taken=st["sip_hit0"] / max(st["sips0"], 1.0),
                        speed_frac=st["speed_on0"] / st["ticks0"],
                        opp_speed_frac=st["speed_on1"] / max(st["ticks1"], 1.0),
                        fed_frac=st["fed0"] / st["ticks0"],
                        # The inventory: potions got down from its rows per duel by each
                        # side, per visit, and the hits taken per visit.
                        refills_per_duel=st["refills0"] / st["episodes"],
                        opp_refills_per_duel=st["refills1"] / st["episodes"],
                        refills_per_visit=st["refills0"] / max(st["inv_opens0"], 1.0),
                        inv_ticks_per_visit=st["inv_ticks0"] / max(st["inv_opens0"], 1.0),
                        inv_hits_taken=st["inv_hit0"] / max(st["inv_opens0"], 1.0),
                    )
            window.clear()
            if league is not None:
                row["league"] = {name: round(beaten, 3) for name, beaten in league.table()}
        if league is not None and iters % args.league_swap == 0:
            league.refresh(policy)
            league.assign()
        log.write(json.dumps(row) + "\n")
        log.flush()
        if iters % args.log_every == 0 or iters == 1:
            b = row.get("bot", {})
            s = row.get("sp", {})
            q = row.get("lg", {})
            print(f"it {iters:4d} {total_samples/1e6:7.2f}M {row['sps']:6.0f}/s ent {info['entropy']:.2f} "
                  f"kl {info['kl']:.4f} r {info['mean_step_reward']:+.4f} | bot: hit/min {b.get('hits_per_min', 0):5.1f} "
                  f"taken {b.get('taken_per_min', 0):5.1f} on-target {b.get('on_target', 0):.2f} aim {b.get('aim_err_deg', 0):5.1f} "
                  f"sprint {b.get('sprint_hit_frac', 0):.2f} block {b.get('block_frac', 0):.2f} blocked {b.get('blocked_frac', 0):.2f} "
                  f"hold {b.get('block_ticks', 0):.1f} tap {b.get('block_tap_frac', 0):.2f} kb {b.get('kb_per_hit', 0):.2f} "
                  f"dmg {b.get('damage_per_min', 0):4.0f}/{b.get('damage_taken_per_min', 0):4.0f} "
                  f"pearl {b.get('pearls_per_min', 0):.1f}/{b.get('opp_pearls_per_min', 0):.1f} "
                  f"first {b.get('pearl_first_hit', 0):.2f}/{b.get('opp_pearl_answered', 0):.2f} "
                  f"win {b.get('win_frac', 0):.2f} deaths {b.get('deaths_per_min', 0):.1f} "
                  f"drill-died {b.get('drill_died', 0):.2f} of {b.get('drills', 0):.0f} "
                  f"pot {b.get('potions_per_min', 0):.1f}/{b.get('opp_potions_per_min', 0):.1f} "
                  f"heal {b.get('potion_heal', 0):.1f}/{b.get('opp_potion_heal', 0):.1f} "
                  f"reach {b.get('potion_reach', 0):.1f} gift {b.get('potion_gift', 0):.1f}/{b.get('opp_potion_gift', 0):.1f} "
                  f"full {b.get('potion_perfect', 0):.2f} x2 {b.get('potion_double', 0):.2f} at {b.get('potion_at_health', 0):.1f} "
                  f"away {b.get('potion_away_ticks', 0):.1f} back {b.get('potion_back_ticks', 0):.1f} off {b.get('off_frac', 0):.2f} "
                  f"hit {b.get('potion_hits_taken', 0):.2f} "
                  f"drink {b.get('drinks_per_duel', 0):.2f}/{b.get('opp_drinks_per_duel', 0):.2f} "
                  f"eat {b.get('meals_per_duel', 0):.2f}/{b.get('opp_meals_per_duel', 0):.2f} "
                  f"done {b.get('sip_done', 0):.2f} sip-hit {b.get('sip_hits_taken', 0):.2f} "
                  f"speed {b.get('speed_frac', 0):.2f}/{b.get('opp_speed_frac', 0):.2f} fed {b.get('fed_frac', 0):.2f} "
                  f"refill {b.get('refills_per_duel', 0):.2f}/{b.get('opp_refills_per_duel', 0):.2f} "
                  f"visit {b.get('refills_per_visit', 0):.1f} in {b.get('inv_ticks_per_visit', 0):.1f} "
                  f"inv-hit {b.get('inv_hits_taken', 0):.2f} "
                  f"| self: hit/min {s.get('hits_per_min', 0):5.1f} combo {s.get('max_combo', 0):.1f} "
                  f"aim {s.get('aim_err_deg', 0):5.1f} pot {s.get('potions_per_min', 0):.1f} "
                  f"heal {s.get('potion_heal', 0):.1f}"
                  + (f" | league: win {q.get('win_frac', 0):.2f} hit/min {q.get('hits_per_min', 0):5.1f} "
                     f"taken {q.get('taken_per_min', 0):5.1f} pearl {q.get('pearls_per_min', 0):.1f} "
                     f"pot {q.get('potions_per_min', 0):.1f} full {q.get('potion_perfect', 0):.2f} ["
                     + ", ".join(f"{name} {beaten:.2f}" for name, beaten in row.get("league", {}).items()) + "]"
                     if league is not None else ""), flush=True)
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
