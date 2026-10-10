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
