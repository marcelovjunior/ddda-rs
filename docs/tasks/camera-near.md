# camera-near: What the game camera does close to walls and the Arisen

- **Owner:** codex lane
- **Branch:** `task/camera-near`
- **Board area:** `play/camera-near`
- **Status:** in progress (codex-30)

## Goal

Playtest (maintainer, 2026-10-09): when the follow camera is pushed in by
a wall, the viewer shows the inside of models and cuts the Arisen open.
Head implemented the original's collision rule (0x007de570 from
0x007e1400, `play::follow_camera`): a ray target->eye, then a sphere
of radius 1.3 x near + 10 x (1 - |cos|), one near plane ahead of the
eye, swept toward the target. The near plane is the recorded 96 cm
(`CAMERA_NEAR`). That keeps the near plane out of *collision*, but not
out of drawn geometry that has no collision near it, or out of the
Arisen's body.

Trace and implement what the original does here:
1. Whether the near plane changes: who writes the current camera's
   +0x34 (`*(DAT_018D0B6C + 0xCE0) + 0x34`) and when (indoors, collided,
   by camera parameter files). Check with the oracle in a tight spot
   (Cassardis, the porch outside the class-selection room).
2. The Arisen fading out when the camera comes close (the original
   fades the player model; find the rule: distance, alpha, which
   models, pawns too).
3. The 0x007e1400 branch at +0xB98 (the eye moved toward the target by a
   signed distance when the pitch passes conditions) and the stage
   attribute bit 0x10 hits the original ignores (0x007c6f90).
4. Whether the camera's collision includes layout objects' collision
   (houses, pillars placed as objects), not only the stage SBC. Head's
   probe (`--probe`, F9) now logs each hit's distance to collision.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs use `--unattended`.

## Scope

`crates/ddview/src/play.rs` (follow_camera, camera_projection),
`crates/ddview/src/character.rs` (fade), `docs/CAMERA.md`, this brief's
Results.

## Done when

The maintainer's spots (porch pillar outside the class-selection room)
no longer show model insides, and the Arisen fades as in the original.
Fill in Results and report to `head`.

## Results

2026-10-09, codex-30: traced the actor fade at **0x0088bdc0**, its
semantic-joint lookup **0x00ff9a20**, draw consumer **0x00b6a070**, and
explicit normal camera near96 initialization **0x007dc8a0**. Detailed
source, thresholds, state transitions and limitations are in CAMERA.md.

Implemented: `mtcamera::fade` ordinary distance state machine; viewer
adapter for PlayState actors after joint propagation, installed-exe
threshold reads, private material clones and restoration. Source global
bypasses, aiming pawn cone and special camera variants remain open.

Tested: two headless state-machine regressions and the viewer material
restoration regression pass (release profile); `cargo build --release -p
ddview` passes with two pre-existing warnings. A bounded hidden
`--unattended --play --radius 0 --shot ... --shot-delay 8` run rendered a
two-actor field screenshot and exited automatically without query/system
errors. Its ordinary distant camera does not validate near-fade appearance.
Compared against original: not yet; porch and wall acceptance remains open.

Followup (head scope approval #2452): nonfield `stage_collision` now
preserves authored SBC material records and triangle attributes. The
installed Cassardis regression passes: unchanged triangle count and at
least 100 surface casts retain their source metadata, including nonzero
flags. No additional geometry was introduced.

Sweep followup (head #2493): replaced inward bisection with the sourced
target-outward nearest sphere cast, using existing character-contact
geometry through a narrow `sphere_contact` wrapper. `Ground::ray_with_flags`
and the Terrain override preserve nearest ray metadata. Both ray/sweep
reject bit0x10 after selecting the hit, and sweep response stands 0.1 cm
off the surface. Three geometric regressions cover a clear pocket past a
thin wall, a flagged nearest wall with an unflagged wall behind it, and
initial overlap. The three new and one existing follow-camera regression
pass, as does the nearest-ray metadata regression. Release ddview build
passes (two existing warnings). No new original/render comparison yet.

Next: compare the corrected camera at the reported porch/room wall.
Head's probe #2486 found a wall only 42 cm ahead of the eye (near96), with
drawn st200_md12 about 10 cm beyond its SBC; this is not evidence for adding
new scenery geometry. The porch pillar itself was not yet probed.
No game or install changes; no live game control used for this checkpoint.

Further source trace: CAMERA.md now records the +0xB98 producer chain
from selected actor sphere/capsule contacts (0x00888750), per-frame maximum
accumulator (0x00421d60/0x00420910), and exact scalar smoother
(0x007bc800, vtable0x01599174). No behavior was added for this path: the
viewer lacks the selected animated actor collision entries and eligibility
state. It should not substitute guessed render bounds. Source review
included disassembly because Ghidra prunes the producer's contact loop.
This documentation checkpoint adds no tests or original comparison.

Subsequent implementation checkpoint: `mtcamera::actor_offset` now has
source query construction, explicit `EligibleEntryProvider` contract,
per-frame contact maximum and per-view scalar smoothing. Three targeted
unit tests and clippy pass. Existing authored `mIsObjAdj` volumes are a
partial provider foundation, but source collection1 includes activation,
owner vf64 filtering and runtime-created entries; no default or viewer
hook is supplied. CAMERA.md records these gates and unresolved actor
enum semantics. No new rendered/original comparison.

Pose-shift followup: added the exact 0x007e1400 pre-stage-sweep eye/target
translation with strict 0.1 threshold and caller-supplied transition gate.
Disassembly proves the target condition uses displacement length, not
remaining view length. A fourth actor-offset regression covers that rule
and its signed boundary. Independent lane review passed all six mtcamera
library tests (four offset, two fade); targeted clippy passes. Provider
and viewer integration remain explicitly absent pending complete gates.

Handoff to later staffing: source evidence now identifies opening dragon
uEm5800 as a concrete eligible class and documents its ordinary enemy
vf64 filter and phase93 collection snapshot in CAMERA.md. Preserve source
shape kinds, actor +0x2748 state, per-view eligibility and runtime-created
entries before wiring a provider. Camera work paused by lane assignment
to staff the higher-priority doors brief; no provider or original visual
acceptance claim. Branch remains reviewable and all camera claims released.
