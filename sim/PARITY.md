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
| the hotbar slot in hand on the client (`Input::slot_key`, `LocalPlayer::sync_slot`) | the number keys in `Minecraft.runTick`, `PlayerControllerMP.updateController` and `syncCurrentPlayItem`, which also runs before an attack, a use and a release |
| a right click with pearls or an empty hand (`right_click`) | `Minecraft.rightClickMouse`, `PlayerControllerMP.sendUseItem`, the client side of `ItemEnderPearl.onItemRightClick` (one off the stack, no entity) |
| `Server::use_item`, `spawn_pearl` | `processPlayerBlockPlacement` (direction 255, the inventory sent again with `S30` and `S2F`, and the copy of the inventory brought up to date without sending), `processHeldItemChange`, `ItemInWorldManager.tryUseItem`, `ItemEnderPearl.onItemRightClick`, `EntityThrowable(world, thrower)`, `setThrowableHeading` |
| `mth::atan2`, `atan_tab` | `MathHelper.atan2` (the table-driven one). Its two tables are built with `Math.asin` and `Math.cos`, which differ from other maths libraries in the last place for 11 of 257 entries, so the values Java gives are stored |
| `Server::pearl_update` | `EntityThrowable.onUpdate` on the server (block ray, the players near the path, the thrower left out for five ticks), `EntityEnderPearl.onImpact` (the thrower put at the pearl with `setPositionAndUpdate`, fall distance cleared, 5 points of fall damage) |
| `Server::pearl_tracker_update`, `S2C::Pearl`, `S2C::Destroy` | `EntityTrackerEntry` for a thrown entity: `S0E` with `S1C` and `S12` at the throw, an update every 10 ticks with velocity, `S13` on the tick after it is gone |
| what a player holds (`ServerPlayer::equip`, `S2C::Equip`) | the equipment check in `EntityLivingBase.onUpdate`: `S04` to the other player and the attribute modifiers of the item moved, once per movement packet; `EntityPlayer.onUpdate` ending the use of an item that is no longer in hand |
| `inside_opaque_block` | `Entity.isEntityInsideOpaqueBlock` and the suffocation damage in `EntityLivingBase.onEntityUpdate` |
| `ClientPearl`, the inventory on the client | `handleSpawnObject`, `handleEntityVelocity`, `handleEntityMovement` and `handleEntityTeleport` through `Entity.setPositionAndRotation2`, `EntityThrowable.onUpdate` on a client (it flies on and hits nothing), `handleDestroyEntities`, `handleWindowItems`, `handleEntityEquipment` |

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
- A number key takes a slot in hand at once on the client. The server is
  told at the start of the next tick, or earlier if an attack, a use or a
  release comes first on the same tick.
- The server throws a pearl along the view it last heard of, which is the
  one of the tick before the click. Mouse movement on the tick of the click
  comes too late for the throw.
- A pearl flies 1.5 blocks a tick at first. Its thrower is put where the
  pearl was at the start of the tick in which it hit something, so up to a
  block and a half short of the block or player it hit, and often a little
  above the floor. The thrower takes 5 points of damage that a block does
  not lessen. As after any damage, for the next ten ticks a stronger hit
  only adds the difference and does not knock back: a player that has just
  come out of a pearl cannot be knocked away for half a second. Its view
  is set to what the server last had: with a ping of 200 ms that is where
  it looked about four ticks earlier.
- A pearl that reaches the other player stops there and does them no harm.
- A pearl flies through its own thrower once it is five ticks old, and on
  that tick it does not notice a block either. Thrown steeply upwards and
  walked under, it goes through the thrower and the floor.
- A pearl that hits a ceiling, or a wall from less than a quarter block
  away, puts its thrower's head inside a block: the server takes a point of
  suffocation damage on every movement packet that passes its hit
  protection until the client has pushed itself out.
- The damage of a hit comes from the item the server saw in the attacker's
  hand at its last movement packet. A hit on the very tick the sword is
  taken back in hand is a bare-hand hit (1 instead of 8), with full
  knockback; a hit on the tick the pearls are taken in hand still does the
  sword's damage.
- Taking another slot in hand ends a block on the client at once and
  without a word to the server, which ends it at the next movement packet.
  With the button still down, the next tick throws a pearl.
- After a throw the server sends the whole inventory again, every stack a
  new one, unless it still has the thrower blocking. A client that has the
  sword back in hand and is blocking when that arrives drops the block on
  its own side only, exactly as when the sword alone is sent again.
- A pearl is a second or two in flight and the other client is told its
  position only at the throw and every ten ticks after; in between it
  flies on by the same rules on both sides.
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
- A sword in the first hotbar slot and ender pearls in the second; the
  other seven are empty. Breaking blocks is not modelled: a left click on a
  block swings the arm and that is all.
- At most one number key per tick.
- The random part of a throw (a few thousandths per axis) is drawn by the
  simulator. A replay takes the direction the game drew from the recording
  and checks where the pearl started, that the game's direction is within
  the random part of the simulator's, and everything that follows.
- A pearl does not spawn an endermite: that needs the mob spawning rule,
  which PvP servers turn off, as the rig does.
- The time a server makes a player wait between pearls (`pearl_cooldown`)
  is a plugin's, not the game's. A throw inside it is refused and the
  inventory sent again, so the client gets its pearl back.
- `pearl_guard`, off in replays and on in training, is what servers add
  against pearl glitches: a pearl that would put its thrower inside a block
  is lost. Without it the plain game puts a player into the ground or a
  wall in the cases above, and the arena's ground has no bottom.
- Players and pearls are always in each other's tracking range and in
  loaded chunks, as they are in an arena. (The game stops updating an
  entity within 32 blocks of a chunk that is not loaded; one recording in
  which a pearl got out of the rig's small world showed it. The replay can
  follow that, and no stored recording needs it.)
- Mouse movement is applied once, at the start of the tick. The mod will do
  the same.
- `mouse_delay_fix` chooses the yaw used for the pick. Vanilla 1.8.9 uses
  the head yaw of the previous tick (a known bug); with the fix the current
  yaw is used. Training turns the fix on and the mod will carry the same
  fix. Vanilla behaviour stays available for the parity checks.
- Clocks tick every 50 ms exactly. Latency is a constant per direction plus
  uniform jitter, and packets keep their order.
- Not modelled yet: health, armour, food, potions, rods, bows.
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
   of 8, the block hit as a sprint reset, the block that the server keeps
   after the sword came back, a pearl carrying its thrower and costing 5
   points, a pearl stopping at the other player, the pearl cooldown, the
   bare-hand hit right after the sword is taken back, and the block that
   ends when another slot is taken).
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
   | `pearl1`, `pearl2` | fights under a roof in which both players throw pearls: at each other, past each other, up, down and sideways, with the view flicked for one tick; two sensitivities | 2024 / 2024 | 134 (58, 36), 54 pearls |
   | `pearl3`, `pearl4` | the same with blocking around the throws, one of them with unbreakable swords and no Speed | 2024 / 2026 | 87 (42, 24), 59 pearls |
   | `pearl5` | pearls thrown with the button held (one every four ticks), slots changed back and forth, and the scripted player throwing often | 1012 / 1012 | 80 (36, 18), 32 pearls |
   | `pearlsolo1`, `pearlsolo2` | the real player alone, throwing at walls, floor and ceiling | 1624 / 1630 | 32 pearls |

   Together: 24,744 client ticks and 24,788 server ticks, 1,772 hits, 1,478
   attacks by the real client, 1,471 knockbacks received by it, 41,483
   packets from the client and 62,685 from the server compared. For
   blocking: 791 right clicks, 2,215 ticks spent blocking, 1,354 clicks
   lost to a block, 229 hits taken on a block, the sword sent again 331
   times, and 20 blocks that the client dropped without telling the
   server. For pearls: 177 thrown (97 by the real client), every one of
   them followed tick by tick on the server and as the client sees it, 26
   that stopped at the other player, 575 changes of slot, and 24 hits made
   while the server had something other than the sword in the attacker's
   hand. All of it is reproduced with zero differing ticks.

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

Pearls were ported from the source in the same way and then recorded. Six
of the seven scenes matched on the first replay of both halves, throw,
flight, landing and all. What the recordings added:

- Suffocation. A pearl thrown at the roof of the pen put the player's head
  in it, and the game took a point of damage that the simulator did not.
- A client drops a pearl that has fallen out of the world by itself.
- In the seventh scene a pearl went through its own thrower and the wall
  behind, exactly as the port said, and left the few chunks the rig keeps
  loaded, where the game stops updating it. That scene was recorded again
  with another seed; the stored ones stay inside.
- In the rig, the scripted player's connection has to count as open, or
  its pearls vanish without moving it.
