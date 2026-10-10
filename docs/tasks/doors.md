# doors: Doors that open and close by themselves

- **Owner:** codex lane
- **Branch:** `task/doors`
- **Board area:** `world/doors`
- **Status:** working (codex-32)

## Goal

Maintainer, 2026-10-09: the "missing ground" in Cassardis (missing-ground
brief: st200_md06 floor mask 0x3 with nothing under it at level 4) is in
the original too. The original hides it with **a door that closes by
itself** at that house's entrance; in the original a row of unaliased
blue pixels shows the hole under the door. The LOD rule is right; the
viewer is missing the door.

Make doors work as in the original, for every stage: find which object
class the auto-closing doors are (uOm* door classes, their layout
records in `scr\stNNN\etc\*` / SDL, models and collision), spawn them at
their authored places, and run their open/close behaviour (what opens
them: player proximity, interaction, events; how fast; when they close;
their collision while open/closed, for the player and the camera). Check
the doors on the opening route first: the class-selection room, the
Cassardis houses and gates. Doors that are stage handoffs (door task /
JumpArea) keep working as they do now.

Build the system, not the step: one door runtime for all stages.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks follow the safety rules in `docs/CODEX.md`. Test runs use
`--unattended`.

## Scope

New `crates/ddview/src/doors.rs` (or a runtime crate if the logic is
shared with the host), the layout readers it needs (mtlot / SDL),
`docs/WORLD.md` doors section, this brief's Results. Coordinate on the
board with the owners of opening.rs and stream.rs.

## Done when

From the maintainer's spot (top of the path by the Cassardis gate,
looking down the alley; probe points (2733, -82, -2239) stage cm) the
doorway shows its closed door, and walking up to it opens it as in the
original. Fill in Results and report to `head`.

## Results

### Source and headless checkpoint (2026-10-09)

- The absent Cassardis objects are in `_p` layouts, whereas the viewer
  loaded only `s00/s10`: 23 eligible door candidates at scene 1400/noon,
  in groups 17/18/19. The reported doorway resolves to group 18 record 2,
  `om7121`. No floor LOD change is needed (maintainer's original check).
- New `dddoor` reads the player's executable object table at `0x0150fe40`
  (32-byte rows, null-terminated), selecting the longest case-insensitive
  prefix as `0x0075f0a0` does. `0x00762d30` selects the concrete class at
  row+8: `om7120/7121` use `uOmObj7010`, DTI `0x019a5b64`.
  `cLayoutSetOm` vf12 shares the existing `0x0075faa0` GPL gate.
- `0x00c3e700` initializes hinged variants. `0x00c3f130` runs the door
  phases; `0x00c3fba0` latches auto-close after all occupancy observations,
  including paired leaves. `0x0049e920/0x00c3fb00` choose a nearest matching
  leaf within twice the class half-width and 250cm vertically, then test
  its closed state. An already-open nearest leaf does not fall through
  to a farther closed leaf.
- `0x00c3f870` searches players (including pawns), NPCs, enemies (with an
  embedded excluded-kind list) and human enemies. `0x00422650` tests an
  inclusive 400cm horizontal / 250cm vertical cylinder. Runtime accepts
  all eligible actors, with no global player.
- Opening is an actor request `0x1a`, sent by `cPlActOpenDoor` at
  `0x00afc730`. The player interaction path `0x00b6eac0` tests input bit
  `0x1000`; proximity alone is not treated as an opening request.
- Open action `0x00c1f220/0x00c1f420` chooses motion 3 or 4 from the
  actor's side, disables both h00/e00 collision objects immediately,
  and selects hold motion 1/2 on authored motion completion. Close action
  `0x00c1f4c0/0x00c1f5a0` chooses 5/6, restores collision only on completion,
  and returns to motion 0. Each motion switch requests 5-frame blending;
  sound requests are 0x28/0x29. Installed Cassardis types 7060/7120/7121
  have all seven slots (holds: 2 frames; transitions: 61 frames).

Validation: headless phase/pair/occupancy/name tests and installed
candidate/model/collision/motion checks pass; strict targeted clippy
passes. No original-game input or viewer render comparison performed yet.

Remaining: host ownership/frame hooks, presentation of authored models
and clips, dynamic player/camera collision, per-seat interaction selection,
lock/key/night/save conditions, sliding/special door variants, and bounded
original/viewer comparison. Core candidates deliberately do not pretend
to resolve persistent object records or resource readiness. Next action:
wire the regular hinged doors into the host and the stage renderer.

### Host/collision checkpoint

The host now owns door requests, authored clip frames, occupancy, and replay
hash state. A shared h00 terrain overlay toggles player and camera geometry
without duplicating base terrain; streamed base-ground replacement has an
explicit wrapper API. Canonical loaded NPC positions join all player/pawn
seats in occupancy. Other enemy manager admission remains unwired.

Validation: 7 dddoor + 7 ddhost unit tests pass. Installed Cassardis house
regression opens the real group18/record2 door, holds it open with a second
actor, closes after departure, restores collision, and refuses unresolved
persistence. dddoor strict all-target clippy passes; ddhost lib/door-test
clippy passes with existing install.rs chunks_exact_to_as_chunks lint
allowed (unrelated all-target replay lint also predates this change).
The nearest interaction adapter is explicitly opt-in and provisional:
original focus/angle arbitration still needs connection. Viewer rendering
and original-game comparison remain pending; the brief is not complete.

### Presentation checkpoint

The opening adapter instantiates 23 authored hinged models and clips in
Cassardis; the host collision Arc is also used by the follow camera.
Loaded `_p` layouts are cached and GPL gates remain live. Static stage
inspection can show authored closed door poses with explicit preview
scene/hour and clear layout flags. Stage handoff clears the old runtime;
retained candidates keep their host motion state on a gate/cell rebuild.

Paired-message review confirmed `0x00c3fb00` queues `0x1a` through
`0x00887250`, and `0x00c3f130` accepts it without an availability check.
A regression therefore checks that a known locked leaf follows its
unlocked partner. Unknown persistence is conservatively excluded.

Host occupancy now includes fight goblins and explicitly marks missing
manager coverage. The fresh Cassardis `_e` audit finds admitted uEm8300
and uEm8600-family actors; the former's ctor `0x00a9bd00` writes kind
0x65, the latter's `0x00aa0050` writes 0x6b (excluded by door queries).
Head #2725 explicitly authorized authored ambient spawn positions as a
viewer stand-in until the ambient-creatures brief supplies moving actors.
The adapter labels this stand-in; unknown admitted families still prevent
closing. This is not an original-game actor simulation comparison.

Validation: 7 door + 7 host unit tests and installed host regression pass,
including unknown-coverage hold, second actor occupancy and paired locked
leaf. Cached placement candidates equal the uncached installed candidates.
Release build passed. Hidden bounded opening smoke loaded all 23 doors
with coverage=true, then hit the older NPC Set Disable Flag blocker;
updating to the already-merged NPC fix precedes the next smoke. Static
capture confirmed the real door mesh, but its stage inspection path needed
the same nonfield stage-model hook used by opening (now added, pending
capture). Private captures/logs remain under E:/ddda-re/notes/doors-codex32.
