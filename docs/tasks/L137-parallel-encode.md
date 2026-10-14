# L137: ordered parallel encoding

Assigned by head to codex-11, 2026-10-13. Base `335318fc`, branch
`task/L137-parallel-encode`, worktree `E:/ddda-rs-wt/L137`.

## Scope and implementation

Measure the ddgpu finisher, then opt in to 2â€“4 persistent encoding workers.
No game execution, lifted shard builds, game-data changes, or Ghidra changes.

`DDGPU_ENCODE_THREADS=1` is the default. Values 2â€“4 enable the pool; missing,
invalid, zero and out-of-range values select 1. `set_encode_threads` is the
explicit per-backend equivalent and drains pending work before switching.

wgpu 29 already records an owned command stream. In wgpu-core 29.0.4,
`command/mod.rs`'s command-buffer encoding loop calls `render::encode_render_pass`
while finishing the encoder. The public pass begin/end/state/draw APIs record
that stream; timing those calls does **not** measure the deferred HAL operations.
L137 retains this recorder instead of introducing a second copy of every draw.

The backend cuts encoders at existing pass/copy boundaries, after at least
`ceil(draws_per_submit / workers)` draws/clears/copies in the chunk. It does
not split a live pass, remove a pass, change attachment load/store operations,
or relax the existing pipeline/group/buffer state suppression. A long single
pass can therefore remain serial. All chunks stay in original order; the
finisher collects independently finished buffers by index, then makes **one**
queue submission for that batch. Resource-use tracking covers all chunks.
Uniform/inline ring writes still precede the batch handoff, ring restart
still flushes, and mutable resource writes retain their existing fences.

`encode::Pool<C, R>` accepts an encoding callback `fn(C) -> Vec<R>` so L138
can later substitute owned Vulkan chunks expanding into barrier/raw buffers.
Callbacks must encode only; the coordinator owns queue writes/submission.
Worker panics propagate to the finisher fence instead of hanging a waiter.

## Measurement

`DDGPU_ENCODE_PROFILE=1` enables per-Present histogram reports; otherwise no
Instant calls or histogram locks are made. After flushing/fencing the frame,
the report resets totals, counts and power-of-two-nanosecond p50/p95 upper
bounds. Recording counters cover the ordinary draw path (including cache
checks/handle updates); helper clear/blit/native recording is not included.
Finish and submit counters include finisher batches and direct backend
submissions; the present sink owns its independent submissions.

- `record_begin`, `record_end`: public draw-pass recording and drop.
- `record_pipeline`, `record_groups`, `record_vertex`: the corresponding
  state blocks, including suppression of equal state.
- `record_draw_index`: draw and optional index-buffer state block.
- `record_dynamic`: viewport, scissor, stencil reference and blend constant.
- `finish_cpu`: sum of per-chunk encode/finish durations, across workers.
- `finish_wall`: batch finish elapsed time, including dispatch/collection;
  overlaps `finish_cpu`, so do not add the two.
- `submit`: queue submission elapsed time, excluding finishing.

Instrumentation perturbs small recording calls. CPU sum and elapsed time
can differ with multiple workers. Internal deferred begin/end/state/draw
timings cannot be isolated through wgpu's public API; patching/vendoring
wgpu-core is outside this runtime-only change. No internal-stage split is
claimed from the public recording counters.

The ignored `parallel_encode::encode_stage_benchmark` uses generated minimal
shaders and 950 alternating RGBA32F target runs, 864 one-draw runs and 86
larger runs, totaling 5,294 draws. Targets are 4x4, so this is a CPU encoding
fixture, not a simulation of the game's GPU workload. It includes normal
backend recording, ring upload, finish and submit, excludes initialization
and discards three warm-up frames before nine measured frames. Run with
`DDGPU_ENCODE_THREADS=1/2/4`, optionally `DDGPU_ENCODE_PROFILE=1`.

## Results

Implemented in `02cf25e9`. Default remains one worker. No new redundant
state commands were found beyond the existing L80 suppression; no empty
pass or attachment operation was removed without equivalence evidence.

### Verification

- **20 ddgpu CPU tests pass**, including four new tests for bounded opt-in,
  histogram bounds, ordered multi-result chunks/pool reuse and panic propagation.
- **New GPU oracle passes** with 1, 2 and 4 workers: three frames of 129
  dependent RGBA32F passes each, copies across chunk boundaries, per-draw
  constants, occlusion queries, in-place texture upload ordering, equal
  submission counts and pool replacement with pending work. Readbacks are
  byte-identical. Passed debug and optimized builds (profiling enabled in
  the latter).
- With **both 1 and 4 workers**, Vulkan draw **17 pass / 1 existing ignored**,
  cache **3 pass**, targets **7 pass / 1 existing ignored**. These include
  host D3D9 comparisons, ring rollover, mutable buffers, depth/stencil,
  render-to-texture, self-sampling, MRT and cube targets. Each case ran in
  an isolated process with a hard 60 s timeout after `pause.py status`
  reported `not paused`; no individual case timed out or failed.
- Runtime light suite, `--no-default-features --no-fail-fast`, serial:
  **200 pass / 1 inherited fullscreen-gamma Reset failure / 4 ignored**.
  The failing Reset returns HRESULT 2289436780. No checks were weakened.
- Focused ddgpu compilation passes. Broad `cargo test -p ddgpu --no-run`
  hits the inherited `module_cost` example's missing `crate::pipeline_log`
  import. The initial whole draw-suite process exceeded 60 s and was
  killed; all its nonignored cases subsequently passed in isolation.
- All cargo commands: `CARGO_INCREMENTAL=0`,
  `CARGO_TARGET_DIR=C:/ddda-targets/L137`, `CARGO_BUILD_JOBS=4`.
  No game, shards, Ghidra writes, game-data writes, or push.

### Synthetic timing

Optimized ddgpu/test target (`--config profile.dev.package.ddgpu.opt-level=3`),
workspace dependency optimization level 2, Vulkan. Own builds and regression
jobs had ended before measurement; the machine remains shared, so timings
have substantial scheduler/driver variability. Each row is the median
per-frame total over nine frames after three warm-ups. Recording timers
include their state checks and updates, and do not describe the deferred
HAL sub-stages inside finish.

| Stage (ms/frame, profiling enabled) | 1 worker | 2 workers | 4 workers |
|---|---:|---:|---:|
| Public begin recording | 0.307 | 0.273 | 0.280 |
| Public end/drop recording | 0.415 | 0.422 | 0.373 |
| Pipeline state block | 0.251 | 0.248 | 0.239 |
| Bind-group state blocks | 0.326 | 0.329 | 0.333 |
| Vertex-buffer state block | 0.385 | 0.393 | 0.398 |
| Draw / index-buffer block | 0.642 | 0.661 | 0.646 |
| Dynamic state block | 0.401 | 0.390 | 0.397 |
| Finish CPU sum | 13.075 | 18.103 | 18.028 |
| Finish wall (overlaps CPU sum) | 13.080 | 12.411 | 8.634 |
| Queue submit | 1.385 | 1.980 | 1.835 |
| Whole fixture frame | 17.326 | 17.260 | 12.912 |

The final frame reports 959 draw passes and 11 queue submissions in every
mode (950 target runs plus pre-existing draw-count pass splits), with
11 / 21 / 41 finished encoders for 1 / 2 / 4 workers. The public recording
cost barely changes; finish wall drops with four workers, at the cost of
more aggregate CPU and submission work. Producer and finisher overlap;
these medians must not be summed to reconstruct frame elapsed time.

With profiling **off**, alternating order 1,2,4,4,2,1 yielded these two
nine-frame median measurements per configuration:

| Workers | Frame medians (ms) | Individual frame range across both runs (ms) |
|---|---|---|
| 1 | 19.968, 15.670 | 12.429–34.446 |
| 2 | 14.795, 14.627 | 8.768–20.909 |
| 4 | 11.596, 12.095 | 7.451–17.480 |

This is evidence for a head A/B candidate, not a stable speedup guarantee
or gameplay/FPS result. Profiling overhead cannot be estimated reliably
from these noisy separate runs. The workload uses generated tiny shaders
and 4x4 targets, not game data or the real cloth simulation.

Private local logs (not committed): `C:/ddda-targets/L137-bench-*-p*-t*.log`,
`L137-bench-summary.log`, `L137-gpu-suites.log`, per-case GPU logs,
`L137-runtime-light-all.log`, `L137-oracle-opt.log`.

Next action: head compares same-scene images/FPS with
`DDGPU_ENCODE_THREADS=1` versus `4`; use `DDGPU_ENCODE_PROFILE=1` for a
separate attribution run. Keep default 1 until that comparison. L138 has
cherry-picked the generic pool checkpoint for its own Vulkan encoder.
