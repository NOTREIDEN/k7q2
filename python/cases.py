"""Play a checkpoint through single cases of a duel with real health and
print how each one went.

    python cases.py runs/stage1/latest.pt
    python cases.py latest.pt --level 3 --duels 128

Each case fixes who starts hurt, how far apart the players start and who
has pearls; everything else is drawn as in training. A duel runs until a
player dies or the time is up."""
import argparse

import numpy as np
import torch

from pvpai.env import BOT, VecEnv, sim_dims
from pvpai.model import load_policy

FULL = {"infinite_health": 0, "potions_min": 7, "potions_max": 7, "pearl_prob": 1, "pearl_cd_min_s": 8, "pearl_cd_max_s": 8}
CASES = [
    ("even start, potions and pearls for both",
     dict(FULL, start_hurt=0, start_dist_min=5, start_dist_max=10)),
    ("agent at 6 health, opponent on top of it, no pearls",
     dict(FULL, start_hurt=1, hurt_who=1, start_health_min=6, start_health_max=6, start_dist_min=2.5, start_dist_max=4,
          pearl_prob=0)),
    ("agent at 6 health, opponent 8 blocks away with pearls",
     dict(FULL, start_hurt=1, hurt_who=1, start_health_min=6, start_health_max=6, start_dist_min=7, start_dist_max=10,
          pearls_who=2)),
    ("opponent at 6 health and healing, agent 10 blocks away with pearls",
     dict(FULL, start_hurt=1, hurt_who=2, start_health_min=6, start_health_max=6, start_dist_min=8, start_dist_max=12,
          pearls_who=1)),
    ("both at 6 health, potions and pearls for both",
     dict(FULL, start_hurt=1, start_health_min=6, start_health_max=6, start_dist_min=4, start_dist_max=8)),
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("checkpoint")
    ap.add_argument("--level", type=int, default=4)
    ap.add_argument("--duels", type=int, default=96)
    ap.add_argument("--seconds", type=float, default=90.0)
    ap.add_argument("--param", action="append", default=[])
    ap.add_argument("--lib", default=None, help="path to libpvpsim.so")
    args = ap.parse_args()

    policy, ck, _ = load_policy(args.checkpoint, sim_dims(args.lib))
    policy.eval()
    ticks = int(args.seconds * 20)
    print(f"checkpoint with {ck.get('samples', 0)/1e6:.1f}M samples against bot level {args.level}; {args.duels} duels at a time for {args.seconds:.0f} s")
    for i, (name, params) in enumerate(CASES):
        torch.manual_seed(100 + i)
        env = VecEnv(args.duels, BOT, 4321 + i, args.lib)
        p = {"bot_level_min": args.level, "bot_level_max": args.level, "ep_ticks_min": ticks, "ep_ticks_max": ticks,
             "start_yaw_noise": 25}
        p.update(params)
        for kv in args.param:
            k, v = kv.split("=")
            p[k] = float(v)
        for k, v in p.items():
            env.set_param(k, v)
        obs, priv = env.reset()
        state = policy.initial_state(args.duels)
        for _ in range(ticks):
            act, _, _, state = policy.act(torch.from_numpy(obs.copy()), torch.from_numpy(priv.copy()), state, False)
            env.step(act.numpy().astype(np.int32))
            obs, priv = env.obs, env.priv
        st = env.take_stats()
        env.close()
        duels = max(st["episodes"], 1)
        deaths = st["deaths0"] + st["deaths1"]
        thrown, broke, theirs = max(st["potions0"], 1), max(st["splashes0"], 1), max(st["splashes1"], 1)
        print(f"\n{name}")
        print(f"  {duels:.0f} duels, {st['ticks0']/20/duels:.1f} s each; the agent won {st['deaths1']:.0f}, lost {st['deaths0']:.0f}"
              f" ({100*st['deaths1']/max(deaths,1):.0f} % of those that ended), {duels-deaths:.0f} ran out of time")
        print(f"  agent: {st['potions0']/duels:.1f} potions a duel, thrown at {st['pot_health0']/thrown:.1f} health, "
              f"{st['heal_self0']/broke:.1f} of 8 healed each ({st['pot_offer0']/broke:.1f} offered, all 8 by {100*st['pot_perfect0']/broke:.0f} %), "
              f"{st['heal_gift0']/broke:.1f} given to the opponent")
        print(f"         {100*st['pot_double0']/thrown:.0f} % thrown right after another, crosshair off the opponent {st['pot_away0']/thrown:.1f} ticks "
              f"a potion, {st['pot_hit0']/thrown:.2f} hits taken in the second after, {st['pearls0']/duels:.1f} pearls a duel")
        print(f"  bot:   {st['potions1']/duels:.1f} potions a duel, {st['heal_self1']/theirs:.1f} of 8 healed each, {st['pearls1']/duels:.1f} pearls a duel")


if __name__ == "__main__":
    main()
