# Parity with Minecraft 1.8.9

The simulator is a port of the decompiled 1.8.9 client and server (MCP 9.19
names), method by method. No game code is copied into this repository; the
table lists which game method each part follows so it can be re-checked.

## What is ported

| Simulator | Game method |
|---|---|
| `mth::sin`, `mth::cos`, `look_vec` | `MathHelper.sin/cos` (65536-entry table), `Entity.getVectorForRotation` |
| `Aabb::calc_*_offset`, `calculate_intercept` | `AxisAlignedBB.calculateXOffset/YOffset/ZOffset`, `calculateIntercept` |
| `Body::move_entity` | `Entity.moveEntity` (collision order Y, X, Z; ground flag; motion zeroing) |
| `Attr::value` | `ModifiableAttributeInstance.computeValue` for `generic.movementSpeed`, sprint modifier then speed potion; read on every ground move as `EntityPlayer.getAIMoveSpeed` does |
| `Living::jump`, `move_flying`, `move_entity_with_heading`, `living_update`, `player_post` | `EntityLivingBase.jump`, `Entity.moveFlying`, `EntityLivingBase.moveEntityWithHeading`, `EntityLivingBase.onLivingUpdate`, tail of `EntityPlayer.onLivingUpdate` |
| `Client::tick` | `Minecraft.runTick` order: pick, click, entity updates; `EntityPlayerSP.onLivingUpdate` sprint rules; `onUpdateWalkingPlayer` packets |
| `pick_with` | `EntityRenderer.getMouseOver(1.0F)`: 4.5 block ray, box widened by 0.1, 3.0 block limit from the eye, and the entity must be nearer than the block ray's result |
| `World::ray_trace_blocks`, `cube_trace` | `World.rayTraceBlocks` (cell-by-cell walk, MISS at the last air cell entered), `Block.collisionRayTrace` |
| client attack | `PlayerControllerMP.attackEntity`, client side of `EntityPlayer.attackTargetEntityWithCurrentItem` (own motion x0.6, sprint dropped) |
| `RemotePlayer` | `NetHandlerPlayClient.handleEntityMovement/Teleport`, `EntityOtherPlayerMP.onLivingUpdate` (3-tick interpolation) |
| `Client::handle` | `handleEntityVelocity`, `handleEntityStatus`, `handleEntityMetadata`, `handleEntityProperties`, `handleAnimation` |
| `Server::process_player` | `NetHandlerPlayServer.processPlayer`: server-side living update, jump detection, `handleFalling` |
| `Server::on_update_entity` | `EntityPlayerMP.onUpdateEntity`, including `collideWithNearbyEntities` (server only in 1.8) |
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

## Choices and simplifications

- The arena is a flat floor with optional full-height walls, all full
  blocks. Step-up, sneaking, webs, ladders and liquids are left out because
  nothing in the arena triggers them.
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
2. Parity rig (pending): scripted inputs are played in the real game and in
   the simulator and the per-tick state is compared bit for bit.
