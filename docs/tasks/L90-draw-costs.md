# L90: per-draw D3D9 costs

Owner: codex-7. Branch: `task/L90-draw-costs`. Base:
`task/fps-line` at `c349d834` (L89/L83/L94/L97/L102 integrated;
previously rebased on L92/L93 at `8c01fe5b` per head).
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


## Final integration validation

- Rebased onto head's requested FPS base **eeaaa7a6**. Core commits are
  compact **83efa036**, groups **eb3ed5c6**; dependencies are L89 fixture
  compatibility **2644b7d3** and L96 headless PRIMARY **3bc8ed49**.
  The initial Arc/identity commit **ef1370cd** was handed to head separately.
  Original pre-rebase compact/group commits: **8999b372**, **a91da3d1**.
  Newer FPS-branch changes after eeaaa7a6 are not included in these numbers.
- Integrated L97 removes the final production snapshot allocation:
  **2 allocations/draw**, both in the test COM helper. Set
  `DDRUNTIME_SYNC_BACKEND=1` to isolate snapshot cost from L89 queue work.
  L98's batched `flush_reads` remains intact in the extracted snapshot
  module; both dirty-flush regressions pass with dirty-flush auditing.
- **7,850** distinct installed shaders match Microsoft's disassembly;
  **18,036** full/compact layout and input-interface variants from Title
  and root validate in Naga, zero failures. Existing checks are retained.
- Vulkan driver gate: **35 active named tests pass**, executed as 38
  invocations because all 200 target formats are split into four shards;
  three existing exploratory tests remain ignored. The 150 random-state
  cases, 180 depth cases, frame/readback, MSAA, presentation and cache
  comparisons pass. Every case has a 60-second process-tree timeout;
  head authorized 300 seconds for cache, which finished in 18.27 seconds.
- The compact-layout test now renders **24,576** independently visible
  pixels and asserts an actual ring restart after L105 enlarged the ring
  to 48 MiB; exact driver RGBA comparison passes in 2.07 seconds. L102's
  nonblocking query contract required one fixture adjustment: request the
  result, fence pending work, then retain the exact count-of-16 assertion.
  This fixture passes; no worker or queue production code was changed.
- Three alternating matched debug `draw_cost_game` pairs (saved Arc-only
  and compact-only binaries on the same pre-integration base, 1,000 draws
  x 10 frames): submitted medians **239.18 -> 50.98 us/draw**; GPU-waited
  frame medians **243.92 -> 56.50 ms**. Per-draw ranges were 224.68-302.39
  and 47.84-52.02 us. Across 11,000 draws including warmup, render passes
  fell from 23 to 15. Shared-machine microbenchmarks are not gameplay FPS.
- Earlier integrated full light runtime validation passed 154 tests with
  one existing ignored test. On the latest base, final validation is
  recorded below; the independent fullscreen-gamma Reset issue recurred.
- Private logs/binaries are under C:/ddda-targets/L90-*. No game was run,
  no lifted shard was built, and no game/research data is committed.
- Final latest-base light runtime suite: **155 passed, 1 failed,
  1 existing ignored**, with serial tests, synchronous synthetic backend and
  `DDRUNTIME_VERIFY_DIRTY_FLUSH=1`. The only failure is fullscreen-gamma
  Reset (D3DERR_INVALIDCALL), previously reproduced on untouched code by
  L98. All four draw-path tests and both 42-resource/alias dirty-flush
  regressions pass. Earlier 5 queue, 5 frame and 4 backend CPU tests pass.
