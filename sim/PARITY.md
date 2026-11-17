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
| right click and blocking on the client (`right_click`, `Client::tick`) | `Minecraft.runTick` (while an item is in use, letting go of the button ends it and every click of the tick is thrown away; a held button clicks again when `rightClickDelayTimer` is 0), `rightClickMouse` (not while a block is being hit), `PlayerControllerMP.isPlayerRightClickingOnEntity`, `interactWithEntitySendPacket`, `onPlayerRightClick`, `sendUseItem`, `onStoppedUsingItem`, `ItemSword.onItemRightClick`, `EntityPlayer.setItemInUse`, and the check in `EntityPlayer.onUpdate` that the item in use is still the stack in the hand; `EntityPlayerSP.onLivingUpdate` (input times 0.2, no sprint while using an item) |
| blocking on the server (`ServerPlayer::set_using`, `Server::damage_entity`) | `processPlayerBlockPlacement` with direction 255 -> `ItemInWorldManager.tryUseItem`, `processPlayerDigging` RELEASE_USE_ITEM -> `stopUsingItem`, the using-item bit of the flags byte in the metadata, `EntityPlayer.damageEntity` (a blocked hit does `(1 + damage) * 0.5`; a fall cannot be blocked) |
| the sword sent again (`slot_dirty`, `S2C::SetSlot`) | `ItemSword.hitEntity` -> `ItemStack.damageItem`, `Container.detectAndSendChanges` in `EntityPlayerMP.onUpdate`, `S2FPacketSetSlot`, `NetHandlerPlayClient.handleSetSlot` |

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
- A block starts after the clicks of the tick the button went down on, so
  click and block on one tick is a hit followed by a block (the block hit).
  While blocking the client moves at a fifth of its input and cannot
  sprint, and it sprints again by itself on the tick the block ends: a
  one-tick block is a sprint reset, like letting go of W.
- Clicks made while blocking are lost, and so are the clicks of the tick on
  which the button is let go.
- A right click on the tick of a left click on a block (the floor, say) is
  ignored; a button that is still down clicks on a later tick.
- A hit that registers costs the sword a point of durability, and the
  server then sends the sword to the attacker again. A client that is
  blocking when it arrives drops the block on its own side without telling
  the server, moves at full speed again and, with the button still down,
  clicks again only once four ticks have passed since the last right
  click. If the button is let go before that, no release is ever sent: the
  server keeps the player blocking, and everybody sees it blocking, while
  it attacks freely. It lasts until the next block that ends normally. An
  unbreakable sword never causes this, and Unbreaking makes it rarer.
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
- A click is pressed and released inside one tick. Players set to click
  more than twenty times a second make two clicks in some ticks.
- The right button is either down or up for a whole tick; it goes down at
  most once per tick.
- Swords only. Breaking blocks is not modelled: a left click on a block
  swings the arm and that is all.
- Mouse movement is applied once, at the start of the tick. The mod will do
  the same.
- `mouse_delay_fix` chooses the yaw used for the pick. Vanilla 1.8.9 uses
  the head yaw of the previous tick (a known bug); with the fix the current
  yaw is used. Training turns the fix on and the mod will carry the same
  fix. Vanilla behaviour stays available for the parity checks.
- Clocks tick every 50 ms exactly. Latency is a constant per direction plus
  uniform jitter, and packets keep their order.
- Not modelled yet: health, armour, food, potions, pearls, rods, bows.
  Those come with the later training stages. Damage is counted up to the
  block (critical hits, blocked hits) and not yet taken from health.
- In open air the block ray returns a MISS at the point where it entered its
  last cell, and an entity only counts if it is nearer than that point. With
  a steep diagonal ray that point can be as close as 2.77 blocks, so a few
  hits between 2.77 and 3.0 blocks do not register. The port keeps this.

## How it is checked

1. `cargo test --release`: known 1.8.9 numbers (walk 4.317 m/s, sprint
   5.612 m/s, sprint-jump 7.127 m/s, jump apex 1.2492, jump length 12 ticks,
   knockback packets 0.4/0.3608 and 0.9/0.4608, 10-tick hit interval, sprint
   reset rule, 3-block reach, determinism, a blocked hit doing 4.5 instead
   of 8, the block hit as a sprint reset, and the block that the server
   keeps after the sword came back).
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
   | `move` | random keys and mouse, Speed II | 612 / 615 | |
   | `nospeed` | the same without Speed | 412 / 415 | |
   | `walls1` | running and jumping into a stone corner | 612 / 615 | |
   | `fight1` | fight in a walled pen, Speed II | 1012 / 1015 | 105 (51, 26) |
   | `fight2` | fight, mouse sensitivity 0.83 | 1012 / 1014 | 119 (54, 28) |
   | `fight3` | fight, Speed I | 1012 / 1012 | 97 (50, 25) |
   | `fight4` | fight, no Speed, sensitivity 0.2 | 1012 / 1012 | 95 (44, 21) |
   | `long1` | nobody leaves the ground, so the tracker reaches its periodic full update | 1012 / 1012 | 49 (6, 0) |
   | `wall1`, `wall2`, `wall3` | knocked along a wall again and again | 3036 / 3042 | 247 (247, 0) |
   | `block1`, `block2`, `block4` | fights in which both players block: taps with a click, holds, bursts; three sensitivities and speeds | 3036 / 3041 | 282 (157, 58), 86 blocked |
   | `block3`, `block6` | the same with unbreakable swords, which are never sent again | 2024 / 2026 | 180 (85, 49), 62 blocked |
   | `block5` | blocks that start on the tick of a hit and are held past the sword coming back | 1012 / 1012 | 99 (54, 20), 48 blocked |
   | `fight2_v1`, `wall3_v1` | two recordings from before blocking was recorded, kept for their five teleports back | 2024 / 2030 | 198 (138, 26) |

   Together: 18,060 client ticks and 18,096 server ticks, 1,471 hits, 1,173
   attacks by the real client, 1,165 knockbacks received by it, 28,720
   packets from the client and 44,959 from the server compared, 6 teleports
   back. For blocking: 491 right clicks, 1,769 ticks spent blocking, 1,083
   clicks lost to a block, 196 hits taken on a block, the sword sent again
   249 times, and 8 blocks that the client dropped without telling the
   server. All of it is reproduced with zero differing ticks.

   The damage a block takes off is not in the recordings: both players
   have Resistance V there, so no hit costs health. Whether the server has
   a player blocking at every tick is compared; the halving itself follows
   the game's formula and will be recorded together with health.

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

Blocking was ported from the source first and recorded afterwards. The six
blocking scenes matched on the first replay, including the block that the
client drops when its sword is sent again, which the source had predicted.
The one thing the recordings corrected was in the rig itself: the scripted
player had not been registered to be told about its own inventory, as a
player that joins is, so the game never sent it its sword.
