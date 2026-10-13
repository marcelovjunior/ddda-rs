# L151: parallel loading preparation

Owner: codex-6. Branch: `task/L151-load-parallel`, base `3b9f79fb`
(`task/fps-line`). Assigned in the maintainer prompt, 2026-10-13.
Scope: ddgpu recipe warmup, advisory shader-object notifications through
runtime/d3d9cmd/capture, associated tests and documentation. No game runs,
shard builds, changes to the install, or speculative game-logic ports.

## Evidence and critical path

`WgpuBackend::derive` calls `module` and `warmup::create_pipeline` on the
`d3d9-backend` consumer (unless the synchronous diagnostic backend is selected).
First-use misses stall that consumer and can fill its bounded producer queue.
L116 already starts serial speculative compilation from recorded recipes in
`ddplay::start_game`, after device/runtime initialization and before guest game
execution. L151 uses that earliest existing backend startup hook, not an
uninitialized device at literal process entry. Helpers, native Device and raw
Vulkan objects are separate paths and are not warmed by these recipes.

Known c21/c23 totals: 517 modules / 0.45 s module / 0.28 s translation;
2,331 pipelines / 2.66 s; archive reads 4,224 / 1.24 s; fast inflate 4.9 s.
L149's c23 intervals refine the ranking: resource loading 32.181–45.474 s
(13.292 s), inclusive zlib 6.660 s / fast 4.133 s / read services 0.634 s.
The following 1.661 s includes 1.207 s pipelines and 0.558 s modules.
These overlapping call totals are not additive critical-path wall time.
The matched c25 FAST=0/1 timeline is awaited from head; no measured whole-load
improvement or original-game image comparison is claimed here.

`DDRUNTIME_FILE_READ_AHEAD=1` already enables synchronous forward 256 KiB
windows for read-only install ARC handles, with an 8 MiB runtime budget.
It does not decode or asynchronously fetch the next archive entry. Current
interval evidence ranks inflate above reads; no additional I/O speculation
or parallel execution of shared original zlib state was added.

## Implementation and controls

- `DDGPU_PIPELINE_THREADS=N`: one creator by default, explicit 2–4 opt in;
  values above four cap at four, zero/invalid values retain one. Separate
  persistence coordinator services records while creators work. Saved recipes
  are queued immediately after the existing exe/driver/schema validation.
- One `OnceLock<Result<ShaderModule, String>>` per WGSL digest deduplicates
  background module creation, including failures. Only creators wait on these
  cells. Queue/map locks are released before GPU operations. All creators use
  the same existing L140 persistent pipeline cache and unchanged descriptor
  builder. wgpu 29's std error-scope stacks are thread-local (installed
  `wgpu-29.0.4/src/api/device.rs`).
- `DDGPU_SHADER_PREFETCH=1`: successful runtime shader-object creation emits
  an owned, advisory token event. DeferredBackend uses `try_send`: a full
  command queue drops speculation without advancing its completion mark.
  Capture forwards the event without changing the frame-file format. The
  backend sends a token hash to the coordinator, which prioritizes matching
  recorded recipes, preserving their relative order.
- A shader object alone cannot specify an exact cold pipeline: input layout,
  paired shader, target formats, depth bias and sampled-channel variants are
  draw state. Therefore events prepare only previously observed exact variants;
  no invented likely states or placeholder shaders. With no valid recipe file,
  the first encounter still creates objects normally and records the recipe.
- Draws never join creators or wait for queued recipes. Missing, in-flight,
  failed or source-mismatched recipes take the original validated creation
  path. Existing current-source digest checks remain mandatory. This avoids
  a new explicit wait; shared driver/CPU contention still needs head's A/B
  before claiming any latency benefit.
- Drop cancels queued work; in-flight GPU creation may finish off-thread.
  `DDGPU_PIPELINE_WARMUP=0` still records without speculative compilation.
  `DDPLAY_PIPELINE_CACHE=0` disables both persistence and recipe warmup.
- `DDGPU_WARMUP` reports creator count, recipes, successfully compiled count,
  shader-event count while compiling, background elapsed milliseconds, pending
  count and cancellation. `DDGPU_PIPELINE_LOG=1` retains per-creation thread,
  phase and disposition. Aggregate GPU counters include overlapping background
  work; pipeline creator CPU falls under the timeline's `other` class.

Implementation hashing invalidates old recipe manifests on this source change.
Seed the candidate once before a warm-cache comparison. Local cached WGSL and
recipes remain in the user's sandbox, never in version control.

## Validation

All commands used `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`,
`CARGO_TARGET_DIR=C:/ddda-targets/L151`. No game or shard run by this agent.

- **7 warmup CPU tests pass**: recipe/checksum/identity/source variants,
  bounded opt-in policy, priority/cardinality across concurrent consumers,
  one attempted failed module across four creators.
- **16 d3d9cmd + 6 d3d9frame tests pass**, including owned-token forwarding,
  accepted-only completion marks, and nonblocking drop on a full draw queue.
- **6 GPU checks pass**, two per N=1/2/4: fully warmed reuse creates no cold
  modules/pipelines; immediate draws while warmup is active preserve both
  target images. Each process had a 60 s hard timeout, launched only after
  `pause.py status` returned `not paused`; all exited 0 in ~2–3 seconds.
  Every seeded case compiled **12/12 recipes**, receiving **24 shader events**.
- Final runtime light suite with `DDGPU_SHADER_PREFETCH=1` and
  `--no-fail-fast`: **211 reported passes / 2 failures / 5 ignored**. This
  includes two nested diagnostic-child passes (209 distinct top-level passes).
  Inherited fullscreen gamma Reset failure remains; the render-tail timing
  failure passed alone. An earlier default run stopped on the Present timing
  test, which also passed alone. No assertions weakened. This is not an
  all-green full-suite claim.

Synthetic target image BLAKE3 (identical across cold/warm/in-flight/N):

```
4049adcf9b48fab62654d0b26749b3f54d8ac43614f97ea64c020b061a85474b
9a9121b3c1af0fe064248ae163cb6c288899a006b692b43b38517c8f1f1f7bde
```

| Creators | Fully warmed 12-recipe coordinator ms | With immediate draws ms |
|---|---:|---:|
| 1 | 21.409 | 49.030 |
| 2 | 11.072 | 47.325 |
| 4 | 12.165 | 49.810 |

Single tiny workload sample, not a benchmark or loading speedup: these elapsed
values include a **0–10 ms coordinator polling tail**, driver cache effects,
and shared-machine scheduling. In-flight timings include contention with
normal draw fallback. Retain opt-in status pending head's game measurements.

Private test logs are `C:/ddda-targets/L151-*.log` and
`L151-gpu-{1,2,4}.{out,err}`. SHA-256 receipts:

```
L151-gpu-cpu-final.log fe7f22aab593544ae7af4568c741ca2c1e5be7772bdcd60a26b82c0ffe65bebd
L151-command-tests.log 7b870c3687238a17eabeb4e517c156310b8a579346cacd085abf28c7c6ce0137
L151-runtime-final.log 31b5e6d0d76caf833f0f7a7ef08de5151b65762557f0998989a835a979337dfa
L151-gpu-1.err eec6260e70982c4981841d08893d2ff126a27d7ac956a3c3bba213429e917893
L151-gpu-2.err c9258815004eada6befc73855bab9e81df6fa352ebec4adb26803753bde17edc
L151-gpu-4.err e72fbc5f8fff6608dfdad411cd8307b4080dd17d0e1aad173fd94106b027dfd3
```

## Head run request

After seeding this exact candidate's recipes, compare identical saves/routes
with `DDGPU_PIPELINE_THREADS=1` vs `2`, then `4` only if 2 helps. Keep
`DDGPU_SHADER_PREFETCH=1`, `DDGPU_PIPELINE_WARMUP=1`,
`DDRUNTIME_LOAD_LOG=1`, `DDGPU_PIPELINE_LOG=1`,
`DDGPU_NATIVE_PIPELINE_CACHE=1`, `DDDRAW_NATIVE=drawcall`, `DDGPU_VK=0`,
`DDRUNTIME_ZLIB_PORT=0`, `DDRUNTIME_FILE_READ_AHEAD=0`, and the selected
c25 FAST policy fixed. Use the same cache directory/temperature and seed,
visible input/route, resolution and no allocation census. Separately compare
SHADER_PREFETCH=0/1 at the selected N. Compare consumer object-miss counts/time,
ready warm hits, boot/menu/save intervals and images. A logging-off acceptance
run is still required. Agent supplies source for head's integration build;
no agent-built ddplay binary.

## Results

Implemented and synthetically tested: parallel exact-recipe creation with
single module creation per digest, advisory shader-load prioritization, and
unchanged draw fallback. Not compared against the original game in L151.
No additional IO changes justified by the available interval evidence.
The c25 timeline has not arrived at this checkpoint (requested on board
#12647); re-ranking and whole-loading acceptance await that external evidence.
Next action: head seeds the integrated candidate and runs the N=1/2 matched
comparison above, supplies c25 timeline, then re-rank the measured intervals.
