# L89: deferred D3D9 backend

Assigned to codex-6 by the maintainer; branch `task/L89-backend-worker`,
base `task/L88-quick-render` (`3e08804d`). See AGENT-RULES.md.

Implement one bounded FIFO and one host worker owning the graphics backend.
Uploads and snapshots must own all bytes. Reads and query results execute
after preceding work and return synchronously. Present returns after recording,
with at most two outstanding Presents. Preserve the window sink's per-frame
presentation context. `DDRUNTIME_SYNC_BACKEND=1` selects the synchronous path.

Scope: d3d9cmd queue module/API, ddruntime backend installation/presentation,
focused queue and GPU equivalence tests, draw_cost_game measurement, docs.
No game runs, shard builds, guest behavior changes, or game data commits.

Validation: light runtime suite and D3D9 tests, queue ordering/ownership/lifecycle
tests, ddgpu driver equivalence and queued-versus-synchronous pixels, producer
draw timings. GPU runs require pause.py to report not paused, each capped at 60 s.

## Results

Implemented and tested. Core **e7a069f3**, shared-handle fixture compatibility
**ffc2b759**, standalone target-format sharding **0a1776ab**. No game run,
lifted-shard build, or push by this lane.

- One bounded 2048-command FIFO, owned closures/snapshots, one backend consumer,
  synchronous result round trips, shutdown drain/join, worker failure propagation.
  Two outstanding Presents block the first command of a third frame, not merely
  its Present. The frame's copied presentation context reaches the worker sink.
- `set_backend` enables the worker by default; `DDRUNTIME_SYNC_BACKEND=1`
  preserves immediate execution. Capture is inside the worker. Readbacks and
  occlusion results are FIFO round trips; host-side query answers and target
  LockRect fence first. Default headless Recorder behavior is unchanged.
- CPU verification: `cargo test -p ddruntime --no-default-features`: **134 pass**,
  including the worker presentation-context regression. Final library/device
  rerun: **14 + 5 pass**. `d3d9cmd`: **5 pass** (ownership with a blocked consumer,
  FIFO/results/reuse, queue backpressure, frame pacing, worker failure).
  `d3d9frame`: **5 pass**. Jobs=4, incremental=0, target `C:/ddda-targets/L89`.
- GPU: **34 pass, 3 existing ignored**, including exact queued-versus-sync
  two-frame pixels, buffer replacement/reuse, copies/fills, occlusion counts,
  and execution of present_sink on one worker. Driver comparisons for randomized
  draws, render states, depth, MRT and render-to-texture pass. Debug executable
  runs hit the aggregate 60-second limit; unfinished cases ran individually in
  optimized `play`, each still capped at 60 seconds.
- The default 200-case target-format matrix exceeded 60 seconds amid concurrent
  GPU suites. Test-only commit **0a1776ab** adds `TARGETS_SHARD=i/n` after all RNG
  input generation: default unchanged, four disjoint shards preserve all 200
  cases and assertions. All four shards pass: **200/200 compared, zero
  differences, zero refused formats**, 12.91–13.20 seconds per process.
- `draw_cost_game` now reports immediate and queued producer costs and total
  GPU-waited frame time on the same warmed backend. Final `play` measurements
  ran through `HEAVY_BIG=1` after the head's hold, with no overlapping GPU suite,
  jobs=4 for the earlier build and no lifted crates. Each process was capped at
  60 seconds; all six benchmark processes completed in 2.54–2.75 seconds.

| Draws/frame | Frames/run | Sync producer us/draw | Queued producer us/draw | Sync total ms/frame | Queued total ms/frame |
|---|---|---|---|---|---|
| 1700 | 30 | 14.35 | 4.41 | 28.72 | 29.77 |
| 5200 | 10 | 14.38 | 9.72 | 79.18 | 85.98 |

These are medians of three paired runs per load, on the L88 base plus L89
(without L90's shared payloads). Producer time falls **69.3% / 32.4%**; total
time rises **3.7% / 8.6%**. The larger burst hits the 2048-command bound.
Snapshots are prebuilt, and each frame ends with read_back: this measures
offloading and completion cost, not game simulation overlap or FPS. Earlier
contended samples and an interrupted lease-acquisition trial are not used.

- The no-op runtime backend probe passes both modes: one debug sample was
  29.97 -> 40.38 us/draw. This isolates recording overhead, with no backend work
  to offload. L90's shared handles will remove most payload-copy allocations;
  this branch's production queue already accepts those types unchanged.
- Limits: command-count bound, not a byte bound; a single upload can be large.
  Existing target LockRect CPU-shadow behavior remains: this adds ordering, not
  its missing GPU readback. L89-only gameplay FPS and original-game visuals have
  not been compared. The worker takes no runtime/guest/device lock.
- Head's combined L83+L89+L90+L94+L97 run reports **7.2 vs 4.9 FPS** (#10493),
  not an isolated L89 A/B. It exposes 26% render-thread wait on the D3D lock
  because another thread calls synchronous occlusion GetData while holding it.
  The head owns the follow-up **L102 non-blocking query** work; this branch
  implements the original brief's synchronous-result contract. The subsequent
  L102+L103+L99 combo reports **8.9 FPS** (#10525); neither combined result
  isolates L89's contribution.
- Logs: `C:/ddda-targets/L89/{runtime-tests,queue-compatible,gpu-tests,
  gpu-remaining,benchmarks-serial}.log` and `gpu-isolated-target-formats-*.log`.
  Final job exited and released its heavy slot at 02:27:02, before the next
  game-run hold. Next action: head integrates L90's snapshot groups and L98's
  dirty-resource flushes into the renderer; no further L89 gate is pending.
