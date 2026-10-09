# opening-perf: Frame rate in Cassardis and on the field

- **Owner:** codex lane
- **Branch:** `task/opening-perf`
- **Board area:** `render/perf`
- **Status:** in progress (codex-29)

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

### 2026-10-09: instrumented room baseline

Added opt-in `--perf` Bevy frame and render-pass CPU/GPU diagnostics,
five-second p95 / over-16.67-ms / over-33-ms counts, and visible mesh and
material counts. The visibility counts cover all views and are explicitly
**not draw-call counts**. Added a clock section timer. Ordinary runs do
not enable the render diagnostic queries. No rendering rule or asset was
changed in this checkpoint.

Release build passed at base `87843ee` plus these diagnostics (two existing
warnings). Bounded hidden run, with no other ddview process or concurrent
build, used:

```text
ddview <nativePC> --opening --skip-intro --unattended --perf
  --size 1920x1080 --shot <private>/room-before.png --shot-delay 35
```

Hardware/backend: RTX 4060, NVIDIA 610.62, Vulkan. The screenshot was
inspected at 1920x1080 and shows the Cassardis class-selection room.
Six warm five-second windows measured median **6.9 ms**, p99 **7.6–8.4
ms**, worst **10.4 ms**, and **zero frames over 16.67 or 33 ms**. The
initial window includes a **523.2 ms** startup hitch (two frames over
33 ms), so startup is not included in the steady-state result. No
steady-state room slowdown was reproduced at this pose.

Scene counts: 13,331 entities, 3,824 mesh entities, 318 visible meshes
(279 unique mesh assets), 166 visible DD material assets (1,663 allocated).
Last reported opaque GPU pass averaged 0.85 ms; character animation was
about 0.82 ms CPU. LOD scan averaged 0.01 ms; clock section rounded below
0.01 ms here. Existing `Loader::models` and per-model `mat_cache` already
share repeated meshes/materials; the brief's no-batching hypothesis must
be measured rather than assumed.

Evidence remains private under
`E:/ddda-re/notes/opening-perf-codex29/room-before.{png,stdout.log,stderr.log}`.
This is a measured viewer baseline, **not comparison against the original**
and **not completion of the route performance gate**. No before/after
optimization comparison exists yet. Startup logs also report unavailable
fight data and untranslated NPC schedule variables; codex-31 is checking
those as route blockers.

Next: measure the walked streets and field streaming using the route
integrator's bounded harness, identify the actual slow pass/system, and
compare the same screenshot pose after any optimization. Head owns the
concurrent missing-ground LOD correction; do not alter that rule here.
