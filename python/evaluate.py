"""Play a checkpoint against each scripted bot level and print how it did.

    python evaluate.py runs/stage1/latest.pt
"""
import argparse

import numpy as np
import torch

from pvpai.env import BOT, VecEnv, sim_dims
from pvpai.model import load_policy


def health(st):
    """With real health: the share of the duels ended by a death that the
    agent won, how long a duel lasted, potions thrown per duel by each
    side, and per potion the health it gave its thrower (eight at best),
    the health it could have given at the place it broke, and the health
    it gave the other player; then how the agent throws: the share of its
    potions that offered all eight points, the share thrown right after
    another, its health at the throw, the ticks its crosshair left the
    opponent for a potion, how many of them came after the throw, and the
    hits it took in the second after."""
    deaths = st["deaths0"] + st["deaths1"]
    duels = max(st["episodes"], 1)
    s0, s1 = max(st["splashes0"], 1), max(st["splashes1"], 1)
    t0 = max(st["potions0"], 1)
    return (f"  {st['deaths1']/max(deaths,1):4.2f}  {st['ticks0']/20/duels:6.1f}  {st['potions0']/duels:12.1f}  "
            f"{st['potions1']/duels:6.1f}  {st['heal_self0']/s0:11.1f}  {st['heal_self1']/s1:6.1f}  "
            f"{st['pot_offer0']/s0:5.1f}  {st['heal_gift0']/s0:4.1f}  {st['heal_gift1']/s1:6.1f}  "
            f"{st['pot_perfect0']/s0:4.2f}  {st['pot_double0']/t0:6.2f}  {st['pot_health0']/t0:9.1f}  "
            f"{st['pot_away0']/t0:10.1f}  {st['pot_after0']/t0:10.1f}  {st['pot_hit0']/t0:10.2f}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("checkpoint")
    ap.add_argument("--duels", type=int, default=64)
    ap.add_argument("--seconds", type=float, default=60.0)
    ap.add_argument("--levels", default="0,1,2,3,4")
    ap.add_argument("--greedy", action="store_true", help="always take the most likely action")
    ap.add_argument("--param", action="append", default=[])
    ap.add_argument("--health", action="store_true", help="real health, armour and healing potions; a duel ends with a death")
    ap.add_argument("--lib", default=None, help="path to libpvpsim.so")
    ap.add_argument("--no-block", action="store_true", help="the agent never holds the right button with its sword")
    args = ap.parse_args()

    policy, ck, _ = load_policy(args.checkpoint, sim_dims(args.lib))
    policy.eval()
    ticks = int(args.seconds * 20)
    print(f"checkpoint with {ck.get('samples', 0)/1e6:.1f}M samples; {args.duels} duels of {args.seconds:.0f} s per level")
    print("level  hits/min  taken/min  sprint-hit  on-target  aim-err  clicks/s  best-combo  blocking  blocked  dmg/min  dmg-taken/min"
          "  block-ticks  block-tap  kb/hit  pearls/min  their-pearls/min  first-hit-own  first-hit-theirs  shake-deg/s"
          + ("  wins  duel-s  potions/duel  theirs  heal/potion  theirs  reach  gift  theirs  full  double  at-health  away-ticks  back-ticks  hits-after" if args.health else ""))
    for level in [int(x) for x in args.levels.split(",")]:
        env = VecEnv(args.duels, BOT, 1234 + level, args.lib)
        env.set_param("bot_level_min", level)
        env.set_param("bot_level_max", level)
        env.set_param("ep_ticks_min", ticks)
        env.set_param("ep_ticks_max", ticks)
        if args.health:
            env.set_param("infinite_health", 0)
        for kv in args.param:
            k, v = kv.split("=")
            env.set_param(k, float(v))
        obs, priv = env.reset()
        state = policy.initial_state(args.duels)
        for _ in range(ticks):
            act, _, _, state = policy.act(torch.from_numpy(obs.copy()), torch.from_numpy(priv.copy()), state, args.greedy)
            act = act.numpy().astype(np.int32)
            if args.no_block:
                act[:, 7] = 0
            env.step(act)
            obs, priv = env.obs, env.priv
        st = env.take_stats()
        mins = st["ticks0"] / 1200.0
        print(f"{level:5d}  {st['hits0']/mins:8.1f}  {st['hits1']/mins:9.1f}  {st['sprint_hits0']/max(st['hits0'],1):10.2f}  "
              f"{st['on_target0']/st['ticks0']:9.2f}  {st['aim_err_sum0']/st['ticks0']:7.1f}  "
              f"{st['swings0']/st['ticks0']*20:8.1f}  {st['max_combo_sum0']/max(st['episodes'],1):10.1f}  "
              f"{st['use_ticks0']/st['ticks0']:8.2f}  {st['blocked0']/max(st['hits1'],1):7.2f}  "
              f"{st['damage0']/mins:7.0f}  {st['damage1']/mins:13.0f}  "
              # Ticks a block is held; share of hits dealt with sprint right after a block; speed a
              # hit gives the bot; pearls thrown by each side; and how often the agent got the first
              # hit in after its own pearl landed and after the bot's.
              f"{st['use_ticks0']/max(st['blocks0'],1):11.1f}  {st['tap_hits0']/max(st['hits0'],1):9.2f}  "
              f"{st['kb0']/max(st['hits0'],1):6.2f}  {st['pearls0']/mins:10.1f}  {st['pearls1']/mins:16.1f}  "
              f"{st['pearl_won0']/max(st['pearl_won0']+st['pearl_lost0'],1):13.2f}  "
              f"{st['pearl_lost1']/max(st['pearl_won1']+st['pearl_lost1'],1):16.2f}"
              # Degrees a second of turning the view that the next turn took back.
              f"  {st['shake0']/st['ticks0']*20:11.1f}"
              + (health(st) if args.health else ""))
        env.close()


if __name__ == "__main__":
    main()
