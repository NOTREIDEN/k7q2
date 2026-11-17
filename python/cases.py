"""Play a checkpoint through single cases of a duel with real health and
print how each one went.

    python cases.py runs/stage1/latest.pt
    python cases.py latest.pt --level 3 --duels 128

Each case fixes who starts hurt, how far apart the players start, who has
pearls, whose Speed is running out, who is hungry and who has potions in the
inventory; everything else is drawn as in training. A duel runs until a
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
# Speed that runs out and hunger. A player named in speed_who has the Speed
# and the potions of it that the case says; the other keeps Speed for good.
TIMED = dict(FULL, start_hurt=0, speed_end_prob=1, drinks_min=1, drinks_max=1)
NONE = dict(TIMED, speed_left_min_s=-1, speed_left_max_s=-1)
CASES += [
    ("Speed runs out in 3 to 8 s for both, two potions of it each, 10 blocks apart",
     dict(TIMED, speed_left_min_s=3, speed_left_max_s=8, drinks_min=2, drinks_max=2, start_dist_min=8, start_dist_max=12)),
    ("Speed runs out in 3 to 8 s for both, one potion of it each, 3 blocks apart",
     dict(TIMED, speed_left_min_s=3, speed_left_max_s=8, start_dist_min=2.5, start_dist_max=4)),
    ("agent without Speed and one potion of it, opponent with Speed on top of it",
     dict(NONE, speed_who=1, start_dist_min=2.5, start_dist_max=4)),
    ("agent without Speed and one potion of it, opponent with Speed 10 blocks away",
     dict(NONE, speed_who=1, start_dist_min=8, start_dist_max=12)),
    ("opponent without Speed and one potion of it, agent with Speed 10 blocks away",
     dict(NONE, speed_who=2, start_dist_min=8, start_dist_max=12)),
    ("both too hungry to sprint, 10 blocks apart",
     dict(FULL, start_hurt=0, hunger_prob=1, start_food_min=6, start_food_max=6, start_dist_min=8, start_dist_max=12)),
    ("both too hungry to sprint, 3 blocks apart",
     dict(FULL, start_hurt=0, hunger_prob=1, start_food_min=6, start_food_max=6, start_dist_min=2.5, start_dist_max=4)),
]
# The inventory. Nobody has a healing potion at hand and both have ten in
# the rows, or only the agent has (refill_who=1) and the opponent has none.
STOCK = dict(FULL, potions_min=0, potions_max=0, refill_prob=1, inv_potions_min=10, inv_potions_max=10, start_hurt=0)
CASES += [
    ("no healing potion at hand and ten in the inventory for both, 10 blocks apart",
     dict(STOCK, start_dist_min=8, start_dist_max=12)),
    ("the same, 3 blocks apart",
     dict(STOCK, start_dist_min=2.5, start_dist_max=4)),
    ("agent at 8 health, none at hand and ten in the inventory, opponent without any 10 blocks away",
     dict(STOCK, refill_who=1, start_hurt=1, hurt_who=1, start_health_min=8, start_health_max=8, start_dist_min=8, start_dist_max=12)),
    ("the same with the opponent on top of it",
     dict(STOCK, refill_who=1, start_hurt=1, hurt_who=1, start_health_min=8, start_health_max=8, start_dist_min=2.5, start_dist_max=4)),
    ("agent without Speed, its potion of it in the inventory, opponent with Speed 10 blocks away",
     dict(NONE, speed_who=1, potions_min=5, potions_max=5, drinks_min=0, drinks_max=0, refill_prob=1, refill_who=1,
          inv_potions_min=0, inv_potions_max=0, inv_drinks_min=1, inv_drinks_max=1, start_dist_min=8, start_dist_max=12)),
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("checkpoint")
    ap.add_argument("--level", type=int, default=4)
    ap.add_argument("--duels", type=int, default=96)
    ap.add_argument("--seconds", type=float, default=90.0)
    ap.add_argument("--param", action="append", default=[])
    ap.add_argument("--only", default=None, help="numbers of the cases to play, from 0, with commas")
    ap.add_argument("--lib", default=None, help="path to libpvpsim.so")
    args = ap.parse_args()

    policy, ck, _ = load_policy(args.checkpoint, sim_dims(args.lib))
    policy.eval()
    ticks = int(args.seconds * 20)
    only = None if args.only is None else {int(x) for x in args.only.split(",")}
    print(f"checkpoint with {ck.get('samples', 0)/1e6:.1f}M samples against bot level {args.level}; {args.duels} duels at a time for {args.seconds:.0f} s")
    for i, (name, params) in enumerate(CASES):
        if only is not None and i not in only:
            continue
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
              f" ({100*st['deaths1']/max(deaths,1):.0f} % of those that ended), {max(duels-deaths, 0):.0f} ran out of time")
        print(f"  agent: {st['potions0']/duels:.1f} potions a duel, thrown at {st['pot_health0']/thrown:.1f} health, "
              f"{st['heal_self0']/broke:.1f} of 8 healed each ({st['pot_offer0']/broke:.1f} offered, all 8 by {100*st['pot_perfect0']/broke:.0f} %), "
              f"{st['heal_gift0']/broke:.1f} given to the opponent")
        # The crosshair around a throw: off the opponent in the ten ticks before (and what
        # ten ticks of the duel have anyway), and the ticks after until it is back.
        usual = 10 * st["off0"] / max(st["ticks0"], 1)
        print(f"         {100*st['pot_double0']/thrown:.0f} % thrown right after another; crosshair off the opponent "
              f"{(st['pot_away0']-st['pot_after0'])/thrown:.1f} of the 10 ticks before ({usual:.1f} of any 10) and back on it "
              f"{st['pot_after0']/thrown:.1f} ticks after; {st['pot_hit0']/thrown:.2f} hits taken in the second after, {st['pearls0']/duels:.1f} pearls a duel")
        print(f"  bot:   {st['potions1']/duels:.1f} potions a duel, {st['heal_self1']/theirs:.1f} of 8 healed each, {st['pearls1']/duels:.1f} pearls a duel")
        if "speed_end_prob" in params or "hunger_prob" in params:
            begun = max(st["sips0"], 1)
            print(f"  agent: {st['drinks0']/duels:.2f} drinks and {st['meals0']/duels:.2f} meals a duel, {100*(st['drinks0']+st['meals0'])/begun:.0f} % "
                  f"of the {st['sips0']/duels:.2f} begun were finished, {st['sip_hit0']/begun:.2f} hits taken during one; "
                  f"Speed {100*st['speed_on0']/st['ticks0']:.0f} % of the time, able to sprint {100*st['fed0']/st['ticks0']:.0f} %")
            print(f"  bot:   {st['drinks1']/duels:.2f} drinks and {st['meals1']/duels:.2f} meals a duel, {st['sip_hit1']/max(st['sips1'],1):.2f} hits taken "
                  f"during one; Speed {100*st['speed_on1']/max(st['ticks1'],1):.0f} % of the time, able to sprint {100*st['fed1']/max(st['ticks1'],1):.0f} %")
        if "refill_prob" in params:
            for who, k in (("agent", "0"), ("bot:  ", "1")):
                visits = max(st["inv_opens" + k], 1)
                print(f"  {who} {st['refills' + k]/duels:.2f} potions a duel from the inventory in {st['inv_opens' + k]/duels:.2f} visits: "
                      f"{st['refills' + k]/visits:.1f} a visit, {st['inv_ticks' + k]/visits:.1f} ticks each, {st['inv_hit' + k]/visits:.2f} hits taken meanwhile")


if __name__ == "__main__":
    main()
