# opening-perf: Frame rate in Cassardis and on the field

- **Owner:** codex lane
- **Branch:** `task/opening-perf`
- **Board area:** `render/perf`
- **Status:** open

## Goal

The maintainer finds the look "extremely close if not 1:1" but the frame
rate is what makes the viewer unplayable. Head's batch took the worst
Cassardis frame from 46 ms to 12 ms: the relighting is spread at 64
materials per frame, the sun shadow uses one cascade over 60 m, parts
without the shadow pass get `NotShadowCaster`, and LOD follows the
original's rule (`event::lod_update`). The maintainer still reports low
FPS. Find where the frame goes and fix it, without changing the picture.

Targets: a steady 60 fps or better at 1080p in the Cassardis class-
selection room, the Cassardis streets and the field out of the gate
(`--opening --skip-intro`). No frame over 33 ms while walking, including
when field cells stream in.

## How

- Measure first: `--perf` (perf.rs sections), plus Bevy's frame-time and
  render diagnostics; count draw calls, meshes, materials and pipelines
  per view. Record the numbers before and after in Results.
- Likely suspects, in order to check: draw-call count (one entity per
  part, unique `DdMaterial` per part, so no batching: dedupe materials,
  share meshes), the shadow pass (casters in view, cascade size),
  per-frame systems that scan every entity (`lod_update`, sky, clock),
  streaming spawns on the main thread, and transparent sorting.
- The original's own culling and LOD rules are in docs (LOD 0x00EA5320,
  view ranges from `uBaseModel`); prefer the original's rules over
  inventing new ones, and label any viewer-only optimisation.

**Rules.** Same picture: compare screenshots before and after the same
`--shot` run. Test runs use `--unattended`.

## Scope

`crates/ddview/src/{stage,event,main,perf,stream,sky,clock}.rs`, plus
this brief's Results. Coordinate on the board with anyone holding
`crates/ddview/src/play.rs` or `opening.rs`.

## Done when

The targets above hold in a `--perf` log. Fill in Results and report to
`head`.

## Results

(filled in by the agent)
