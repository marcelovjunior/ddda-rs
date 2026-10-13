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
60-second hard timeout per executable. Measure `draw_path` and
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
