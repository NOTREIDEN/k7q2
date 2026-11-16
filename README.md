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
