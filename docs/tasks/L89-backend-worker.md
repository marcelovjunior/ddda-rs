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

Implemented, with final target-format gate and isolated timings pending the
head's CPU-quiet window (#10403). No gameplay run or shard build performed.

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
- GPU: **33 pass, 3 existing ignored**, including exact queued-versus-sync
  two-frame pixels, buffer replacement/reuse, copies/fills, occlusion counts,
  and execution of present_sink on one worker. Driver comparisons for randomized
  draws, render states, depth, MRT and render-to-texture pass. Debug executable
  runs hit the aggregate 60-second limit; unfinished cases ran individually in
  optimized `play`, each still capped at 60 seconds.
- The default 200-case target-format matrix exceeded 60 seconds amid concurrent
  GPU suites. Test-only commit **0a1776ab** adds `TARGETS_SHARD=i/n` after all RNG
  input generation: default unchanged, four disjoint shards preserve all 200
  cases and assertions. It compiles; the four GPU runs remain pending.
- `draw_cost_game` now reports immediate and queued producer costs and total
  GPU-waited frame time on the same warmed backend. Preliminary, **contended**
  medians (three runs, not an FPS claim): 1700 draws, 18.22 -> 6.11 us/draw,
  36.05 -> 37.01 ms/frame; 5200 draws, 18.55 -> 14.43 us/draw,
  101.96 -> 125.40 ms/frame. The larger burst hits the command-count bound.
  A requested isolated repeat was interrupted by the head's game handoff and
  is discarded. Repeat under `HEAVY_BIG=1` after the quiet period.
- The no-op runtime backend probe passes both modes: one debug sample was
  29.97 -> 40.38 us/draw. This isolates recording overhead, with no backend work
  to offload. L90's shared handles will remove most payload-copy allocations;
  this branch's production queue already accepts those types unchanged.
- Limits: command-count bound, not a byte bound; a single upload can be large.
  Existing target LockRect CPU-shadow behavior remains: this adds ordering, not
  its missing GPU readback. Neither gameplay FPS nor original-game visuals have
  been compared. The worker takes no runtime/guest/device lock (L93 compatible).
- Logs: `C:/ddda-targets/L89/{runtime-tests,queue-final,gpu-tests,gpu-remaining,
  benchmarks}.log`. Next action: all four target-format shards and isolated
  paired timing under the shared heavy slot after `build B done`; then head-run
  gameplay A/B against `DDRUNTIME_SYNC_BACKEND=1`.
