# S33: LAN: replicate every enemy kind, not only goblins

- **Branch:** `step/s33-lan-enemy-kinds`
- **Files:** `crates/ddnet/src/mirror.rs`, `proto.rs` (protocol bump),
  `crates/ddview` (the guest's enemy bodies), tests
- **Kind:** one feature
- **After:** S29 (the dragon exists as a host actor)

## Context

`ddnet::mirror::Enemy` carries a goblin's place, yaw, action, motion
(bank/slot/frame), HP and death; `from_fight` reads only `Fight::goblins`,
and the guest draws every mirrored enemy as a goblin. S29 made the beach
dragon a host actor (any local seat sees it), but a LAN guest does not:
codex-1, board #4133. The cyclops (on hold) and later families will need
the same path, so do it once, generically.

## Do

1. Give the mirrored enemy an identity: its family/model (goblin, dragon,
   later cyclops...) and whatever the guest needs to build the right body
   from its own install (model path, motion banks). Read it from the host
   fight, not a table guessed in the viewer.
2. Carry a full rotation (quaternion) and scale where the family needs it
   (the dragon pitches and rolls in flight); keep goblins cheap in delta
   snapshots.
3. Guest side: spawn/despawn and pose the body by family; unknown families
   are logged once, not drawn as goblins.
4. Bump the protocol; round-trip tests (whole/delta) for a goblin and the
   dragon; an installed host+guest test where the guest sees the dragon.

## Done when

A two-seat LAN test shows the guest mirroring the host's dragon (identity,
pose, HP) alongside goblins, with the protocol bumped and documented.

## Results (ready for review)

Protocol 7 carries the host-loaded model and indexed motion-bank paths, family
dispatch tag, yaw or full quaternion, nonuniform scale, action/motion/frame and
main-pool HP/death. Six delta groups avoid resending resource names when only
pose changes; an unchanged enemy still occupies five bytes. Protocol tags and
interpolation are co-op presentation decisions, not original-game network rules.
No model, motion, texture or bulk stats are copied into the wire or repository.

After reviewed S29 merge `bb68257`, `from_fight` projects goblins and the canonical
opening dragon. Goblin identity preserves the existing installed loader's path.
Dragon identity comes from its bound `rModel` descriptor and loaded body banks;
rotation, per-axis scale, frame and health come from the host actor.
`Dragon::current_action` exposes its existing typed entrance actions 10/52/53/54,
without inferring actions from motion slots. Future host families still need a
projection and supported dispatch tag; cyclops is not implemented by this brief.

The guest owns bodies by authoritative owner id, loads resources from its own
install, shares parsed banks by resource paths and removes missing/replaced
identities with their child entities. Unknown families are logged once and not
substituted with goblins. Ordinary LAN play and the opening guest use this path;
opening previously did not read enemy snapshots. Poses retain full rotation and
nonuniform scale with the existing centimetres-to-metres conversion. Quaternion
interpolation and immediate motion switches are presentation choices; native
animation blend state is not replicated.

Final validation on integrated source `3a0023a` (including reviewed S26):

- `cargo test --release -p ddnet`: **16 passed**, compile 14.26 seconds. The
  installed mixed test took 2.57 seconds, goblin replication 5.39 seconds and
  NPC/cutscene story replication 4.49 seconds. The earlier unleased S34 run had
  exited before this command started; there was no build overlap.
- The mixed test uses two seats over actual loopback UDP, 30 ms +/- 10 ms
  latency and 5% loss. It observed all four entrance actions and actual non-yaw
  flight rotation, then checked the received snapshot hash and exact enemy
  identity/pose/motion/HP against that host frame. Closing the authored LOT gate
  retired the dragon while retaining the goblin. This is retirement, not death.
- `cargo test --release -p ddview`: **64 passed, 2 existing ignored**, compile
  27.70 seconds, tests 12.41 seconds. New ECS regressions cover owner-keyed pose
  despite snapshot reordering, full quaternion/nonuniform scale, identity
  replacement, and root/child removal. Normal release build passed in 46.08
  seconds. Only inherited Args-visibility/audio-dead-code warnings remained.

Two real ddview processes used the player's install and joined over UDP. Final
host PID 21572 and guest PID 33176 both exited 0 after 54.7958705 seconds total;
neither process's 60-second watchdog triggered. Both final PNGs were inspected:
the guest visibly renders the host's dragon flying over Cassardis. Guest logs
show owner 100 Goblin (`model\\em\\e01\\e0100\\e0100`) and owner 101 Dragon
(`model\\em\\e58\\e5800\\e5800`) before its screenshot. An earlier guest capture
visibly showed the goblin near the two Arisen while the dragon was outside the
ordinary follow camera. All raw screenshots, logs and launcher remain private
under `E:/ddda-re/notes/steps/S33/`.

Visual acceptance uses the explicit entrance diagnostic: the host alone adds a
synthetic goblin under `DDVIEW_LAN_ENEMY_CAPTURE`, and the existing
`DDVIEW_DRAGON_ENTRANCE_CAPTURE` camera frames the mirrored dragon using the host
fixture's offset. This does not assert a natural mixed encounter, matching host
and guest screenshot frames, full story-route acceptance or comparison against
an original-game recording. No game installation or original game process was
modified. Source/test/LAN acceptance is complete; head review and merge are next.
