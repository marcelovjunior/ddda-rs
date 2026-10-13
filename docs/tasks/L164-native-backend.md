# L164: native backend constant processing

Agent: codex-9. Branch: `task/L164-native-backend`.
Assigned by head: profile c29nat1's native backend and reduce exact host
overhead. Runtime/ddgpu only; no game run or lifted-shard build. Sink and
worker lifetime/refusal policy remain with claude-2. Planned narrow hooks
posted on board #13158, #13167 and #13172 before editing.

## Evidence

Private inputs: `E:/ddda-re/notes/runs/profile_c29nat1.txt` and
`run_c29nat1.log`. No c29nat raw-stack JSON was found under the private
notes tree. Text profile: 645 rounds / 30 seconds, percentages use busy
samples, not measured function CPU durations. Inclusive rows overlap.

| Thread / operation | Evidence |
|---|---|
| d3d9-backend | 18.5 CPU seconds / 30 seconds; 386 busy, 259 waiting |
| native Worker::run | 242/386 inclusive busy samples (62.7%) |
| native Worker::prepare | 86/386 (22.3%) |
| Uniforms::upload | 61/386 (15.8%) |
| synchronous scatter encoder finish | 27/386 (7.0%); compute encoding 26 |
| integer constant coverage validation | 11/386 leaf samples (2.8%) |
| native texture preparation | 26/386 inclusive samples (6.7%) |
| default/fallback draw path | 111/386 inclusive samples (28.8%) |
| wgpu-finish | 8.9 CPU seconds / 30 seconds; 206 busy, 439 waiting |
| finisher vk::submit | 151/206 inclusive samples (73.3%) |

The final 5,120-job counter interval (attempted 108,544 -> 113,664) has
267,406 draws, 5,714 passes, 85,180,040 upload bytes, 4,254,974 dirty
registers, no refusals and no new pipeline/texture-set objects or resource
resets. This is **52.228 draws/job, 1.116 passes/job, 318.542 uploaded
bytes/draw, 15.912 dirty registers/draw and exactly one submit/job**.
Counters do not time descriptor/pipeline lookups separately; no precise
per-draw game CPU time is inferred from these sampled stacks.

Input SHA-256:

- profile: `c8f504bd11483c6cd703e3bb415b3a60a39406555ebce6bd6567d1f8d28d440d`
- log: `acc68869152d61e50daeb3be4fa1daafa4688a29b924f9ac52dcbf22f00cc023`

## Changes

- Return the resident scatter's unfinished command encoder to the existing
  ordered finisher as its first `Chunk::Wgpu`. Recording and queue writes
  stay on the backend; source-slot tickets, scatter-before-render ordering,
  one submission per job and resource ownership are preserved. Direct
  recorder submission still finishes synchronously. This moves work from
  the backend; it does not eliminate GPU scatter work or queue submissions.
- If the first constant write covers its entire bank, omit the coverage
  bitmap fill/scan. Still check every subsequent range in original order,
  including overflow and empty out-of-bank ranges. The sink supplies full
  integer/boolean banks (`ndraw_native.rs`); partial float ranges retain
  the original algorithm. Regression compares 645 range/pattern cases
  with the previous algorithm, including exact error text.

The proposed 256-byte dirty-scan shortcut was abandoned before implementation:
L152 already measured a dense-update regression. No repeated failed
optimization, worker policy change or game behavior change is included.

## Results

Baseline `3e72b984` and candidate built with the same updated benchmark
harness, `CARGO_TARGET_DIR=C:/ddda-targets/L164`, `CARGO_INCREMENTAL=0`,
`CARGO_BUILD_JOBS=4`, `CARGO_PROFILE_DEV_OPT_LEVEL=3`. No shards built.

- ddgpu CPU: **30 passed / one inherited failure / one ignored**. The new
  645-case coverage regression passes. The baseline binary independently
  reproduces `strip_restart_exclusion_uses_declared_index_range_without_wrapping`
  rejecting the assertion that a widened u16 range must refuse. Owner
  claude-2 reports its correction in separate `ef80de30`; no refusal or
  acceptance assertion was weakened here.
- GPU: **43 passed / zero failed / two benchmarks ignored**, plus **eight
  ordering configurations passed** (resident updates and more-than-four-job
  source reuse, Vulkan off/on and one/four encoder threads). Every process
  started only while not paused and had a 60-second cap; no timeout.
  Suites: native 6, native_job 14, native_compare 4, native_compare_jobs 5,
  vk_encoder 14. Compare tests include intentional mismatch detection.
- Required runtime light suite: **233 passed / inherited fullscreen-gamma
  failure / seven ignored**. The first cargo run stops at d3d9_device;
  remaining integration suites were then explicitly run, all passing.

Paired CPU-side backend timings, three alternating baseline/candidate pairs,
two encoder threads, changing constants, full integer/boolean bank transport,
one pass/job. Time includes job consumption, preparation, recording and final
finisher drain, but not GPU completion. These are synthetic minimal shaders,
not game per-draw timings. Shared-machine variation remains substantial.

| Draws/job | Encoder | Median us/draw before -> after | Median us/job before -> after |
|---|---|---|---|
| 64 | wgpu | 2.673 -> 2.353 (-12.0%) | 171.07 -> 150.57 |
| 64 | raw Vulkan | 2.312 -> 2.216 (-4.2%) | 147.96 -> 141.81 |
| 5,000 | wgpu | 1.572 -> 1.612 (+2.6%) | 7,858.67 -> 8,060.05 |
| 5,000 | raw Vulkan | 1.804 -> 1.832 (+1.5%) | 9,022.21 -> 9,157.29 |

The 64-draw raw case improves in two of three pairs, regresses 0.6% in the
third; the 5,000-draw raw case regresses in all three. Do not infer a general
renderer/FPS gain. All 24 timing runs have identical before/after image
hashes, upload totals and submission counts for their fixture: 64-draw jobs
upload 2,023,056 bytes in 1,562 batches; 5,000-draw jobs 1,900,304 bytes in
20 batches. No new submit batching or upload compression is claimed.

BLAKE3 image receipts (unchanged from L152):

- 96-job paired clears/reuse: `001cc08e551cfe955bbd4e310422e718d91675a0909b85746ec1f846753b268e`
- 64-draw changing jobs: `46607aea6aa1b283858a07dc67fbfbc33baef5f92fcd7eca62768c1e481e40fa`
- 5,000-draw changing jobs: `77adb63c783ccd52bdd2ae668b3e9d7f07e2e82fd4eaad766a006616c537acc3`

Private receipts and runners: `C:/ddda-targets/L164-receipts/`, including
`gate-results.json`, `ordering-results.json`, `bench-results.json`, build
logs, `cpu.log`, `runtime.log` and `runtime-rest.log`. Reproduce with
`cargo test -p ddgpu --lib --test native --test native_job --test native_compare
--test native_compare_jobs --test vk_encoder --no-run`, then run cases
individually and serially with the timeout/pause guard. The ignored
`native_job_encoding_cost` uses `L152_JOB_DRAWS=64|5000`, `L152_PASSES=1`,
`L152_CHURN=1`, `L164_FULL_BANKS=1`, `DDGPU_VK=0|1`, `DDGPU_ENCODE_THREADS=2`.

Next: head's same-scene native VK=1 A/B and native compare run (zero
mismatches), with backend/finisher CPU profiles. Requested on board #13181
and #13318. No game run, installation write, original-image acceptance,
default flip or measured gameplay gain by this agent.

## L159 shadow failure follow-up (head board #13287)

Head's combo30 default-drawcall `DDGPU_VK_SHADOW=997` run completes 1,652
aspect checks with no mismatches, then stops drawing around t=91 seconds:
`DDGPU_VK encode fallback: not Vulkan`, followed by the backend worker's
Present channel disconnect. This run is **not** an acceptance for the default
flip. Non-shadow runs completed, according to head.

Read-only investigation:

- The `not Vulkan` string originates in `vk/pass.rs::raw_batch`, on an empty
  `CommandEncoder::as_hal_mut` callback. Local wgpu-core 29.0.4 source
  (`as_hal.rs::command_encoder_as_hal_mut`, `command/mod.rs::record_as_hal_mut`)
  also produces None for invalid encoders or failure to open the HAL encoder;
  it discards the latter error with `.ok()`. The message does not establish
  a backend mismatch or identify device-loss versus allocation failure.
- Windows System record **2788921**, provider `nvlddmkm`, event **153**, at
  **2026-10-13 11:34:21.022 local / 14:34:21.022 UTC**, records
  `\Device\Video3`, `Error occurred on GPUID: a00`. This supports a driver
  error during the run window, but does not by itself prove TDR, a shadow
  readback fault or the exact trigger. Original run wall-clock correlation
  was requested from head (the copied log's file times are not run times).
- `ddruntime::session::install_fault_hook` suppresses all process panic
  payloads, including host GPU workers. The finisher additionally catches
  the payload and discards it. Therefore absence of panic text is expected
  and does not demonstrate graceful device loss.

Separate diagnostic follow-up: `DDGPU_DIAGNOSTICS=1` installs explicit
device-loss and uncaptured-error stderr callbacks on backend construction.
It keeps uncaptured errors fatal, as wgpu's default handler does; hosts'
existing callbacks remain untouched when the flag is absent. The finisher
prints its caught panic payload before retaining the existing failed-fence
notification. No recovery, error suppression, shader change, default flip
or diagnosis of the exact driver fault is claimed.

Validation: **four encoder-pool CPU tests and two bounded GPU tests pass**,
zero failures/timeouts. The internal ignored diagnostic test deliberately
disables the panic hook, triggers an invalid buffer descriptor, verifies a
fatal panic, then destroys the device. Its stderr retains `Invalid usage
flags BufferUsages(0x0)` and `device lost: reason=Destroyed`. The raw/query
fallback ordering regression also passes. Receipts: `diagnostic-*.log` in
the same private receipt directory. The initial integration-test placement
could not access the private device; the final test is internal, with no
API exposure or production behavior change to accommodate testing.

Next: head repeats the shadow run with **DDGPU_VK=1 DDGPU_VK_SHADOW=997
DDGPU_DIAGNOSTICS=1** and correlates the first device/worker error with
Windows events. The actual shadow-run driver failure remains unresolved;
the default flip must remain pending that diagnosis and acceptance.
