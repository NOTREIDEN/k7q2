# pvpai

A PvP agent for Minecraft 1.8.9, trained by self-play in a headless
simulator and meant to be driven from a client mod.

- `sim/`: the simulator (Rust). Two clients, one server and the network
  between them, ported from the 1.8.9 game code. See `sim/PARITY.md`.
- `python/`: the trainer (PyTorch, PPO with a recurrent policy).

## Build and test

    cd sim
    cargo test --release
    cargo run --release --example bench

## Train

    pip install torch numpy
    cd sim && cargo build --release && cd ../python
    python train.py --minutes 30 --out runs/stage1

Stage 1 trains sword duels with infinite health and Speed II: movement
keys, mouse, clicks, sprint resets and knockback handling.

Check a checkpoint against each scripted bot level:

    python evaluate.py runs/stage1/latest.pt

## Train on several machines

`.github/workflows/train.yml` starts a number of machines that all train
the same agent. Every few minutes each machine publishes its weights as an
asset of a release and replaces them with the average of everybody's latest
weights (`python/pvpai/sync.py`). Each run lasts under six hours and starts
the next one when it ends. To stop, cancel the run and add a file named
`STOP` to the repository.

The same sharing works on one machine through a folder:

    python train.py --worker-id 0 --workers 2 --sync-dir /tmp/share &
    python train.py --worker-id 1 --workers 2 --sync-dir /tmp/share &
