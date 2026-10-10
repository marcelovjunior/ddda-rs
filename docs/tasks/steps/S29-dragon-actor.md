# S29: The beach dragon as a host actor, with its entrance

- **Branch:** `step/s29-dragon-actor`
- **Files:** `crates/ddenemyrt/src/dragon*.rs`, `crates/ddhost` (owning the
  dragon actor in the fight/host frame), `crates/ddview` (its body and
  motions), tests
- **Kind:** one feature

## Context (S24 map, docs/tasks/dragon-attack.md "S24 encounter map")

The Dragon_OP entrance FSM path is `4 -> 5 -> 1 -> 3 -> 0 -> 7 -> 8 -> 2 ->
9 -> 10 -> 6`: HoverUp (52, motion 100), HoverKakku (54, 103), TargetFly
(10, 126), KakkuHover (53, 121), PressAttack (33, 298 -> 299), BreathWide
(21, 263 -> 264). The runtime has tested local prefixes for 52, 54, 53 and
TargetFly's travel substep, and the FSM interpreter covers the path with
synthetic feedback. Missing: an authoritative dragon actor in the host,
its frame phases (incl. `finish_pass_c` after terrain), and its body in the
viewer.

## Do

1. Make the dragon a host-owned actor in the fight (like goblins: owner id,
   victim regions/HP from its data, volumes), spawned for the opening's
   dragon scene (stage 200, sequence 4) when the story asks for it.
2. Drive the entrance FSM states 4 -> 5 -> 1 -> 3 with the existing action
   prefixes and real motion completion (finish TargetFly's steering if the
   path needs it), each frame in the host's phase order.
3. Draw it in the viewer (its model and motions, like goblins).

## Done when

A headless test runs the entrance from rise to hover with real motion
completion (no synthetic feedback), and a capture shows the dragon flying
in over Cassardis. Stop at state 3 (hover); landing and fire are S30.
