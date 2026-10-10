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

## Results (in progress)

Prepared the protocol-7 enemy schema: host-loaded model and indexed motion-bank
paths, family dispatch tag, yaw or full quaternion, nonuniform scale, animation,
and main-pool HP/death. Six field groups avoid resending resource names when
only pose changes; an unchanged enemy still occupies five bytes. These are
co-op protocol decisions, not rules copied from the original game's networking.
`GoblinData.model` preserves the existing installed loader's model path; it is
not selected by a guessed viewer table.

A reusable guest view owns bodies by authoritative owner id, loads models and
motion banks from the guest's install, removes missing/replaced identities, and
logs unsupported family tags once. Both ordinary LAN play and the opening guest
call that path; opening previously never read enemy snapshots. The root scale
uses the existing character renderer's centimetres-to-metres conversion.

Preliminary `cargo test --release -p ddview --no-run` compiled successfully in
41.07 seconds using the shared M5 target. This is a compile check, not a viewer
run or a passing test-suite claim. The mixed-kind whole/delta and interpolation
regressions passed (4 tests, compile 13.63 seconds). The full DDNET suite then
passed all 15 tests: 5 unit, 4 wire, 3 loopback, 2 replication, and 1 story test;
this includes the installed goblin fight under UDP loss (3.33 seconds) and the
NPC/cutscene story path (5.67 seconds). The guest view shares parsed motion banks
by their resource paths; additional owner-keyed pose and body/child removal
regressions are written and await the M5 lease. The dragon projection, installed
mixed dragon/goblin host/guest check and visual check await the reviewed S29
merge. No original-game or dragon LAN acceptance claim is made by this
preparation checkpoint.
