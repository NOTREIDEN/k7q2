# pvpai

A PvP agent for Minecraft 1.8.9, trained by self-play in a headless
simulator and meant to be driven from a client mod.

- `sim/`: the simulator (Rust). Two clients, one server and the network
  between them, ported from the 1.8.9 game code. See `sim/PARITY.md`.
- `python/`: the trainer (PyTorch, PPO with a recurrent policy).
- `parity/`: the rig that records the real game tick by tick. The
  simulator has to reproduce those recordings bit for bit
  (`sim/tests/parity.rs`).

## Build and test

    cd sim
    cargo test --release
    cargo run --release --example bench

## Train

    pip install torch numpy
    cd sim && cargo build --release && cd ../python
    python train.py --minutes 30 --out runs/stage1

Stage 1 trains sword duels with infinite health and Speed II: movement
keys, mouse, both mouse buttons, sprint resets, sword blocking and
knockback handling.

The agent is trained for the conditions it will play under: a ping of 160
to 230 ms and 14 to 17 clicks per second, against opponents with an
ordinary connection that click 20 to 24 times per second. These are the
defaults of the simulator (`agent_ping_*`, `cps_*`, `opp_cps_*` in
`sim/src/env.rs`) and can be changed with `--param name=value`.

Servers do not all push players the way the plain game does, and some
opponents have more than the game gives them. Training can mix both in:
`kb_random` is the share of duels with a server-style knockback (the
game's formula with other numbers, or the ground and air numbers, height
limit and time between hits that plugins of the KnockbackManager kind
set), and `opp_edge_prob` the share in which the opponent has a longer
reach, takes less knockback or can still hit the agent where it was a
moment ago (backtrack). The agent is not told which duel it is in.

The right mouse button blocks with the sword, as in the game: half the
damage of a hit, a slower walk, no sprint and no attacks while it is held,
so one tick of it is also a sprint reset. `w_damage` makes a hit count by
the damage it does (a critical hit 1.5 times, a blocked one a bit over
half) instead of every hit counting the same. `opp_autoblock` is the share
of opponents with an edge that the server always has blocking while they
attack freely, and `bot_block` scales how much the scripted bots block.

A hit wears the attacker's sword, the server sends the sword again, and a
client that is blocking at that moment drops its block without telling the
server (`sim/PARITY.md`). `sword_wear` sets how often that happens: 1 for
a plain sword, 0.25 for Unbreaking III, 0 for an unbreakable one, and by
default it is drawn for each duel.

Check a checkpoint against each scripted bot level:

    python evaluate.py runs/stage1/latest.pt

## Train on several machines

`.github/workflows/train.yml` starts a number of machines that all train
the same agent. Every few minutes each machine publishes its weights and
replaces them with the average of everybody's weights for that round
(`python/pvpai/sync.py`). The trainer only writes to and reads from two
folders; steps of the workflow carry the files between machines as
artifacts of the run (`.github/actions/round`).

The first run starts from `seed/stage1.pt`. Each run lasts under six hours,
leaves its weights in an artifact named `weights` and starts the next run,
which continues from them. To stop, cancel the run and add a file named
`STOP` to the repository. To continue later, start the workflow with the
id of the run to continue from.

While a run lasts, machine 0 pushes the average of all machines to the
branch `weights` every few rounds, with a few lines of its log. The branch
always holds a single commit:

    git fetch origin weights
    git show origin/weights:status.txt
    git show origin/weights:latest.pt > latest.pt
    python evaluate.py latest.pt

The `weights` artifact of a finished run holds one file per machine;
`python merge.py weights/w*.pt --out merged.pt` averages them the same way.

The same sharing works on one machine through a folder:

    python train.py --worker-id 0 --workers 2 --sync-dir /tmp/share &
    python train.py --worker-id 1 --workers 2 --sync-dir /tmp/share &
