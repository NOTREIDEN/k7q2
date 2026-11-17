# Parity rig

This is how the simulator is checked against the real game. The real 1.8.9
client is started with a few hook lines added, plays a scripted scene on its
integrated server, and writes down everything that happens on every tick.
The simulator then replays the recording and has to reproduce it bit for
bit.

No game code is stored here. `patch.py` adds the hook lines to a copy of
five decompiled sources that you provide; everything else is `Rig.java`.

## What is recorded

For every client tick: the packets the client handled before the tick, the
keys, clicks and mouse counts applied, the packets it sent, and afterwards
the local player (position, motion, angles, sprint state, timers, the
crosshair pick as nothing, the player or a block, whether it is blocking
with its sword, the right click delay) and the other player as this client
sees it, including whether it is shown blocking.

For every server tick: the packets handled from each player, the packets
sent to each player, and afterwards both players (position, motion, hurt
timers, last damage, fall distance, the movement check's state, the chunk
they are filed under, whether the server has them blocking) and their
entity tracker entries.

The packets written down are the ones a sword duel is made of: movement,
swings, attacks, sprint and sneak actions, the right click (`C08`, and the
two `C02` interact packets that go with it when the crosshair is on the
player), the release of the block (`C07`), and from the server the entity
tracker's packets, velocity, teleports, the flags byte and the sword being
sent again after a hit that wore it (`S2F`).

Doubles and floats are written as raw bits, so the comparison is exact.

The second player is scripted on the server: it has no client. Its packets
are injected on the server thread and what the server sends to it is only
written down. It can stand at a spot relative to the real player (`FRONT`),
walk towards a point (`GOTO`), jump, sprint, swing, attack and block.

## How the replay works

The two halves of a recording are replayed separately
(`sim/src/replay.rs`). The simulator's client gets the recorded server
packets and the recorded inputs and must end every tick in the recorded
state having sent the recorded packets. The simulator's server gets the
recorded client packets of both players and must end every tick in the
recorded state having sent the recorded packets. Neither half needs the
other's timing, so the thread scheduling of the real game does not matter.

    cd sim
    cargo run --release --example parity -- tests/traces/*.log.gz
    cargo test --release          # replays all of them as a test

`--resync` continues from the recorded state after a tick that differs, so
one difference is reported once instead of on every later tick. `--verbose`
prints every difference. `--wrong-knockback` runs with a knockback number
that is off by 2.5 percent and must fail; it shows that the comparison
notices small errors.

## Recording

Requirements: Linux, a Java 8 JDK, `xvfb`, the LWJGL 2.9.3 natives
(`liblwjgl-java-jni` on Debian and Ubuntu, in `/usr/lib/jni`), and an
MCP-919 workspace in which 1.8.9 has been decompiled and recompiled once, so
that it has `src/minecraft`, `bin/minecraft`, `jars/libraries` and
`jars/versions/1.8.9/1.8.9.jar`. Put `lwjgl-2.9.3.jar` and
`lwjgl_util-2.9.3.jar` (Maven Central, `org.lwjgl.lwjgl`) into `dl/`.

    export MCP=/path/to/mcp-919
    ./build.sh
    python3 gen_script.py fight 11 work/scripts/fight1.txt
    ./run.sh work/scripts/fight1.txt work/traces/fight1.log
    ./record_all.sh --keep      # every scene in scenarios.txt, stored as tests

The client runs under `xvfb` with software rendering. A 1000-tick scene
takes a bit over a minute.

`dec.py TRACE 'awk condition' [fields]` prints lines of a recording with the
hex numbers decoded, for reading one by hand.

## Scripts

One line per client tick, `C forward strafe jump sprint attack dx dy use`,
plus optional lines. `attack` is the number of left clicks in the tick,
`use` is 1 while the right button is held and may be left out:

| Line | Meaning |
|---|---|
| `start x y z yaw pitch` | where the real player starts (default `0.5 4 0.5 0 0`) |
| `fake x y z yaw pitch` | add the scripted second player there |
| `speed N` | Speed amplifier for both, `1` is Speed II (default), `-1` none |
| `sensitivity X` | mouse slider, 0 to 1 (default 0.5) |
| `unbreakable 1` | swords that do not wear, so the server never sends one again |
| `block x y z` | a stone block |
| `pen lo hi` | keep the scripted player's spots inside this square |
| `settle N` | ticks to wait before recording starts (default 100) |
| `F tick POS x y z yaw pitch ground` | scripted player: move there |
| `F tick FRONT dist angle height yaw pitch` | stand `dist` from the real player, `angle` degrees off its line of sight, facing it |
| `F tick GOTO x z step height yaw pitch` | walk towards a point, one step at most |
| `F tick SPRINT 0/1`, `SWING`, `ATTACK`, `GROUND 0/1` | the matching packet |
| `F tick USE`, `RELEASE` | scripted player: start and stop blocking |

Both players hold a diamond sword and get Resistance V and Saturation, so
hits do no damage and knock back in full, which is the infinite-health
setting of training stage one. The world is a flat grass floor at y = 4.

The first line of a recording says which format it is (`fmt=2` has the
right button, the block and the kind of crosshair pick) and whether the
swords wear. Recordings without it are older and are compared on what they
hold.

## Hooks

`patch.py` adds these to a copy of the decompiled sources:

- `Minecraft.runGameLoop`: a call before and after `runTick()`.
- `MinecraftServer.tick`: a call at the top and one after the world update.
- `PacketThreadUtil`: a call just before a packet is processed.
- `NetHandlerPlayServer.sendPacket` and `NetHandlerPlayClient.addToSendQueue`:
  a call at the top.
- `NetHandlerPlayClient.handlePlayerPosLook`: the answer to a server
  teleport is written down too (it is sent around `addToSendQueue`).

Inputs are applied through `KeyBinding` and `Entity.setAngles`, which is
what a keyboard, a mouse or a mod ends up calling.
