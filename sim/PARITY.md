# Parity with Minecraft 1.8.9

The simulator is a port of the decompiled 1.8.9 client and server (MCP 9.19
names), method by method. No game code is copied into this repository; the
table lists which game method each part follows so it can be re-checked.

## What is ported

| Simulator | Game method |
|---|---|
| `mth::sin`, `mth::cos`, `look_vec` | `MathHelper.sin/cos` (65536-entry table), `Entity.getVectorForRotation` |
| `Aabb::calc_*_offset`, `calculate_intercept` | `AxisAlignedBB.calculateXOffset/YOffset/ZOffset`, `calculateIntercept` |
| `World::colliding_boxes`, `collides` | `World.getCollidingBoundingBoxes`: one box per solid block that overlaps the query box |
| `Body::move_entity` | `Entity.moveEntity` (collision order Y, X, Z; ground flag; motion zeroing) |
| `Attr::value` | `ModifiableAttributeInstance.computeValue` for `generic.movementSpeed`, sprint modifier then speed potion; read on every ground move as `EntityPlayer.getAIMoveSpeed` does |
| `Living::jump`, `move_flying`, `move_entity_with_heading`, `living_update`, `player_post` | `EntityLivingBase.jump`, `Entity.moveFlying`, `EntityLivingBase.moveEntityWithHeading`, `EntityLivingBase.onLivingUpdate`, tail of `EntityPlayer.onLivingUpdate` |
| `Client::tick` | `Minecraft.runTick` order: pick, click, entity updates; `EntityPlayerSP.onLivingUpdate` sprint rules; `onUpdateWalkingPlayer` packets |
| `pick_with` | `EntityRenderer.getMouseOver(1.0F)`: 4.5 block ray, box widened by 0.1, 3.0 block limit from the eye, and the entity must be nearer than the block ray's result |
| `World::ray_trace_blocks`, `cube_trace` | `World.rayTraceBlocks` (cell-by-cell walk, MISS at the last air cell entered), `Block.collisionRayTrace` |
| client attack | `PlayerControllerMP.attackEntity`, client side of `EntityPlayer.attackTargetEntityWithCurrentItem` (own motion x0.6, sprint dropped) |
| `RemotePlayer` | `NetHandlerPlayClient.handleEntityMovement/Teleport`, `EntityOtherPlayerMP.onLivingUpdate` (3-tick interpolation) |
| `push_out_of_blocks` | `EntityPlayerSP.pushOutOfBlocks` at the four points around the feet |
| `Client::handle` | `handleEntityVelocity`, `handleEntityStatus`, `handleEntityMetadata`, `handleEntityProperties`, `handleAnimation`, `handlePlayerPosLook` (with its immediate answer) |
| `Server::process_player` | `NetHandlerPlayServer.processPlayer`: server-side living update, jump detection, the "moved too quickly" and "moved wrongly" checks, `handleFalling`; packets are dropped between a teleport and the client's answer |
| `Server::set_player_location` | `NetHandlerPlayServer.setPlayerLocation` and the S08 packet |
| `Server::on_update_entity` | `EntityPlayerMP.onUpdateEntity`, including `collideWithNearbyEntities` (server only in 1.8); the other player is found through the chunk it was filed under at the last world tick, as `World.getEntitiesInAABBexcluding` does |
| `Server::use_entity_attack` | `processUseEntity` (6 block limit, 3 through walls), `attackTargetEntityWithCurrentItem` |
| `Server::attack_entity_from`, `knock_back` | `EntityLivingBase.attackEntityFrom` (20-tick invulnerability, hits land every 10 ticks), `knockBack` |
| `Server::tracker_update` | `EntityTrackerEntry.updatePlayerList`: every 2 ticks or when airborne or metadata changed; 1/32 block and 1/256 turn quantisation; teleport on ground-state change; metadata and attributes echoed to the player itself |
| `encode_velocity` | `S12PacketEntityVelocity` constructor |

## Behaviours that fall out of the port

- Hits register at most once every 10 server ticks per victim.
- Only the first hit after a sprint start carries sprint knockback. The
  client drops its own sprint on each hit and picks it up again next tick
  without telling the server, so a W-tap, S-tap or any other sprint reset is
  needed to get sprint knockback again.
- Each click on the target while the client is sprinting multiplies the
  attacker's own horizontal motion by 0.6.
- A sprint change reaches ground speed on the same tick, and air control one
  tick later.
- Vertical knockback on a later hit of a combo is lower, because the server
  tracks the victim's vertical motion from the jump it thinks it saw.
- Jumping on the tick knockback arrives replaces the vertical knockback and
  adds the sprint-jump boost against it.
- The other player is seen up to several ticks late: tracker period, 1/32
  block rounding, 3-tick interpolation and the ping.
- A player pressed against a wall can sit one unit in the last place inside
  it on the server, because the box is rebuilt from the position with a
  float half-width. When knockback then slides that player along the wall
  by more than a quarter block past a block edge in one tick, the server
  moves it less far than the client did, logs "moved wrongly" and puts the
  player back. The client, put back at that spot, is no longer stopped by
  the wall and can walk into it, where `pushOutOfBlocks` shoves it out and
  the server keeps putting it back. This is the rubber-banding players see
  when they are comboed along a wall.

## Choices and simplifications

- The arena is a flat floor with optional full-height walls, all full
  blocks. Step-up, sneaking, webs, ladders and liquids are left out because
  nothing in the arena triggers them.
- The kick for "flying" (80 ticks in the air in a row) is left out: servers
  that host PvP allow flight, because a long combo trips it.
- At most one click per tick, pressed and released inside the tick. More
  clicks in one tick add nothing in 1.8.9: the server ignores the second
  attack and the client has already dropped its sprint.
- Mouse movement is applied once, at the start of the tick. The mod will do
  the same.
- `mouse_delay_fix` chooses the yaw used for the pick. Vanilla 1.8.9 uses
  the head yaw of the previous tick (a known bug); with the fix the current
  yaw is used. Training turns the fix on and the mod will carry the same
  fix. Vanilla behaviour stays available for the parity checks.
- Clocks tick every 50 ms exactly. Latency is a constant per direction plus
  uniform jitter, and packets keep their order.
- Not modelled yet: health, armour, food, potions, pearls, sword blocking,
  rods, bows. Those come with the later training stages.
- In open air the block ray returns a MISS at the point where it entered its
  last cell, and an entity only counts if it is nearer than that point. With
  a steep diagonal ray that point can be as close as 2.77 blocks, so a few
  hits between 2.77 and 3.0 blocks do not register. The port keeps this.

## How it is checked

1. `cargo test --release`: known 1.8.9 numbers (walk 4.317 m/s, sprint
   5.612 m/s, sprint-jump 7.127 m/s, jump apex 1.2492, jump length 12 ticks,
   knockback packets 0.4/0.3608 and 0.9/0.4608, 10-tick hit interval, sprint
   reset rule, 3-block reach, determinism).
2. Recordings of the real game (`parity/`, `tests/parity.rs`). The real
   1.8.9 client, with hook lines added, plays scripted scenes against a
   scripted second player on its integrated server and writes down every
   tick: inputs, every packet in both directions, and the state of both
   players on the client and on the server as raw bits. The simulator's
   client is fed the recorded server packets and inputs, its server the
   recorded client packets, and each must reproduce the recorded state and
   the recorded outgoing packets exactly. `cargo test --release` replays
   all recordings and fails on a single differing bit.

   | Recording | Scene | Client / server ticks | Hits (sprint, critical) |
   |---|---|---|---|
   | `basic` | walk, sprint, sprint-jump, strafe, walk backwards | 232 / 235 | |
   | `move` | random keys and mouse, Speed II | 612 / 614 | |
   | `nospeed` | the same without Speed | 412 / 415 | |
   | `walls1` | running and jumping into a stone corner | 612 / 614 | |
   | `fight1` | fight in a walled pen, Speed II | 1012 / 1015 | 108 (50, 27) |
   | `fight2` | fight, mouse sensitivity 0.83, one teleport back | 1012 / 1015 | 116 (56, 26) |
   | `fight3` | fight, Speed I | 1012 / 1014 | 95 (45, 23) |
   | `fight4` | fight, no Speed, sensitivity 0.2 | 1012 / 1012 | 95 (44, 21) |
   | `long1` | nobody leaves the ground, so the tracker reaches its periodic full update | 1012 / 1012 | 51 (2, 0) |
   | `wall1`, `wall2`, `wall3` | knocked along a wall again and again; four teleports back in `wall3` | 3036 / 3042 | 247 (247, 0) |

   Together: 9,964 client ticks and 9,988 server ticks, 712 hits, 673
   attacks by the real client, 557 knockbacks received by it, 14,433 packets
   from the client and 20,044 from the server compared, 5 teleports back.
   All of it is reproduced with zero differing ticks.

   What the recordings do not cover: the real game applies mouse movement
   once per rendered frame, the rig once per tick (as the mod will); the
   `mouse_delay_fix` variant of the pick, which needs the mod; and network
   delay, which the recordings leave out on purpose by replaying each side
   on its own.

## Found by the recordings

The first movement and fight recordings matched without changes. Four more
fights and three wall scenes turned up behaviour that was missing, all of it
now ported and covered by a recording:

- The server's "moved wrongly" check, the teleport back and the client's
  answer to it, including the float-width overlap that triggers it along
  walls.
- Collision against one box per block instead of merged wall boxes. The two
  differ exactly when a box overlaps a wall by that last-place unit.
- `pushOutOfBlocks` on the client.
- The entity push between players looks the other player up through the
  chunk it was filed under at the last world tick, so a player that has
  just crossed a chunk border by more than two blocks is missed for a tick.
