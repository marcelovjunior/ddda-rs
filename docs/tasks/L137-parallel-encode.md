# L137: ordered parallel encoding

Assigned by head to codex-11, 2026-10-13. Base `335318fc`, branch
`task/L137-parallel-encode`, worktree `E:/ddda-rs-wt/L137`.

## Scope and implementation

Measure the ddgpu finisher, then opt in to 2–4 persistent encoding workers.
No game execution, lifted shard builds, game-data changes, or Ghidra changes.

`DDGPU_ENCODE_THREADS=1` is the default. Values 2–4 enable the pool; missing,
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

Initial checkpoint: 20 ddgpu unit tests pass, including four new pool/profile
regressions. The GPU multi-pass oracle passes with 1/2/4 workers (1.18 s,
Vulkan): exact readback, upload/copy dependency order, query results and
submission counts. Targeted draw/cache/targets compilation passes; broad
`cargo test -p ddgpu --no-run` hits the inherited `module_cost` example's
missing `crate::pipeline_log` import. A whole draw-test process exceeded its
60 s limit and was killed; tests are now isolated per case with the same
60 s hard limit. Full GPU regressions, runtime light suite and optimized
synthetic timings are pending.
No game/FPS or original-game image comparison by this agent. Next: complete
the byte-exact multi-pass oracle and report the measured synthetic split;
head then performs same-scene image/FPS A/B before any default change.
