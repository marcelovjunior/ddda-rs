# S41: Frame rate: profile the opening and the field, aim for 180 fps

- **Branch:** `step/s41-frame-rate`
- **Files:** wherever the time goes (`crates/ddview` rendering, streaming,
  animation, host frame); `docs/` perf notes; perf tests/benchmarks
- **Kind:** measure, then fix the biggest costs (repeatable)

## Context

The maintainer's machine: Ryzen 7 5700X3D + RTX 4060, 1080p. The game
targeted the PS3, so 180 fps or more should be reachable. Today the
field (`--perf-route`, stage 100) logs median ~31 ms, p95 ~39 ms (about
30 fps), with ~1050-1080 visible meshes, ~450 visible DD materials of
2003 allocated, ~4800 entities (perf.rs). The older opening-perf brief
(docs/tasks/opening-perf.md) has background; this step supersedes its
goal.

## Do

1. Measure first: CPU frame breakdown (Bevy systems, our own sections in
   perf.rs: host frame, character::animate, stream, ...) and GPU passes
   (render diagnostics already logged), in Cassardis and on the field.
   Use Tracy or Bevy's trace if needed. Write the breakdown down.
2. Fix the largest costs, one commit each, re-measuring: e.g. draw-call
   and material batching (unique meshes vs instances, material
   duplication), CPU animation/skinning work for off-screen or distant
   characters (the original's LOD and update culling), streaming work on
   the main thread, shadow-map cost, per-frame allocations.
3. Uncap the frame rate in the viewer (present mode, no artificial
   limiter) while the simulation stays at its fixed 30 Hz with
   interpolation; check nothing gameplay-related depends on render fps.
4. A repeatable benchmark (`--perf-route` numbers) recorded in the Result
   before/after each fix.

## Done when

The Result shows the before/after breakdown and the field route's median
frame time at or under 5.5 ms (180 fps) on the maintainer-class GPU, or
names exactly what still blocks it with measurements.

## Results (in progress)

- Branch `step/s41-frame-rate`, worktree
  `E:/ddda-rs-wt/s41-frame-rate`; reviewed source base `10da58b`.
  Current source checkpoint `9f6ab4d` includes reviewed main `0eb3a1b`.
  No renderer optimization has been measured or claimed yet.
- `--perf` and `--perf-route` already select `AutoNoVsync`; unattended
  runs already use continuous Winit updates. The host keeps its existing
  fixed-step simulation. There is no remaining assumed refresh-rate cap
  to remove in these modes.
- Added optional `profile-trace` feature, using Bevy 0.19.1's
  `trace_chrome` system/schedule/render spans. Set `TRACE_CHROME` to a
  private JSON path. This adds tracing overhead and is **not** the build
  used for acceptance frame times.
- `DDVIEW_PERF_SECONDS=<positive seconds>` makes a perf capture exit
  through ordinary `AppExit::Success`, flushing the trace without a
  screenshot or forced stop. Its clock starts at the first perf update,
  after startup preparation. Timing windows are labelled with their
  current stage and reset at stage handoff.
- Repeatable launcher `scripts/perf_capture.ps1` records a private pinned
  executable's SHA-256, arguments, elapsed duration, numeric OS exit,
  and read-only GPU engine samples including the original game's PID.
  The caller must hold `live/ddview` until actual exit. Example arguments
  and private paths are in the script. It does not control the original.
- `scripts/perf_trace.py` summarizes bounded warm CPU trace windows by
  thread and span; inclusive and exclusive spans are kept separate and
  overlapping threads must not be summed as frame time. It streams the
  tracing-chrome event array instead of retaining multi-gigabyte captures,
  requires explicit frame-thread selection when several threads emit the
  marker, and normalizes Bevy's empty formatted-field suffix. Nine
  regression tests pass, including clipped child accounting and explicit
  reporting of terminal open scopes. Open scopes are never counted as
  completed frames; the opt-in allowance requires independently verified
  normal process exit. Launcher PowerShell syntax check passes.
- Initial source trace build **failed with exit 1**: merged S34's new
  `Victim::Npc` was missing from three `ddnet/src/mirror.rs` matches
  (431/446/489). Reported on board #5007/#5012 to the S34 owner; no
  renderer performance conclusion follows from this build failure.
  Head fixed the matches in `e268c65`, integrated on this task branch as
  `c5e0760`. Subsequent trace release build from `aab579d` exited 0 in
  6m24s; default release tests from `9f6ab4d` passed 71, with two existing
  ignored tests (21.76s compile, 12.05s test). Default release build exited
  0 in 49.56s. Existing private-interface/audio warnings remain.
- Launcher additionally supports explicit `-Backend vulkan|dx12` (default
  `auto`) and records optional NVIDIA utilization/clock/power-state CSV.
  This enables an unchanged-scene experiment: Bevy 0.19.1 release
  `WgpuSettings` retains indirect-call validation when its requested
  backends include DX12, even if the selected adapter is Vulkan; an
  explicit Vulkan request removes that flag. **Hypothesis only**, not a
  measured improvement or a new default; the identical-scene experiment
  below did **not** improve the median.
- Head #4984 requires the original game to remain running for maintainer
  probes. Read-only original PID 23408 3D samples before our captures were
  25–31%. Relative before/after samples must record its concurrent share;
  isolated absolute 180 fps acceptance needs a later maintainer run with
  the original closed. Original-game GPU share during the actual warm
  captures below was 9–17%, at a different GPU clock from that idle probe.
  The original was neither controlled nor closed.

### Private executable provenance

Private binaries, traces and recordings remain outside the repository.

| Build | Source | Private pin | SHA-256 |
| --- | --- | --- | --- |
| Default | `9f6ab4d2f390a9980b2cf700aa242f4152658824` | `E:/ddda-re/notes/steps/s41/ddview-default-9f6ab4d.exe` | `45A458C289C8B1EF48B4DB7B930D3F6EECEB6B54AEC8A9B6932CBBAD576BF57C` |
| Trace (`profile-trace`) | `aab579dde2c030da69e3b8ed31ebb059d0138004` | `E:/ddda-re/notes/steps/s41/ddview-trace-aab579d.exe` | `917799DE6833F07C153F08DF8B4B490CE0685A50C5A5D38B8443C31B6AFEFFE9` |

Trace pin predates the NPC body materialization merge in the default pin.
These standalone `--play` captures create no `OpeningSession`; their
rendering/geometry source is identical. Neither older pin should be used
to validate the subsequent S40 camera route fixes.

### Standalone field measurements (not the ordinary route)

Identical arguments for all three captures:

```text
E:/SteamLibrary/steamapps/common/DDDA/nativePC
--play --stage 100 --cell 64,54
--spawn 138.387,31224.596,3931.14
--pos 138.387,31400,4431.14 --look 138.387,31400,3431.14
--players 2 --clock 12:00 --perf --unattended --size 1920x1080
```

Spawn is the recorded S32 field point `(40138.387,31224.596,143931.14)`
relative to cell `64m54n` origin `(40000,0,140000)`. This normal standalone
benchmark setup does not move actors or acknowledge quests in the route.
The scene has 1907 entities, 982 meshes, 365 visible meshes (263 unique),
208 visible DD materials / 745 allocated. The ordinary route previously
had about 4744–5314 entities, 1712–1971 meshes, 1068 visible meshes and 479
visible materials / 2021 allocated. These are different scenes: the static
result is not evidence of an improvement to the ordinary route.

| Capture prefix under `E:/ddda-re/notes/steps/s41/` | Owned PID | Duration including startup | OS exit | Warm 5-second window medians, elapsed 20–55s | Median of those medians |
| --- | --- | --- | --- | --- | --- |
| `field-default-9f6ab4d` | 20220 | 67.220285s | 0 | 8.8, 8.8, 9.0, 8.9, 8.8, 8.9, 8.8ms | 8.8ms |
| `field-vulkan-9f6ab4d` | 30364 | 67.3577866s | 0 | 18.9, 10.2, 9.5, 8.8, 8.8, 8.7, 8.8ms | 8.8ms |
| `field-trace-aab579d` | 26276 | 52.3729081s | 0 | Instrumented; excluded from acceptance | See CPU trace below |

The first two ran for 60 seconds after the first perf update; the trace
ran for 45 seconds. Default warm p99 windows were 12.1–14.6ms. Explicit
Vulkan warm p99 windows were 11.4–37.1ms, retaining the transient early
slowdown rather than discarding it. There is no measured reason to change
the default backend. NVIDIA warm graphics/memory clocks were stable at
2745/8501MHz (P0). The Vulkan slowdown coincided with lower total GPU load
(one 37% sample); its cause is unresolved, not attributed to clock changes.
Original process PID 23408 warm 3D utilization was 13–16% in the default
capture and 9–17% in the Vulkan capture. Viewer share was 57–67% and
29–69%, respectively. Harness metadata, GPU samples and logs preserve the
coactivity evidence. Root's short core review overlapped startup/warm-up
of the default capture; the reported steady windows were after it ended.

Default GPU diagnostics (median of eight logged warm averages) measured
opaque 4.393ms, bin unpack 0.693ms, clustering 0.068ms, early mesh 0.024ms,
transparent 0.024ms, upscale 0.100ms and UI 0.005ms. Opaque is the largest
measured GPU pass. CPU encoding diagnostics for that pass are not GPU time.

### CPU trace breakdown

The 6,119,163,198-byte private trace was streamed successfully (analyzer
OS exit 0). Window 30–45s, explicitly selected Chrome `(pid=1, tid=0)`;
metadata names that thread `main`, and it is the only thread emitting
`update`. It contains 1546 complete frames, median 9.219ms, p95 12.933ms.
Tracing overhead means these frame times are not acceptance measurements.

| Span / thread | Inclusive wall ms per main frame | Exclusive wall ms per main frame |
| --- | --- | --- |
| Main-thread update | 9.588 | 0.005 |
| Main-thread RenderExtractApp | 6.766 | 5.900 |
| Main app | 2.817 | 0.003 |
| Render-thread Render schedule | 9.124 | 4.542 |
| Render-thread render_system | 4.199 | 0.003 |
| Render-thread queue_submit | 0.997 | 0.997 |
| Render-thread bin unpack encoding | 0.777 | 0.777 |
| Render-thread shadow encoding | 0.445 | 0.445 |

These spans overlap across threads and parents; summing them would be
incorrect. Bevy 0.19.1 `bevy_render/src/pipelined_rendering.rs:184–198`
confirms `renderer_extract` waits for `render_channels.recv()` inside
`scope_with_executor`, then extracts and returns the render world. This
scope may also run pending executor tasks while waiting. Therefore the
5.900ms exclusive wall time mixes handoff/waiting and any untraced work;
it is not a proven 5.900ms CPU-computation cost. Seven terminal open scopes were explicitly omitted
after verifying normal process exit; the two long-lived app/thread scopes
and late shutdown scopes are listed in the private summary. All measured
complete frames end before shutdown.

All owned captures exited and GPU was actually released on board #5375;
head #5358 gives S40's camera12 route priority while this analysis proceeds.
No S41 GUI process or Cargo build remains. Cassardis, the current ordinary
route and same-picture optimization comparison remain **OPEN**. Next:
finish identifying actual render-thread work versus waits from the pinned
Bevy source, measure Cassardis/updated route after S40 releases the GPU,
then test the largest measured cost without changing presentation quality.

### CPU instance-builder experiment (codex-66, 2026-10-10)

- Resumed from the clean prior branch and integrated reviewed committed
  main `861c94e`. Candidate source checkpoint `87a2925` adds only an opt-in
  `DDVIEW_MESH_BUILDER=cpu|gpu` renderer choice. Unset/`gpu` retains Bevy's
  existing default. No simulation, original LOD, geometry or material
  quality changes are part of this experiment.
- Aggregating the old trace's same-named systems across worker threads
  identifies shadow binned-buffer preparation at 1.264 ms, preprocess
  bind-group preparation at 0.938 ms and opaque binned-buffer preparation
  at 0.474 ms of **thread wall work per main frame**. These totals are not
  elapsed frame contributions: workers overlap. Their corresponding
  GPU bin-unpack pass measured 0.693 ms. Animation (0.02–0.03 ms average)
  and streaming (0.06 ms) are not dominant in this standalone scene.
- Pinned Bevy 0.19.1 source: `PbrPlugin::use_gpu_instance_buffer_builder`
  forwards to mesh/preprocess plugins. `render/mesh.rs:245–313` selects
  CPU extraction and batching when false; `render/gpu_preprocess.rs:403–413`
  skips compute-preprocessing systems. CPU `MeshPipeline::get_batch_data`
  and `get_binned_batch_data` retain transforms, skin/morph indices,
  materials, lightmap and mesh grouping. This is a supported renderer path;
  visual equivalence and performance still require the capture below.
- Verified on frozen `87a2925`: `cargo test --release -p ddview`, **79
  passed, 0 failed, 2 existing ignored**, 55.97 s compilation and 10.81 s
  test runtime; `cargo build --release -p ddview`, **exit 0**, 37.17 s.
  Final combined Cargo session 56803 exited 0. Existing private-interface
  and unused audio-method warnings remain. Python trace accounting still
  passes all nine regressions; PowerShell launcher syntax/diff checks pass.
- Private pin `E:/ddda-re/notes/steps/s41/ddview-builder-87a2925.exe`, SHA-256
  `5A6E202CD81F6E1055D27C75A344EE338206843D8BD70AD69CB334CC81BB8902`.
  Shared M5 build lease was released after actual exit. No GUI was launched.
- Launcher now records/restores the CPU/GPU mode and has an explicit hard
  watchdog (`-WatchdogSeconds`, default 120 s), independently of requested
  measurement duration. Board capture request #5940/#5947 queues two
  identical-scene runs from this one pin, default backend, GPU then CPU,
  using the standalone field arguments above plus a screenshot at 45 s.
  The requested 50 s timed exit/90 s watchdog keeps the combined batch
  bounded; compare preselected elapsed 20–40 s windows before screenshot.
  Original PID 23408 stays running and its concurrent GPU share is recorded.
- **Pending:** actual same-picture CPU/GPU frame and pass measurements,
  current full-route measurements, and 180 fps acceptance. No gain is
  claimed from implementing the switch or passing headless tests. Next:
  inspect the queued batch's actual exits, counts, PNGs and warm medians;
  select a default only if comparable captures demonstrate improvement.

### Actual first CPU/GPU comparison: rejected on visual correctness

- Head ran the requested batch (board #5986). Same executable/hash,
  default Vulkan adapter RTX 4060, original PID 23408 retained. GPU viewer
  PID 31764 exited **0** after 62.4 s including startup; CPU PID 31076
  exited **0** after 52.7 s. Both private `.exit.txt` files contain zero.
  Screenshots ended the runs normally before the 50 s perf timer.
- Both modes reported 1908 ECS entities, 982 mesh entities, 365 visible
  meshes / 263 unique, 208 visible DD materials / 745 allocated through
  the warm windows. These counters describe ECS visibility, not successful
  draw submission.
- Preselected elapsed 20–40 s windows (reports at approximately 25, 30,
  35 and 40 s): GPU medians **7.0, 7.0, 7.2, 6.9 ms**, median of medians
  **7.0 ms**; p99 **10.9, 10.4, 11.3, 10.9 ms**. CPU medians **2.1, 2.1,
  2.2, 2.2 ms**, median of medians **2.15 ms**; p99 **4.7, 4.2, 5.0,
  4.8 ms**. The CPU frame times are **invalid as optimization evidence**.
- Actual visual inspection by codex-66 and independently codex-1:
  `field-builder-gpu-87a2925.png` shows intact terrain and both bodies;
  `field-builder-cpu-87a2925.png` shows mostly sky, sliced/distorted terrain,
  and no recognizable bodies. This fails picture equivalence despite
  matching entity/material counts. CPU opaque pass GPU time falls to
  roughly **0.003 ms**, consistent with missing opaque geometry, not
  equivalent rendering made faster. The default remains GPU.
- Warm GPU samples were aligned by UTC with each run's first perf update,
  not the launcher clock (startup lengths differed). All four warm samples
  report original PID 23408's 3D share **0%** in each mode; viewer GPU-mode
  share **47–56%**, broken CPU-mode share **11–15%**. NVIDIA clocks are
  stable **2745/8501 MHz, P0**, overall utilization **84–95%** and **83–92%**
  respectively. Windows 3D-engine counters and NVIDIA total utilization
  sample different metrics/intervals; the nonzero Windows samples belong
  only to the viewer, so their difference does not prove another GPU user.
  Older 8.8 ms measurements had a
  different original-game share/source state and are not a before/after
  optimization comparison with this batch.
- Source diagnosis: setting `PbrPlugin::use_gpu_instance_buffer_builder`
  false selects CPU mesh uniforms, but leaves `GpuPreprocessingSupport`
  at device capability `Culling`. Bevy `core_3d::extract_core_3d_camera_phases`
  then chooses indirect phases, while `no_gpu_preprocessing` only prepares
  ordinary batchable meshes and `DynamicUniforms` batch sets. Multidraw
  meshes are therefore skipped. The correction must set render-app
  preprocessing support to `None` before plugin finish as well as choosing
  CPU uniforms; headless phase-selection regressions are being prepared.
  No shader ABI change or default performance claim follows from the failed
  batch. Next: test that paired contract and queue a corrected CPU picture
  comparison before deciding whether CPU preparation is useful.

### Paired CPU capability and mesh-buffer correction

- Feature commits `1e86872` / `e9eb9e0` add `MeshBuilderPlugin`, installed
  after DefaultPlugins build and before their finish hooks. Opt-in CPU
  mode sets the render-app `GpuPreprocessingSupport` capability to `None`
  alongside the CPU mesh-builder flag. GPU mode leaves device capabilities
  unchanged. The environment choice is cached once so both decisions agree.
  This makes main and shadow phase selection use the CPU batching contract;
  it does not alter custom vertex shader indexing or model data.
- Two meaningful headless regressions construct a real Bevy render sub-app,
  then queue an opaque mesh through `ViewBinnedRenderPhases<Opaque3d>`:
  forced CPU mode puts it in `batchable_meshes` (which CPU batching visits)
  and never in `multidrawable_meshes`; unchanged GPU capability retains the
  multidraw bucket. This catches the failed capture's actual omission path.
- Main-hook ownership was coordinated with codex-67 on board #6025/#6029
  and released immediately after the one-line insertion. Reviewed committed
  main `c7c0505` (NPC HP/gauge fix) integrated as source `a35a00c`.
- Frozen `a35a00c`: full `cargo test --release -p ddview` **81 passed,
  0 failed, 2 existing ignored**, compilation 47.50 s / tests 14.31 s;
  `cargo build --release -p ddview` **exit 0**, 52.00 s. Combined session
  89093 actually exited 0. M5 lease released after the executable was pinned.
- Corrected private pin `E:/ddda-re/notes/steps/s41/ddview-builder-a35a00c.exe`,
  SHA-256 `2DC0A5AD98ABFA1A4D067E5584F6DFC9032815A131BC9D9D0E8118A08D248857`.
  A fresh bounded GPU/CPU request is queued on the board. Both sides must
  use this pin: the old failed CPU picture is never a performance baseline.
  **Pending:** actual corrected PNG fidelity and timings, default selection,
  full-route/room measurements and 180 fps acceptance. Default remains GPU.

### Second actual CPU/GPU comparison: still rejected; startup ordering fixed

- Head completed the `a35a00c` batch on board #6153: GPU PID 24464,
  exit 0 / 58.2 s; CPU PID 8516, exit 0 / 52.4 s. Both `.run.json`
  files record the same pinned SHA-256 above, identical scene/camera/two
  players/default backend, and both `.exit.txt` files contain zero.
- Both PNGs were actually inspected by codex-66 and independently codex-1.
  GPU has intact terrain and both bodies; CPU still has mostly sky,
  sliced/deformed terrain and no recognizable bodies. **Fidelity FAIL.**
  All CPU timing is invalid as an optimization result. Warm 20–40 s
  windows ending approximately 25/30/35/40 s have GPU medians
  7.2/7.1/7.3/7.1 ms (median of medians 7.15 ms), p99
  11.1/10.2/10.9/11.7 ms. Broken CPU medians are 2.9/2.3/2.2/3.0 ms,
  p99 7.3/4.6/5.1/6.0 ms; they do not represent equivalent rendering.
  Visibility counters still match 365 meshes / 263 unique / 208 visible
  DD materials / 745 allocated. Original PID 23408 remains listed and
  has 0% Windows 3D-engine share in all four UTC-aligned warm samples.
- The preceding build-time capability correction was insufficient.
  Pinned Bevy 0.19.1 `bevy_render/src/lib.rs:220` shows
  `init_gpu_resource` unconditionally constructs and replaces a resource,
  unlike `init_resource`. `GpuPreprocessingPlugin::finish`
  (`batching/gpu_preprocessing.rs:91`) registers this for
  `GpuPreprocessingSupport` in `RenderStartup`. It overwrites our build-time
  `None`; the actual CPU log at line 24 again reports full GPU support.
  Earlier queue tests did not execute startup, so they could not detect it.
- Source correction `b41e62d` retains the initial capability override and
  adds a CPU-only `RenderStartup` cap system ordered **after the actual
  `init_gpu_resource::<GpuPreprocessingSupport>`**. It logs the final
  `None` after device initialization. Bevy's `RenderStartup` documentation
  explicitly reruns this schedule on each new `RenderDevice`, so the same
  ordering handles device recovery. A new headless regression reproduces
  unconditional device-capability replacement twice, then queues an actual
  opaque phase each time and verifies the CPU batchable bucket. It
  reproduces resource replacement without constructing a headless GPU.
- Full `cargo test --release -p ddview`: **82 passed, 0 failed, 2 existing
  ignored**, compilation 20.22 s / tests 9.67 s. Default release build:
  **exit 0**, 35.00 s. Combined session 28929 actually exited 0; M5 lease
  released after private pin `ddview-builder-b41e62d.exe`, SHA-256
  `A4D3E774E29172DDEC548BF77D651D7D78A38FF5A347DF0988C95253F9AEFC87`.
- **Pending:** independent source review and actual final resource-mode
  log/picture validation. No further CPU capture was requested before that
  review. Neither passing startup tests nor the new source establishes
  visual correctness or a gain. Default remains GPU; the approved shader
  specialization hypothesis continues on its own separate branch.

### Shader hypothesis rejected; current opening/town measurements next

Separate approved experiment branch `step/s41-material-specialization`,
checkpoint `5d19d88` (feature source `1a0911b`), completed its full viewer
suite (**84 passed / 2 existing ignored**) and default release build, then
head's bounded off/on capture (board #6266). Both modes used the same
private pin SHA-256
`9B9BD7C428AC8C2200AB33AB1387EB7C1143A82A7EEA942E83887D7A77E097CE`
and GPU mesh builder. Owned PIDs 33632/33512 actually exited 0 after
52.4/52.5 s. Both pictures were viewed before comparing timings and retain
the terrain/two bodies/shadows/HUD; 2,019 of 2,073,600 pixels differ, only
23 by more than 10 in any channel, so this static snapshot is visually
equivalent, not byte-identical. Warm visibility/counts match.

Warm 20–40 s frame medians are off 6.7/6.7/6.7/6.7 ms and on
6.9/6.7/6.6/6.7 ms: both median-of-medians **6.7 ms**. Opaque GPU
average-report medians are 3.395866/3.420587 ms. There is **no useful
measured gain**, and specialization remains off/unmerged per head #6283.
Its detailed results remain in the separate branch's
`docs/tasks/steps/S41-material-specialization-notes.md`. This does not
establish full-route/fading/night fidelity or meet the 5.5 ms target.

Head #6283 refocuses S41 on the maintainer's actual opening/road scene
(historically ~22 ms, ~1050 visible meshes / ~2000 materials / ~4800
entities) and Cassardis town. Reviewed committed main `5426528` has been
integrated into this task branch; main was not edited.

Head #6350 approved opt-in stage-specific CPU tracing, source `752c101`:
a `profile-trace` build can set `DDVIEW_TRACE_STAGE`, wait a configurable
wall delay (default 5 s), and record a short span window (default 10 s).
Ordinary events/logs remain enabled before/after it; the window ends in
normal app exit. Default logging is unchanged. Dynamic callsite interest
is essential: a span first seen disabled must be reevaluated when the
window opens. Three regressions cover actual repeated callsites/events,
stage transitions and loading hitches. **Tests/build/capture pending.**
The one-line main hook was normally claimed, committed and released for
codex-70; no opening/play files were edited. The harness records/restores
stage/delay/duration parameters and passed PowerShell parse/diff checks.

`DDVIEW_PERF_SECONDS` uses `Instant` wall time after its first update;
`--shot-delay` uses virtual time. This distinction was verified in pinned
S47 `2dd8b15`, whose early screenshot ended its x4 run before the field;
it was not the wall-time perf timer. The harness's independent hard OS
watchdog can be shorter than the requested benchmark duration.

Source-supported candidate to investigate **after measurement**: streamed
`Loader::spawn_parts` permanently chooses one LOD at load using actor
focus/transform origin, while the whole-stage event path follows the
active camera and transformed model sphere centre each frame. Read-only
canonical Ghidra `0x00ea5320` confirms integer-truncated camera-to-centre
distance, model-header mid/low thresholds, FAR x2 and FARTHEST near;
`0x00ea3d00` transforms the model sphere. No LOD/draw distance/content has
been changed, and the discrepancy is not yet a measured dominant cost.

Next action: run full default/trace viewer acceptance under the normal
shared-target lease after root's integration checks, then obtain current
route/town baselines and a bounded field trace. Keep GPU default and all
performance acceptance open; no CPU mode or shader optimization accepted.

### Current checkpoint trace preparation accepted headlessly

Frozen source `84ed50f` includes reviewed main `4a88b39` (S49 perception),
S46 resume/termination fixes `7e370f1` / `f0e4e6a` and the bounded trace
implementation `752c101`. Harness metadata now records/restores trace
configuration and records checkpoint path/SHA-256, demo step, fight and
time-scale settings; fractional durations use invariant formatting.

- Full default viewer suite: **86 passed, 0 failed, 2 existing ignored**,
  compilation 40.77 s. Full `profile-trace` suite: **89 passed, 0 failed,
  2 existing ignored**, test runtime 12.51 s. The dynamic-callsite,
  actual-stage/wall-delay/hitch and stage-leave tests all explicitly passed;
  S35's authored-line-endings test is present, guarding against stale
  shared-target test artifacts.
- Trace release build **exit 0**, 44.25 s; default release build **exit 0**,
  44.64 s. Combined session 31555 actually exited 0. M5 was normally leased
  and released after both pins, with codex-70 next. Only independent
  existing leased targets overlapped headless compilation; no performance
  acceptance may overlap compilation. Existing warnings remain.
- Trace private pin `E:/ddda-re/notes/steps/s41/ddview-trace-84ed50f.exe`,
  SHA-256 `23194B5592E359D2FCB1FE0CF65E7DE94C9FB5B3422BAB3E44D6EA283DF1178A`.
  Default private pin `E:/ddda-re/notes/steps/s41/ddview-default-84ed50f.exe`,
  SHA-256 `59BC3AD5997E633BD864C81717F16761DFC499B51D7F37DEEB06C91C1B9B1986`.
- Head #6401 requires GUI checks to restore its current private S46
  checkpoints, regenerated when save formats change. Proposed batch #6455
  uses x1 default `FieldSettle` and `WalkOut` saved-host baselines with the
  demo driver disabled, then x4 `FieldWalk` with the trace feature and
  stage-100 5 s delay / 10 s window. Checkpoint identity is recorded at
  launch. This proposal awaits head's decision and head #6460's rescue
  capture has priority. No own GUI was launched.
- **Pending:** actual trace startup/window markers, bounded file size,
  normal process exits, scene/image suitability, current opening/town
  baseline frame/pass counts and source-supported optimization. Real x1
  rendering is the FPS acceptance mode; accelerated x4 trace/visual runs
  are diagnostic only. No gain or optimization default is claimed.

Next: obtain the approved current-scene batch after head releases the GPU
and compilation is quiet; stream the short field trace with the committed
analyzer, separating renderer handoff/waits from actual system work.

### Actual saved-host baselines and cached-span profiling limitation

Head #6493/#6592 approved the frozen `84ed50f` batch. Under a normal
`live/ddview` lease, all compilation was held quiet (#6604/#6579). Each
viewer used the recorded private checkpoint, 1920x1080, GPU mesh builder,
and the immutable default or trace pin above. The original PID 23408
remained running. All three owned PIDs were verified absent after their
normal numeric exit 0; the GPU lease and compilation hold were released
immediately (#6638).

| Capture | Host/checkpoint configuration | Actual PID / exit / wall duration |
| --- | --- | --- |
| Field baseline | `FieldSettle`, x1, demo driver/fight disabled | 28368 / 0 / 60.9331 s |
| Room baseline | `WalkOut`, x1, demo driver/fight disabled | 18416 / 0 / 59.9012 s |
| Field CPU trace | `FieldWalk`, x4, demo fight enabled; 5 s delay / 10 s trace | 23560 / 0 / 30.1599 s |

The first two PNGs were actually viewed by codex-66 and independently by
root (#6658): the field has intact terrain and dressed bodies. Contrary
to its checkpoint name, `WalkOut` restores the **interior room**, stage
200/SceNo1200, with intact geometry and characters; it does **not** supply
the requested outdoor Cassardis benchmark. Private prefixes are
`E:/ddda-re/notes/steps/s41/field-checkpoint-before-84ed50f`,
`town-checkpoint-before-84ed50f` (room despite the filename), and
`field-checkpoint-trace-84ed50f`; run metadata, hashes, exit files,
stderr, GPU samples and screenshots stay private.

Warm reports ending at 25/30/35/40 wall seconds (before screenshots at
45 s) give the following **baseline**, not an optimization comparison:

| Metric | Field | Interior room |
| --- | --- | --- |
| Frame medians, ms | 14.1 / 14.2 / 14.1 / 14.3 | 8.6 / 8.8 / 9.1 / 8.7 |
| Median of those medians | 14.15 ms (~71 fps) | 8.75 ms (~114 fps) |
| p99, ms | 15.8 / 15.9 / 16.3 / 20.2 | 11.7 / 11.1 / 11.8 / 10.1 |
| Entities / mesh entities | 4836 / 1802 | 14423 / 4008 |
| Visible meshes / unique meshes | 1071 / 951 | 418 / 374 |
| Visible DD materials / allocated | 423 / 1979 | 209 / 1729 |
| Bin unpacking GPU, median of average reports | 1.901124 ms (5 reports) | 1.064627 ms (5) |
| Opaque GPU, median of average reports | 1.587558 ms (5 reports) | 1.306304 ms (4) |

These GPU report medians are not per-frame distributions. Original-game
3D utilization was zero in all 3/4 warm samples, but NVIDIA was **P3**,
with core clocks 855–900 MHz (field) / 1140–1215 MHz (room), memory
5001 MHz. They cannot be compared as optimization gains against prior
P0/static-scene runs; matched after captures must report clocks too.

The streamed 85,203,539-byte trace has actual window markers at
13.3709468 / 23.3786632 trace seconds (10.0077 s), no terminal open
scopes, and 633 complete main-thread frames in the trimmed
13.47–23.27 s interval. It reports main `Update` 3.9949 ms/frame,
`PostUpdate` 1.1455, and RenderExtractApp 9.3417 including 8.6798
exclusive. The latter wraps Bevy's pipelined `recv` wait
(`bevy_render-0.19.1/src/pipelined_rendering.rs:184`), **not proof of
CPU compute**. Render-thread `Render` is 14.8229 ms/frame including
8.9693 exclusive; `unpack_bins` encoding is 1.2900 and `queue_submit`
1.5248. These overlapping thread/parent values must not be summed.
Accelerated/instrumented x4 frame times are diagnostic only.

**Actual trace limitation:** there are zero ordinary `system` spans.
Bevy caches `SystemMeta.system_span` during plugin build
(`bevy_ecs-0.19.1/src/system/function_system.rs:52`), before our global
window filter enables recording. A cached disabled `Span` stays disabled
even when dynamic callsite interest changes. The prior regression created
a fresh span on every call and missed this contract. Thus the apparent
render-schedule exclusive cost contains unseen system work/worker waits
and does not identify a dominant bottleneck.

Head #6662 approved replacing the global creation filter with an own
Chrome layer wrapper: preserve cached span metadata, gate entry callbacks,
pair exit decisions through window edges, retain ordinary events, and
test a span created once before the window. Source correction is in
progress; full tests/build and actual system-span capture remain pending.

Head #6571 also approved camera-setting caching, but source inspection
disproved it as an **opening** bottleneck: `play::step` calls
`camera_projection` only under its initial `Local<bool>` guard, then
returns when OpeningRes/NetRes exists; repeated `config_camera_fov`
reads belong only to standalone play. No cache edits or opening gain
claim were made (#6609). It remains a possible standalone/S42 foundation.

Next: validate the cached-span correction headlessly, capture one short
corrected field system trace under normal leases, and target the actual
largest source-supported cost. Outdoor town measurement and the 5.5 ms
acceptance gate remain open; no new renderer default or gain is claimed.

### Capture watchdog handle race correction

Root's actual S47 run (#6716/#6718) exposed a race: a CIM provenance
query found the owned viewer, which then exited before an ID-only
`Stop-Process`; Windows subsequently reused that PID for `rustc.exe`.
No foreign process was stopped, but the harness threw before recording
the viewer's numeric exit. Head #6740 approved hardening this harness.

`perf_capture.ps1` now checks the retained process handle on both sides
of the provenance query and calls that object's `Kill()` rather than
resolving a process by PID. A termination exception is ignored only when
the same launched handle actually exits during a bounded wait; a still
live failure or changed live provenance propagates. The `finally` block
writes its actual numeric exit whenever obtainable. The independent
application timer and hard OS watchdog remain separate.

`scripts/test_perf_capture.ps1` extracts and runs the actual watchdog and
exit-recording AST against in-memory fake process/query/output objects.
**7 passed, exit 0**: already exited; exit during query; exit during kill;
live kill failure; foreign recycled PID with exited original handle;
foreign PID with still-live original handle; verified owned-handle kill.
It launches/stops no process. PowerShell parse and `git diff --check`
also pass. No new live-capture verification is claimed yet.

### Cached-span correction accepted headlessly

Source `061b251` / `35812f0` replaces the creation filter with the own
Chrome callback layer, approved by head #6662 and reviewed by root. Under
the normal M5 lease after root's actual release #6693, combined session
38072 **actually exited 0**, and all owned Cargo/rustc processes were
gone before releasing M5 #6748. Only independent existing leased targets
and root's non-performance visual run overlapped; no FPS acceptance was
performed during compilation.

- Full default suite **86 passed / 0 failed / 2 existing ignored**,
  fresh viewer compilation 37.06 s, tests 15.89 s.
- Full `profile-trace` suite **91 passed / 0 failed / 2 existing ignored**,
  feature dependency compilation 5m13s, tests 14.85 s. All five feature
  regressions explicitly pass: stage/hitch/window transitions, cached
  same-ID entries/boundaries, actual reused Bevy App system, and actual
  Chrome writer output names under LogPlugin layer ordering. Source
  freshness is confirmed by these named tests; output is not a reused
  default viewer artifact.
- Trace release **exit 0 / 46.88 s**; default release **exit 0 / 40.58 s**.
  Existing private-interface/audio warnings remain. No clean/new target
  was used; feature changes legitimately recompiled Bevy trace crates.
- Immutable trace pin
  `E:/ddda-re/notes/steps/s41/ddview-trace-35812f0.exe`, SHA-256
  `9BE13EF31DF7658F67FB5A1ED77F1F3D3374C5AC98E525E620BEDA7B3A418D47`.
  Default pin `E:/ddda-re/notes/steps/s41/ddview-default-35812f0.exe`,
  SHA-256 `E3A6A1D1B0F31AA2F43CA70CAEC130A7CFE5109DC9EDF6AE5E790D486B603944`.
  Cargo resolved the lockfile by removing Bevy Log's now-disabled
  `tracing-chrome` edge; the same already-locked dependency is now direct
  and optional in ddview. This resolver delta is committed with this result.
- Hardened harness source `de8d645`, SHA-256
  `2D079EB05A717A8786BFAEE7676212C363B59FAF08F8D239947C8395971E65D8`,
  has the seven offline actual-AST race tests described above, all passed.
  It does not change the already-frozen renderer source used for the pins.

One bounded corrected trace is requested (#6752), pending the normal GPU
lease and root's approved capture priority. A private copy of the already
measured FieldWalk checkpoint, SHA-256
`D4D5628C8C7E6398768DA98D6C10EEC4F626F661C3B0E1AE41EDD432EDBA968A`,
is frozen at `E:/ddda-re/notes/steps/s41/FieldWalk-84ed50f.ddhs`; canonical
checkpoints are untouched. This keeps the pin's host schema reproducible
when head regenerates canonical checkpoints for S51/ECON v2. Actual named
system spans, bounded window/scope completeness and the largest bottleneck
remain unverified until that diagnostic run. No renderer gain is claimed.

### Actual cached spans restored; release system names require one more fix

Head #6759 approved the corrected `35812f0` pin/frozen FieldWalk diagnostic
run with the retained-handle harness. Own session 93854 **actually exited
0**; owned PID 26260 was verified absent after **31.2239855 s**, while
original 23408 remained running. The GPU lease was released immediately
to root #6775. Root's independent headless compilation overlapped this
x4 instrumented run by agreement: **all timings are contaminated diagnostic
values and cannot be used as FPS acceptance or optimization gains**.

Private `field-checkpoint-trace-35812f0.trace.json` is **835,125,572 bytes**.
It records **474,080 actual system entries**, confirming the callback gate
fixed the missing cached spans. Start/end markers are
14.4022971 / 24.4082144 trace seconds (**10.0059173 s**). The committed
streaming analyzer reports **no terminal open scopes** without an override
and 575 complete frames in the trimmed 14.5–24.3 s interval. The normal
numeric exit file was written by the hardened harness. No new screenshot
was requested or fidelity assertion made for this diagnostic run.

However, the actual **named-system gate failed**: every ordinary system
name is `<Enable the debug feature to see the name>`. In Bevy 0.19.1,
`bevy_utils::DebugName` erases function-derived names in release when its
`debug` feature is disabled; preserving the span is not enough. The prior
Chrome-output test manually named its probe span, which bypassed this
release contract. All system costs therefore collapse into the same
anonymous label and still do not identify the largest specific cost.

Head #6783 approved source `0aa60e8`: add `bevy/debug` to **profile-trace
only**, keep the default build unchanged, assert a real function-derived
System name in the App regression, and have the actual Chrome writer test
run a real App system off/on/off and find its default function name in
paired B/E output. No manual name override remains in that output test.
Full suites/release/new pin and one bounded named-system trace are pending
the normal queue after root, codex-71 and codex-72. Head #6811's low-memory
notice is respected: no Ghidra restart/new instance or new target, and
heavy Cargo should run one at a time where possible.

Separate low-priority source branch `step/s41-camera-settings` at
`203ad96` (`44b3f0a` main hook, reviewed base `959f6a9`) implements the
head-approved standalone settings cache; its full results live in that
branch's `docs/tasks/steps/S41-camera-settings-notes.md`. Source claims
were released. It is **untested** until its queued full viewer/release
acceptance and is not presented as an opening gain.

Next: validate the profile-only names correction, obtain real named
system costs, and propose the first actual opening optimization with
matched x1 FieldSettle before/after captures. The 180 fps gate remains open.

### Named renderer trace gate passed (0aa60e8)

Normal M5 acceptance session 74551 actually exited 0 before release #6891:
full default **86 passed / 2 existing ignored**, full profile **91 passed /
2 existing ignored**, including all five cached-span/window/actual App
name tests. Fresh compiles were 20.53 s / 3m51s; release builds exited 0
in 38.82 s (trace) and 37.94 s (default). Profile-only Bevy debug restores
function-derived names without changing the default feature graph.
Private trace pin `ddview-trace-0aa60e8.exe` SHA-256
`85B0AA93B9E646B4562A6883570C33A8BE7BED3F03E79950A03A959174223E18`;
default pin SHA-256
`F9F123F511336F0259644B67CA4396166C70B105C4C4758EBE06EDED919FD0BB`.

Head-approved three-second diagnostic session 96754 actually exited 0,
owned viewer PID 32748 was absent after 24.3667739 s, original PID 23408
remained running, and the GPU lease was released #6903. The retained-handle
harness recorded the numeric exit. No screenshot or fidelity claim was
requested. This was **instrumented x4 with independent head compilation
overlap**, so its 20.56 ms median is contaminated and is not FPS acceptance.

Actual trace: 217,446,490 bytes, 116,784 system entries, **731 unique real
system names**. Window markers 14.1935162–17.2021869 span 3.0086707 s.
The strict streaming summary trims to 14.3–17.1 s, 132 complete main-thread
frames, **zero terminal open scopes**, without an override. Actual Render
thread is tid 2; tid 1 is a compute worker. Summary, logs and run/exit
metadata stay private under `field-checkpoint-trace-0aa60e8`; raw Chrome
JSON is deleted after this result, per head #6862 disk policy.

Largest normal system costs, summed across workers and normalized by
complete main frames, are CPU core wall time and **not additive critical
path**: `prepare_skins` 3.1707 ms/frame; shadow
`write_binned_instance_buffers` 2.8715; `opening::step` 2.6469 (x4 host);
`prepare_preprocess_bind_groups` 2.0611; opaque bin writing 1.1142;
`character::animate` 0.6987; `extract_skins` 0.4860. Main RenderExtract
exclusive ~10 ms still includes pipelined synchronization wait.
Bevy 0.19.1 `skin.rs:176` shows prepare_skins predominantly uploads its
staging buffer through RenderQueue, while extraction calculates matrices.
Calling that 3.17 ms animation computation would be incorrect.

Head #6923 requests source-backed palette sharing/part reduction and
native shadow-caster culling research before matched x1 A/B. Bevy allocates
each visible SkinnedMesh's full palette independently (`skin.rs:add_skin`);
our `character::spawn_meshes` passes the model's full joint vector to every
submesh. Sharing only the bind-pose asset does not share the renderer
allocation. Bevy already removes invisible skins, but its staging vector
does not shrink when allocation is freed; culling alone does not prove
upload savings. Actual upload byte counts/part palettes and safe material
grouping remain to investigate; no optimization or gain is claimed yet.

Compatible real v1 FieldSettle is frozen privately as
`FieldSettle-84ed50f.ddhs`, SHA-256
`D2EA9F3946E6460216B8C04F11BEEA11CC74FD6C195152A28C85744136B34543`.
Canonical checkpoints are untouched and may now become ECON v2 after S51;
older S41 pins must use only their matching frozen v1 saves.

### Exact compact-palette experiment (head #6959; acceptance pending)

Opt-in `DDVIEW_COMPACT_SKINS=1` compacts each submesh palette to all joint
indices referenced by its four retained influences. Source-order remapping
preserves the joint entity and inverse-bind matrix together; zero-weight
lanes are included because Bevy's current/previous skinning shaders load
their matrices. Invalid inputs or a completely referenced palette retain
the original indices transactionally. Geometry, weights, materials,
transparent sorting's original bone references and visibility remain
unchanged; GPU mesh preprocessing and the default renderer remain enabled.

Two prepared regressions compare exact weighted matrices across animated
translation/rotation/nonuniform scale, actual Bevy posed AABB calculation,
geometry/weights/triangle indices, and invalid/complete palette fallback.
`DDVIEW_SKIN_DIAGNOSTICS=1` reports actual Render SkinUniforms staging
extent/bytes after prepare_skins, including high-water retained storage;
it is not labelled live palette bytes. Capture metadata records both
experiment flags. PowerShell parse and seven actual-AST watchdog tests
pass after this metadata-only addition. Parent reviewed the source #7020
without objections. **Rust compilation, full default/profile suites,
release pin and matching x1 off/on capture remain pending** the agreed
one-heavy-Cargo queue S54 core → S42 viewer → S41 viewer. No gain claimed.

### Native caster-depth investigation: constructor cap disproved live

Existing private decompilation and read-only analysis-exe strings establish
`uShadow::reflection` 0x00ef9880 maps +0x60 to
`mBackforwardViewDistance` (string 0x014340f0). Constructor 0x00ee9ec0
sets 4000 cm, inherited by stable/sky shadow constructors. Projection
0x00ee5ce0 backs its light-space origin by the slice's enclosing sphere
radius plus this field, using a finite depth extent. This is separate from
view distance +0x48, scripted view distance +0x190, and map quality.
Bevy 0.19.1 directional visibility ignores its light near plane
(`bevy_light/src/lib.rs`); Render extraction explicitly pushes that plane
to infinity (`bevy_pbr/render/light.rs`). A finite native caster extent
therefore merits investigation, but its constructor is not an active value.

Head #7035 approved **one read-only original probe** under normal live/game
claim, with a fresh save backup first. Existing private `live/mem.py`
opens only PROCESS_VM_READ | PROCESS_QUERY_INFORMATION. Actual probe
exited 0; PID 23408 image provenance and known object 0x1099fd40 vtable
0x0160df98 were checked before/after. Game lease released immediately;
original stayed running. No input, process writes/injection, heap scan,
install changes or Ghidra restart occurred.

Observed object +0x60 is **21000 cm**, **not constructor 4000 cm**;
+0x48 is 10 cm, +0x190 is 6000 cm, draw enabled, nonzero depth/color maps,
512 map size, three cascades, distribution 0.25. Current SceNo reads 3400.
The object is valid and draw-enabled, but current-scene selection and the
10 cm effective-distance value remain unresolved. Thus a 40 m cap is
**disproven for the observed object** and is not implemented. Raw probe,
source script and save backup stay private; head #7050 was informed.
Next caster work requires active-scene ownership/override evidence before
any opt-in geometry cull. Palette compaction stays the first optimization.

Further read-only installed-data research resolves the constructor override:
`scr/st100/sdl/st100_sky` authors `mBackforwardViewDistance` **30000 cm**;
`scr/st200/sdl/st200_sky` authors **21000 cm**, matching the observed object.
Both author view distance 6000 cm and distribution 0.25. A header-only
inventory of all **8,536 archives** finds exactly one rScheduler copy of
each path, in its corresponding stage archive, so these are the same
resources selected by ddvfs's sorted first-copy rule. SDL magic/version,
ARC version and inflated size were verified; scalar survey and resource
hashes remain private. Future implementation must read these properties
from the player's stage scheduler, not embed this runtime data as constants.

The effective 10 cm distance is also plausible from source, though its
current cause is not proven by the one approved probe: 0x00c6c970 uses
WeatherManager +0x80, which reflection's string at 0x015658d4 names
`mDayNightBlendRate`, not sun elevation. Without the scripted override,
effective distance is the scripted distance multiplied by the clamped
day/night factor `(blend - 0.7) * -5.0000005`, with a 10 cm minimum.
The active blend/override and current-scene object ownership were not read;
no second game probe or shadow implementation follows from this source.

### Compact palettes accepted headlessly; matched FPS capture queued

Source `39acf70` (documentation `3adc1fe`) is frozen. After codex-73's
actual M5 release #7154, a normal lease and empty global Cargo/rustc
preflight preceded the one-heavy build window. All suites/builds actually
exited before M5 release #7189, with no owned Cargo/rustc remaining.

- Full default session 11105 **actual exit 0: 88 passed / 2 existing
  ignored**, fresh compile 25.29 s, tests 11.20 s. Both new palette tests
  explicitly ran and passed: animated matrix/actual Bevy AABB equality
  plus transactional invalid/full-palette fallback.
- Full profile/default-release sequence 8968 **actual exit 0**: profile
  **93 passed / 2 existing ignored**, fresh compile 24.55 s, tests 11.60 s;
  all five real cached-span/name/window regressions and both new palette
  tests passed. Default release **40.14 s / exit 0**.
- Trace release session 27994 **actual exit 0 / 52.80 s**, approved by
  parent during the same lease for eventual named CPU diagnostics. No
  trace GUI was launched. Only existing private-interface/audio warnings
  remain; no clean/new target was needed.
- Default pin `E:/ddda-re/notes/steps/s41/ddview-palettes-39acf70.exe`, SHA-256
  `D0D0E0C2B8FC6C24F954F06C2C012C385BBA469C7B70D93DFBCED8D13E938D8C`.
  Trace pin `ddview-trace-palettes-39acf70.exe`, SHA-256
  `0BB1174054F273D5A822577A93718C4299A50F604A8CC41914EA4EA392FBE32E`.
  Both copied/checksummed privately before actual release. E: free space
  34.35 GB; old accepted trace pin is removed only after this replacement's
  actual exit/checksum and committed results, retaining prior summaries.
- Harness SHA-256 (metadata flags plus retained-handle watchdog):
  `6866DCF7037EB6A8F635DE9FED68F97FB20BEF2516A4783D61780F281DD0975E`.
  Parse check and all seven actual-AST race tests pass.

**Actual game palette/upload reduction, PNG fidelity and FPS gain remain
unverified.** Head #7157 queues its current-main/rescue validation, then
root's S42 settings visual captures, then our matched palette A/B. That
pair uses the same default pin, frozen v1 FieldSettle, x1, GPU builder,
unchanged camera/content and `DDVIEW_SKIN_DIAGNOSTICS=1` on both sides;
only `DDVIEW_COMPACT_SKINS` changes. Native simulation remains 30 Hz.
No new default or 180 fps claim follows from headless tests alone.

### Matched x1 compact-palette capture: upload reduction verified

Head #7204 moved the pair ahead of root's visual queue; codex-76's bounded
CPU diagnostic actually exited 0 and released core #7228 before the normal
live/ddview claim. An initial OFF capture was **rejected**: clearing null
flags through .NET SetEnvironmentVariable left empty environment variables,
while character.rs's diagnostic switches test var_os presence. Its PNG
showed RGB group colours and material counts differed. No ON was launched
from that setup. A head test/build then appeared; the launch guard refused
ON until head #7249 confirmed actual completion. These rejected timings
are not evidence for normal renderer performance.

Fresh `field-palettes-{off,on}-39acf70-r2` captures explicitly removed and
asserted absence of every null environment flag. Same default pin/hash and
frozen v1 FieldSettle/hash above, x1, GPU builder, automatic backend,
1920x1080, unchanged camera, no trace, diagnostics=1 both; only compact
skins=0/1 differed. Each used app Seconds=50, wall Watchdog=90 and screenshot
delay=45. No competing Cargo/rustc/CPU diagnostic was observed at either
preflight; head/root agreed to keep both corrected windows quiet.

- OFF PID **24492 actually exited 0 / 60.9390012 s**; ON PID **31816
  actually exited 0 / 62.2599285 s**. Both private viewers absent afterward,
  original PID 23408 running at its verified install path, normal GPU lease
  actually released #7269. Original game received no input or writes.
- Actual final SkinUniforms: **671 palettes both**, staging extent
  **278,276 → 32,078 matrices**, steady upload **17,809,664 → 2,052,992
  bytes**, an **88.47% reduction**. This is the renderer's actual retained
  staging extent, including high-water allocation, not the sum of live
  palette sizes. Current/previous buffer sizes each changed
  24,213,812 → 2,125,764 bytes. Upload reduction is verified.
- Every warm report has identical **4,836 entities / 1,802 mesh entities /
  1,071 visible meshes / 951 unique meshes / 423 visible DD materials /
  1,979 allocated DD materials**. Fixed 30 Hz simulation is unchanged.
- Reports ending approximately 25/30/35/40 s: OFF median frames
  **14.4 / 13.9 / 14.5 / 14.0 ms**; ON **10.9 / 11.1 / 11.0 / 11.1 ms**.
  Median of those four reports **14.2 → 11.05 ms**, 22.2% lower observed
  wall time (about 70.4 → 90.5 reciprocal fps). OFF p99
  16.9/14.9/17.9/17.1 ms; ON 13.9/14.3/12.7/13.3 ms. Frames over 16.67 ms
  total 22 → 6; over 33/50 ms zero in both warm windows.
- **Clock confound remains:** read-only warm GPU samples show P3 both,
  OFF graphics 885–915 MHz versus ON 1170–1200 MHz; memory 5001 MHz both.
  Original game's sampled 3D share is zero in every warm sample. GPU
  clocks were not changed. Thus faster paired frame time is observed,
  but an isolated causal FPS gain is **not proven** by this one pair.
- Both actual PNGs were viewed: intact terrain, two normally textured
  dressed characters, matching HUD/scene; no apparent skin/bounds or
  colour regression. These are bounded field fidelity checks, not
  exhaustive motion/content coverage. Parent independent review requested
  with exact private paths. Compact palettes remain **opt-in/default off**.

Private run/exit/GPU/stderr/PNG and bounded analysis summaries remain under
`E:/ddda-re/notes/steps/s41/field-palettes-{off,on}-39acf70-r2.*`.
The private launcher records verified absent negative flags rather than
empty values. Source/render harness freeze is unchanged. Next: review
the pair independently, then reverse-order matched replication or a
named CPU diagnostic under new approval/normal leases to separate upload
cost from the observed GPU clock change. Root's queued visual captures
take the released GPU first; no additional GUI was launched.

### Head-approved default promotion prepared; validation pending

Parent #7282 independently viewed both corrected PNGs and accepted the
upload/fidelity checkpoint. Head #7280 approved compact palettes as the
normal default, retaining explicit `DDVIEW_COMPACT_SKINS=0` for diagnosis;
this decision does not remove the documented clock confound. The prepared
change selects compaction for an absent or empty flag, and full palettes
only for explicit zero. Per-part SKIN_PALETTE startup logs require
`DDVIEW_SKIN_DIAGNOSTICS=1`, avoiding thousands of diagnostic log records
on the ordinary default path. Accepted paired captures already used that
flag on both sides, so their diagnostic behavior is unchanged.

A new regression exercises actual compact entity/bind/index selection
for absent/empty/1/0 settings without process-global environment mutation.
Existing animated matrix/posed AABB and invalid-input transactional
regressions remain. **This default-promotion source is not yet compiled or
accepted by full suites/release.** Those checks follow root's quiet visual
queue and coordination of reviewed-main integration. Head #7286 separately
approves one reverse-order ON→OFF replication under the same original
39acf70 pin after root's actual GPU release. No new quality/clock setting,
renderer ABI experiment or game probe is part of that pair.

### Reverse-order x1 replication complete; default validation underway

After root's actual MEDIUM/HIGH exits and GPU release #7355, a normal
live/ddview claim preceded the head #7286 approved reverse pair. Same
39acf70 default executable, v1 FieldSettle, hardened harness, x1, camera,
geometry/material counts and explicit absent diagnostic flags as r2;
only ON→OFF order and fresh r3 output names changed. All four actual
field PNGs (r2/r3) were viewed by this agent and independently by parent
#7282/#7384: matching terrain/two clothed textured characters/HUD, no
apparent deformation, missing parts or culling loss. No original-game
visual equivalence or exhaustive motion coverage is claimed.

- ON PID **30992 actual exit 0 / 61.8992287 s**; OFF PID **22656 actual
  exit 0 / 61.1123812 s**. Both owned viewers absent, original 23408 at
  its verified install path running. Normal GPU lease released #7377.
- Actual steady upload again **2,052,992 ON versus 17,809,664 OFF bytes**,
  32,078 versus 278,276 staging matrices, **671 palettes both**. All warm
  scene counts again match r2 exactly. All-spawn diagnostic logs record
  3,457 compacted / 26 unchanged parts, with median retained 15 joints;
  this spawn statistic is not a live-visible/upload sum.
- Warm median reports ON **11.1 / 11.0 / 11.1 / 11.1 ms**, OFF
  **15.4 / 15.7 / 14.3 / 14.1 ms**. Median reports **11.1 versus 14.85 ms**,
  25.25% lower observed wall time for ON; both orders reproduce improvement.
  ON p99 12.9/12.4/13.3/13.3 ms; OFF 21.6/18.4/16.9/15.7 ms.
  Frames over 16.67 ms total 0 versus 106; over 33/50 ms zero both.
- **Frequency still covaries:** ON graphics 1230–1260 MHz, OFF 825–900 MHz,
  P3/memory 5001 MHz both; original 3D share zero all warm samples. No
  power/clock settings changed. Reverse order rules out a simple second-run
  warm-up explanation, but does not isolate a frequency-independent gain
  or assign the full difference to prepare_skins. Upload reduction remains
  directly measured; head accepted default promotion on this checkpoint.

Default source `a7b9d90` was then dependency-integrated with reviewed main
7ea18c9 as **ae70239**, automatic merge with no conflict/manual resolution,
under head #7309's explicit dependency-integration exception. Parent's
reviewed ASCII settings labels 66c210d were cherry-picked as **4f8f726**.
No main/other worktree/WIP changes occurred. Branch is clean, source hooks
and new default-selection regression present. Head #7352 approves fresh
full default/profile suites plus default/trace releases in one normal M5
window, then private pins/checksums before actual release. **That fresh
integrated acceptance is underway; no Ready claim yet.**

### Ready: compact defaults on current main; full acceptance complete

Normal M5 claim #7381 followed empty global Cargo/rustc preflight and
parent/head #7352/#7355 queue approval. **All checks actually exited 0**
before M5 release #7400; both private executables were copied/checksummed
before release and no Cargo/rustc remained. Code source is **4f8f726**
(default promotion a7b9d90, reviewed-main merge ae70239, ASCII dependency
4f8f726); this final documentation update does not change compiled source.

- Default suite session **39633: 109 passed / 0 failed / 2 existing ignored**,
  fresh compile **31.66 s**, tests **13.03 s**. All three palette regressions
  explicitly pass: default/empty/opt-out actual index/bind/joint selection,
  exact animated skinning/Bevy posed bounds, invalid/full-palette fallback.
- Profile suite session **82521: 114 passed / 0 failed / 2 existing ignored**,
  fresh compile **25.17 s**, tests **13.58 s**. The same palette tests, all
  five cached-span/name/window regressions, and all three mesh-builder
  resource/order regressions explicitly pass. GPU remains the default;
  rejected legacy CPU captures do not become valid from these tests.
- Default release session **96356: actual exit 0 / 39.84 s**.
  Private `E:/ddda-re/notes/steps/s41/ddview-palettes-default-4f8f726.exe`,
  SHA-256 **7978950F4F315D3C6DB333E2FA209525A97A7DAFD144EF45ACF97CBA30CD645B**.
- Trace release session **88897: actual exit 0 / 41.35 s**.
  Private `ddview-trace-palettes-default-4f8f726.exe`, SHA-256
  **1A7288FA365464BCDC9010EC5B58BF920D9D9293D79DFC3885FF5F07717C7611**.
  No trace GUI has run from this pin. Previous accepted 39acf70 trace pin
  is removed only after this replacement's actual exit/hash and this
  committed summary, under head #7352. Previous capture summaries remain.
- Only existing Args/private-interface and audio forget_actor warnings.
  No clean/new target, quality/clock changes or additional original-game
  probe. E: free space about **33.9 GB** after compilation.

**Ready for parent review/head merge** on `step/s41-frame-rate`: exact
compact palettes now default, explicit zero opts out, startup palette
diagnostics require their own flag. Field upload/fidelity was validated
against the same frozen executable in both orders; promotion/current-main
integration has the complete fresh suites and release checks above.
Observed wall-time benefit reproduces, but GPU clock covariance remains
documented and no frequency-independent or 180 fps claim is made.

Next head #7280/#7352 action: one bounded named CPU trace on the accepted
compact-default source to choose between shadow bin-buffer writing and
preprocessing bind-group preparation. No additional optimization is
implemented. Source inspection confirms a finite native caster extent
would need both CPU directional visibility (Bevy ignores near plane) and
GPU extracted shadow frusta (prepare_lights pushes it to infinity); a
simple cascade far-range change is insufficient. Native per-stage
back-forward properties must be loaded from stage SDL, not copied as bulk
constants. That follow-up needs normal source/GPU leases and queue timing;
root S30/core and the queued children received this actual build handoff.

### Root handoff: completed compact-default named trace

Under head #7449/#7538, root ran the frozen `4f8f726` trace executable
above with the compact flag absent, installed field checkpoint in ECON-v2
format, x1 time, GPU mesh builder, unchanged default backend and 1920x1080.
The checkpoint SHA-256 was
`3690FEEFF71565855422B65BC87CF0D10B50609B4E89565C4ABC7E36B27E4208`.
There was no demo, screenshot exit or diagnostic colour override. The
normal GPU claim preceded launch; no Cargo/rustc/CPU diagnostic overlapped.

Viewer PID34172 and harness session80963 actually exited **0**, after
28.1841818 wall seconds. The stage100 trace window ran from
11:47:47.949319 to 11:47:50.961720 UTC, **3.012401 wall seconds**, and its
own completed-window condition requested normal application exit. The
owned viewer was absent and original DDDA PID23408 remained at its verified
install path before normal lease release #7561. No watchdog truncation
or original-game control occurred.

Actual Chrome instant markers, rather than the separate perf elapsed
origin, bound the analysis: start **18.2024448 s**, end **21.214852 s**.
Streaming analysis trims 100 ms from each boundary and selects the actual
main thread `(pid1, tid0)`. It contains **209 complete main update spans**.
Strict parsing passes with no unmatched, truncated or terminal open scopes;
`--allow-open-scopes` was not used. The named systems migrate among compute
workers, so the following groups combine matching system names across
threads, with **210 complete runs each**:

| Named CPU system | Total inclusive/exclusive CPU ms | CPU ms per complete main update |
| --- | ---: | ---: |
| Shadow `write_binned_instance_buffers` | 439.9097 | 2.104831 |
| `prepare_preprocess_bind_groups` | 320.4129 | 1.533076 |
| `prepare_skins` | 86.8364 | 0.415485 |

Inclusive and exclusive agree for these three system scopes. Across
threads and enclosing spans, CPU times overlap; the rows are a ranking,
not additive frame time, critical-path savings, GPU timing or an FPS gain.
The traced main-update median12.898 ms/p95 15.9137 ms is diagnostic only.
Compact palettes remain the accepted default; this capture adds no code.

The 320,134,513-byte raw trace, launch/exit/provenance logs, marker events,
strict streaming summary and grouped ranking remain private under
`E:/ddda-re/notes/steps/s41/field-compact-default-trace-4f8f726.*`.
No game records, screenshots or trace data enter the repository.

Next proposed target is finite native shadow caster extent, because shadow
bin-buffer writing is the largest of these named CPU costs. Any adapter
must handle both CPU directional visibility and GPU extracted shadow
frusta, and obtain native stage properties at runtime. Head must assign
that source scope before implementation; no culling or quality change is
claimed here.

### Prepared opt-in finite caster adapter (not yet compiled)

Head #7581 approved the next source scope. Source **a60a6c9**, on the owned
branch after reviewed-main integration **faceaee**, adds a small viewer
adapter without modifying Bevy, its registry sources or shadow-map quality.
It is enabled only by `DDVIEW_FINITE_SHADOW_EXTENT=1`; absence/empty/zero
retain the accepted renderer. No default promotion or performance gain is
claimed before the agreed full suites, release and matched x1 A/B.

`SkyShadow` now reads the installed scheduler's
`mBackforwardViewDistance` as an optional finite nonnegative value, with
no constructor/bulk-value fallback. The native depth boundary from
`0x00EE5CE0` uses the camera-view slice's AABB centre and enclosing radius,
then extends toward the light by that radius plus the authored distance.
The viewer adapts this near boundary to the existing Bevy cascades; native
XY projection fitting/texel snapping and depth projection remain separate.
Using a world-space AABB instead would wrongly change the sphere radius
as the camera turns.

CPU cascade entity lists are filtered after Bevy's light visibility pass,
with the finite near plane enabled in OBB intersection. Explicit
`NoFrustumCulling` and missing bounds remain conservative. The same planes,
keyed by light, camera and cascade, are extracted into the render world.
Directional shadow frusta are repaired after `prepare_lights`' deferred
view insertion and before view-uniform preparation, restoring the finite
GPU near halfspace for `NoCpuCulling` meshes too. Point/spot lights and
unmapped views are unchanged. Missing stage data clears the extracted
plane map rather than retaining stale data.

Four meaningful regressions are prepared, **unexecuted**: camera/light
rotation and native sphere boundary; remote versus crossing caster OBB;
render-world repair for two cameras, unmapped cascades and point lights;
and actual CPU entity-list filtering preserving opt-outs/missing bounds,
plus clearing planes when the authored setting disappears. The existing
installed sky regression also checks valid authored caster distances for
both opening stages. No Cargo/GPU work overlapped head's combined-main
checks; the root source is frozen pending its queue window after the
children's first headless checks. Next: compile and run the full viewer
suite with all four names present, then build/pin one executable for the
opt-in OFF/ON identical-picture x1 comparison.

Source review then caught a stage ownership issue before compilation:
the existing initial `SkyShadowSetting` reads opening stage200 once. A
field checkpoint or stage100 handoff must not inherit that stage's caster
distance. Follow-up **b9c8233** reads the current `Streamer.stage` after
opening/stream updates and caches `(stage, weather)`, without altering
the accepted sky projection. Missing/invalid new-stage data clears the
caster setting. A fifth prepared regression verifies stage/weather
handoff, one installed load per key and no stale prior-stage fallback.
This follow-up also remains **uncompiled/unexecuted**; candidate pin/plan
metadata must use the final actually tested source, not the earlier a60
checkpoint. The prepared private OFF/ON helper has an explicit `UNBUILT`
guard until full acceptance and checksum/pin gates are complete.

Head's uninterrupted-route regeneration found a separate RoadWalk freeze
after ReynardRepeat, despite S50's passing resumed checkpoint. Root's
viewer build/capture window now follows head's diagnostic build and short
resume run, not merely the children's actual core exits. The accepted
named trace above remains valid; no candidate capture overlapped that
diagnosis and no full uninterrupted-route claim follows from S50.
