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
