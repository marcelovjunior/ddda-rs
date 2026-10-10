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

## Results (codex-1)

The host now reads the sequence-4 placement, group gate, Dragon_OP graph,
model, motion banks, combat parameters and independent HP regions from the
installation. Admission reevaluates LOT changes each frame. A shared enemy
owner allocator avoids colliding with goblins; the dragon participates in
the fight's hit pass and publishes motion-authored volumes at pass C.

TargetFly steering is translated from `0x00a60b70` / `0x00a61590`, look-at
construction `0x007af5b0` and angular limiting `0x007ae860`. Its movement
consumes the previously cached model forward, and its strict arrival test
uses pre-movement displacement. The ordinary yaw rotator is disabled for
that action. The entrance controller consumes installed FSM requests and
reports actual motion/action completion. S29 holds the completed state-3
hover before the unimplemented PressAttack request; this is an explicit
scope boundary, not an original controller pause.

Verified after the rotator-clear follow-up: nine dragon unit tests,
two existing installed dragon tests (2.39 s), and the new installed host
entrance test (2.36 s); release compilation took 16.58 s. The latter visits
**4 -> 5 -> 1 -> 3**, reaches
the real hover motion end, applies q0 bit 1, leaves bit 20 clear, retains
ownership and publishes the installed rise's hurt volumes. Hover's final
frame has no active hurt volumes; the test checks authored activation
instead of assuming the body is always damageable. No original live replay
comparison has been performed.

The release viewer suite passed **61 tests, 2 existing ignored** (12.94 s,
compile 32.76 s); its normal build passed in 50.73 s. Image inspection
caught the character root losing its centimetre-to-metre scale, and the
fixture retaining unrelated story controllers. Those were corrected. A
subsequent capture exposed the raise-only scene setter refusing to lower
the prepared scene; that fixture setup was corrected too.

The final release build passed in 61 s. The final private 1600x900 image
was inspected: its HUD says scene **1025**, and the complete dragon is
flying above Cassardis with the installed model and flight motion. Its
process (33152) exited **0 in 38.94 s** under a 60-second watchdog. Earlier
captures are discarded as acceptance evidence. Screenshots, logs, binaries
and decompiler exports remain in the private research tree. Another
coordinator-owned viewer process was still present during this capture
despite the normal exclusive board claim succeeding; that lease mismatch
was reported to head. This capture verifies body presentation, not timing
or performance against the original.

`DDVIEW_DRAGON_ENTRANCE_CAPTURE` with `--skip-intro` explicitly starts an
actor-only fixture at the documented post-ev00 layout gate, stops unrelated
story scripts and holds flight frame 15 reached by real simulation while
GPU pipelines warm. Its camera is chosen for visibility. This is **not**
the real opening route; S31 still replaces the stand-ins.

Remaining limits: layer-2 turn motion/weight is represented but not blended
into the skeletal pose; attached/rider effects and general flight collision
are not implemented. Degenerate vertical/zero target look-at preserves
orientation rather than guessing initialized fallback globals. The host
has no real actor condition/controller word adapter yet, so it preserves
sequence-4 initial collision protection rather than inventing those words.
The entrance subset supplies no status acceleration, external correction or
velocity caps; that absence is explicit, not a tuned approximation. The
viewer supplies sourced five-frame primary interpolation; headless poses
currently sample the primary motion without transition blending. LAN guests
do not yet receive this host actor; the separate assigned S33 brief owns
enemy identity, quaternion/scale replication and guest body dispatch.

Next action: after head reviews/merges S29, continue S30's authored landing
and breath actions; S33 separately completes the LAN guest adapter.
