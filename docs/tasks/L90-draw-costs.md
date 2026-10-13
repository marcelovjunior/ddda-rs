# L90: per-draw D3D9 costs

Owner: codex-7. Branch: `task/L90-draw-costs`. Base:
`task/L92-stage-autoplay` at `8c01fe5b` (rebased per head).
User-assigned brief, 2026-10-13.

## Scope

Reduce shader constant copies/uploads to the registers read by the bound
shader, reuse unchanged snapshot groups, and remove per-draw allocations.
Preserve draw ordering and binding validity. L89/codex-6 owns backend worker
and queue plumbing; shared payload ownership is coordinated with its queue.
No game runs, game data changes, or lifted shard builds.

## Validation

Use `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`, and
`CARGO_TARGET_DIR=C:/ddda-targets/L90`. Runtime light suite and D3D9 tests;
ddgpu driver-equivalence tests only while pause status permits, with a
60-second hard timeout per test (head permits 300 seconds for cache.rs).
Serialize GPU suites through HEAVY_BIG=1/heavy.py. Measure `draw_path` and
`draw_cost_game` before/after. Only head measures gameplay.

## Results: first isolated batch (Arc payloads)

- Shader/declaration objects allocate immutable shared payloads once.
  Draw snapshots and L89 queue clones retain handles. Constant updates
  compare bits before detaching shared blocks; float/int/bool updates and
  object release preserve earlier commands.
- Backend shader keys use a retained allocation registry. Identical
  independently created programs are deduplicated once; existing draws
  do not hash bytecode. Constant reuse checks identity before contents.
  No WGSL layout, arithmetic, draw order or uploaded extent change yet.
- **150 runtime light tests passed**, including the new lifetime test;
  **5 frame serialization tests**, **1 identity test** passed. All ddgpu
  tests/examples compile. Completed GPU comparisons passed (including both
  full 150-case randomized driver tests, instancing, MSAA, frame replay,
  presentation, MRT, render-to-texture and render-pass reuse).
  Long-lived cache, depth precision and target-format cases exceeded their
  60-second limits and remain unverified; no checks or case counts changed.
- Matched L92/L93 debug runtime benchmark, three alternating runs:

  | Metric | Before | Arc |
  |---|---:|---:|
  | Indexed draw, median us | 49.35 | 45.59 |
  | Indexed draw, range us | 41.18–54.34 | 40.41–52.45 |
  | Allocations/draw, including test COM arguments | 15 | 10 |

  Timing ranges overlap under shared-machine load. The earlier L88-only
  baseline was 29.94–33.32 us and is not a matched timing comparison after
  adopting L93's lock changes. No gameplay/FPS improvement claimed.
- GPU debug `draw_cost_game 1000 10`: **442.91 -> 256.04 us/draw** submitted,
  **449.32 -> 261.47 ms/frame**; both 11,000 draws / 23 passes including
  warmup. Same backend source baseline across the L92 rebase; shared-machine
  load limits timing attribution. Logs are private under `C:/ddda-targets/L90-*`.
- Next: head integrates/measures the Arc-only commit; compact constants
  and dirty snapshot groups remain separate follow-up commits.

## Results: compact constants

- Shader creation records the bytecode's highest float register read,
  including matrix rows. Relative reads retain the full stage register
  file. Device banks remain complete for later shader rebinding/Get*;
  immutable draw prefixes reuse unchanged data, including writes outside
  the bound shader's extent.
- Backend WGSL float arrays use that extent (one zero slot when unused),
  with the integer/bool/extra fields packed at the matching offsets.
  Float bits are copied as one byte slice on little-endian hosts. Dynamic
  offsets keep 256-byte alignment; maximum binding ranges remain valid,
  and both stages are preflighted before a ring restart.
- The VS150/PS32 fixture float payload is **7,680 -> 2,912 bytes** (62.1%
  smaller). Paired debug CPU medians: Arc **28.82** vs compact **28.76
  us/draw** (ranges 28.50–29.24 and 28.13–30.21); both 10 allocations/draw.
  This unchanged-draw fixture does not demonstrate a CPU timing gain.
- Runtime light suite: **151 passed, 1 failed, 1 existing ignored**. The
  failure is fullscreen-gamma Reset at d3d9_device.rs:135, independently
  reproduced on untouched runtime code by L98 (#10482 on the board).
  New compact prefix/rebinding regressions pass. Frame serialization 5,
  shader extent 1, backend identity/byte packing 2 unit tests pass.
- The new GPU driver regression **passed with exact RGBA equality**:
  4,096 independently visible draws alternate c0/c223 across uniform-ring
  rollover and check alpha-test tail offsets. The full 150-case texture/
  state comparison and completed draw cases also passed. The serialized
  gate was yielded for L96's independent WGL teardown fix; remaining
  cases and paired GPU timings are still pending. No checks were weakened.


## Results: dirty snapshot groups and allocation reuse

- Device-owned immutable groups retain texture/sampler descriptors, streams,
  shader/declaration payloads, index format and target metadata. Setters
  dirty only their groups; UP draws explicitly dirty/reset stream/index
  bindings. Redundant texture/sampler/stream setters retain shared lists.
  DrawCall texture and stream lists are Arc slices, so queued/recorded
  clones keep prior draws without copying lists.
- Resource shadows still flush on every draw. The bounded 42-entry read
  array replaces the transient Vec; L98 independently owns selecting
  dirty resources. Snapshot code now lives in d3d9snapshot.rs. L97 owns
  render-state sharing: its merge changes that module's render-state
  expression to s.render.shared(). Neither worker nor queue code changed.
- Indexed draw arguments use fixed arrays; constant setters reuse their
  word buffer. **Allocations/draw: 10 -> 3**, including the COM helper's
  allocations; this branch still has the render-state Vec copied per draw.
- Three paired debug CPU runs, Arc vs groups: median **39.19 -> 23.05 us**,
  ranges **35.05-51.31 -> 16.31-26.57 us**. Shared-machine load varies;
  allocation counts are stable, and no gameplay gain is attributed here.
- **152 runtime light tests + 5 frame tests pass**, with the same one
  baseline fullscreen-gamma Reset failure and one existing ignored test.
  New regression checks old snapshots, pointer sharing, sampler changes,
  stream offsets/frequencies, target/index changes, nonindexed draws and
  the UP stream reset. All ddgpu tests and examples compile. GPU gate and
  paired backend timings remain pending the head's next run-done release.
