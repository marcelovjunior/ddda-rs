# opening-perf: Frame rate in Cassardis and on the field

- **Owner:** codex lane
- **Branch:** `task/opening-perf` (merged diagnostics), `task/opening-perf-route` (continued measurement)
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
warnings). `cargo clippy --release -p ddview --no-deps` also passed (19
existing warnings; no new diagnostics/clock warning). Bounded hidden run, with no other ddview process or concurrent
build, used:

```text
ddview <nativePC> --opening --skip-intro --unattended --perf
  --size 1920x1080 --shot <private>/room-before.png --shot-delay 35
```

Hardware/backend: RTX 4060, NVIDIA 610.62, Vulkan. The screenshot was
inspected at 1920x1080 and shows the Cassardis class-selection room.
Six warm five-second windows measured median **6.9 ms**, p99 **7.6â€“8.4
ms**, worst **10.4 ms**, and **zero frames over 16.67 or 33 ms**. The
initial window includes a **523.2 ms** startup hitch (two frames over
33 ms), so startup is not included in the steady-state result. No
steady-state room slowdown was reproduced at this pose.

Scene counts: 13,331 entities, 3,824 mesh entities, 318 visible meshes
(279 unique mesh assets), 166 visible DD material assets (1,663 allocated).
Last reported opaque GPU pass averaged 0.85 ms; character animation was
about 0.82 ms CPU. LOD scan averaged 0.01 ms. The opening setup returns
before inserting the generic `GameClock`, so its clock timer measures an
early return here, not an active relighting workload. Existing
`Loader::models` and per-model `mat_cache` already
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

### Supplemental field baseline (2026-10-09)

The same copied binary was run hidden and bounded for 35 seconds, with
other agents holding builds and viewer captures. The original game stayed
paused with background execution off. Command:

```text
ddview <nativePC> --play --unattended --perf --stage 100 --cell 64,54
  --spawn 1337.184,31001.443,3614.313 --players 1 --clock 08:00
  --size 1920x1080 --shot <private>/field-before.png --shot-delay 35
```

The spawn is the installed `scr\st100\etc\st100` start-position entry 2,
converted to cell-local coordinates. The screenshot was inspected and
shows the field outside Cassardis's gate. This is **static supplemental
coverage**, not the opening route or walked streaming acceptance.

Excluding the initial window (629.5 ms worst, three frames over 33 ms),
six windows measured median 6.9â€“7.4 ms, p99 9.7â€“31.7 ms, worst 55.1 ms,
37 frames over 16.67 ms and five over 33 ms. All five over-33-ms frames
were in the final window near screenshot capture; their cause is not yet
attributed, so they are not discarded. Earlier warm windows peaked at
22.0 ms. Clock work peaked at 0.4 ms; streaming at 0.5 ms after startup.
Opaque GPU pass averages were roughly 3.6â€“4.2 ms. Scene counts were 1,461
entities, 763 meshes, 219 visible meshes (195 unique), 171 visible DD
materials (688 allocated). Logs and screenshot are private under
`E:/ddda-re/notes/opening-perf-codex29/field-before.*`.

The copied demo log from codex-31 provides street route reconnaissance,
but overlapped a build and cannot establish acceptance timing. Its route
still ends at q2 boot; it does not walk into the field. Next measurement
must isolate the final-window field spikes and provide a deterministic
street/field walk before optimization or a completed gate can be claimed.

### Route harness follow-up (head assignment #2535)

`--perf-route --size 1920x1080` selects the opening demo, skips the intro,
enables diagnostics and unattended operation, and rejects screenshot
options. The existing room/Quina/q2 route continues to the installed
stage200 door entry (task 2, record 0), turns to its authored interaction
angle, and supplies player interaction input. It uses the existing actual
JumpArea handoff. In stage100 it walks toward the installed
`q0002_event1` scenario volume until the simulated player crosses a cell
boundary, then waits 30 seconds and exits. Failure to complete within
600 seconds exits with an error. No new placement or quest/NPC callback
shortcut is added; the pre-existing opening actor stand-ins remain a
separate route-fidelity limitation.

Unattended runs now use Bevy's continuous update mode. The local
bevy_winit 0.19.1 default (`WinitSettings::game`) otherwise uses a
60-Hz low-power update policy while unfocused. `--perf` reports the
number of unfocused frames per window. Earlier baselines did not record
focus state, so this is a removed measurement confound, **not yet an
attribution of their hitches**. Ordinary interactive update policy is
unchanged.

Release build and targeted clippy passed (existing warnings only). The
first bounded route run walked through the Cassardis gate and loaded
stage100/start2 at (41337.184, 31001.443, 143614.31), cell (64,54).
It then correctly stopped with the existing unimplemented stage100
scenario detection rule. The core owner is implementing that prerequisite
from cAreaHit::vf07 (0x00742810); no harness bypass is introduced. This run
overlapped builds and is functional validation only, not acceptance timing.

The run also exposed a harness-only blocking A* search: opening step
336.6 ms, frame 342.7 ms, yielding a 196-waypoint collision-safe path.
The demo planner now retains its frontier across frames and yields between
node expansions after a 1 ms work budget (an individual expansion can
exceed this budget). While planning it supplies neutral player input,
never changes player placement. A regression compares one-expansion
slices against the uninterrupted route around a wall; that and the two
installed Cassardis door/village navigation tests pass in release mode.
Live timing of the incremental planner and the completed field cell
crossing remain pending the core fix. No new performance acceptance claim.


### Clean route checkpoint (2026-10-09, 23:47:27–23:51:08 UTC)

Branch `task/opening-perf-route`: harness `c41e7be`, incremental planner
`68f78a3`, plus the core owner's field detection `952176d` cherry-picked
as `195c1ca`. Combined release build passed. Shared-target reuse initially
selected a stale ddstory artifact; cleaning only that release package and
rebuilding resolved it. The final planner clippy has existing warnings only.

Ran the copied executable hidden with `--perf-route --size 1920x1080`.
All Codex builds and GPU runs were held; a board-wide hold was posted.
The original stayed paused with background execution off. Owned PID31060
exited and all holds were released. Private evidence:
`E:/ddda-re/notes/opening-perf-codex29/route-clean.stderr.log`,
`route-clean-summary.json`, and `route-combined-build.log`.

The route completed room exit, Quina, the village walk, q2 event, and the
real gate handoff. It entered stage100/start2 at
(41337.184, 31001.443, 143614.31), cell (64,54). The previous field detection
error is gone. The next host frame stopped explicitly on `scenario task 13`;
there was **no field cell crossing or 30-second field settle**. The original
callback dispatcher 0x00798ce0 returns null for tasks outside 1–11, and
0x00798250 treats a null callback as result zero without changing terminal
status bits. This sourced task13 correction is handed to the core owner;
the harness does not skip the failure.

Five-second windows grouped by the current demo step (boundary windows
can include the preceding step, and room/q2 include cutscenes):

| Step | Windows / frames | Median range (ms) | p99 range (ms) | Worst (ms) | Frames >16.67 / >33 ms |
| --- | --- | --- | --- | --- | --- |
| RoomExit, including startup | 12 / 8,263 | 6.9–7.0 | 7.4–44.5 | 752.1 | 61 / 20 |
| TalkQuina | 2 / 1,209 | 7.0–7.4 | 22.1–55.3 | 76.8 | 26 / 17 |
| WalkOut | 2 / 1,176 | 7.5–7.9 | 11.9–19.1 | 28.4 | 7 / 0 |
| Q0002Event, walk and cutscene | 25 / 11,776 | 7.1–17.5 | 8.0–22.9 | 27.2 | 1,322 / 0 |

The long village search again produced 196 waypoints, spread over 375
calls: the first report covered 323 calls averaging 1.09 ms, worst 2.0 ms;
the next covered 52 averaging 1.05 ms, worst 1.5 ms. A shorter 10-waypoint
search took five calls, averaging 1.41 ms, worst 2.7 ms. This validates the
harness hitch removal structurally and in a live run; it is not a matched
render optimization comparison. Focus logging captured both focused and
unfocused operation with continuous updates; no claim is made that focus
caused the earlier field spikes.

**Implemented/tested:** bounded route driver through the gate, continuous
unattended updates, focus diagnostics, incremental collision planner,
three release navigation regressions, release build and targeted clippy.
**Measured:** clean room/street/cutscene windows and live planner slices.
**Not accepted:** steady60fps, walked field streaming, full authored NPC
route, or visual comparison against the original. No rendering or LOD
optimization was made. Next action: merge the core owner's source-correct
task13 dispatch, then rerun the route when the coordinator schedules it.
Camera-feel work takes priority over another perf run (head2666).
