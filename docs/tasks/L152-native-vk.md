# L152: native job Vulkan encoding

Assigned to codex-9 by head, 2026-10-13. Branch `task/L152-native-vk`,
base `task/fps-line` (`3b9f79fb`). Prerequisites: L148 `93d55d1f` and
`5f8c3931`, replayed here as `0bb1e4c3` and `d86a17ed`.

Scope: accepted nDraw native jobs through the raw Vulkan encoder under
`DDGPU_VK=1`; retain wgpu and D3D9 comparison paths, exact image semantics,
atomic job rejection, resource lifetime, and ordered submissions. No game
execution, generated shards, game data, or private dumps in this branch.
Coordinate native.rs/native_job.rs changes with claude-2 and upload hooks
with codex-11. Builds use `C:/ddda-targets/L152`, incremental off, four jobs.
GPU tests require an unpaused machine and a 60-second process timeout.

## Profile evidence

Head's private `notes/runs/profile_c24nat.txt`: 380 rounds in 30.1 seconds,
245 busy backend samples; backend CPU 18.9 seconds (63%). Of backend busy
samples, inclusive `Worker::run` is 62.4%, `Worker::prepare` 15.5%,
`Uniforms::upload`/`Recorder::finish_parts` 12.2%, queue submit 10.2%, and
`update_constant_words` 9.8%. Self samples: heap free 6.9%, allocation 4.5%,
constant-word updates 5.3%, prepare 4.5%. These shares overlap; they cannot
be added as independent wall times. Export-only `_NLG_Return2` and Vulkan
loader symbol labels do not establish exact copy/driver attribution (L147).

The recorder currently creates wgpu passes, scans/copies constant words,
and uses a per-job resident scatter upload. Raw encoding can reuse its
validated state and immutable object handles. Head's existing comparison
receipt (130,304 matched / zero mismatched) is the baseline, not validation
of this new code.

## Results

Verified source checkpoint **cf74be1b**. Profile posted to head/claude-2 on board #12577. Head granted
narrow native hook edits in #12655, with hook inventory in #12663/#12678.
Codex-11 agreed disjoint hooks
and prerequisite replay (#12586/#12595), and codex-6 agreed the narrow
Vulkan pass initializer changes in lib.rs (#12616).

Implemented, opt-in: Worker `new_cached` devices collect `vk::Pass/Draw`
using the existing native validation, constant snapshots, sampler bias,
and dynamic state. The public native API remains the wgpu oracle,
including its optional bundles. A separate `native_vk.rs` bridge caches
raw bindings by native pipeline/texture-set handles. Device-owned raw
pipeline templates survive resource resets without retaining textures or
uniform arenas; each recorder binds its own uniform buffer. First-use and
unsupported raw passes retain the same wgpu-native fallback. Raw pass
support includes colour/depth/stencil clears and first-instance offsets.

Submission preserves the existing wait-before-upload, scatter-before-draw,
and ordered finisher boundaries. Finished scatter commands and rendering
share one queue batch per dirty job (previously two submits), exposed as
`NativeJobStats::submit_batches` / the native-worker log field. Raw pass keepalives retain their Vulkan
objects through GPU completion. L150's separate pending-upload flush hunk
must remain immediately before `wait_finisher` when integrating that lane;
this branch does not change upload-ring code.

The resident upload scanner short-circuits an identical complete snapshot
sequence, otherwise preserving the original scalar dirty-register loop. A
scalar-record regression checks snapshot lengths and repeated generations.
`update_constant_words`, worker lifetime/refusal policy and nDraw code
remain unchanged.

Final verification (optimized workspace crates, dev dependencies opt-level 2):

- **27 CPU tests pass**, one timing ignore; the timing test separately passes.
- **53 GPU cases pass / zero failures / zero timeouts**, plus one ignored
  benchmark: 41 cases with `DDGPU_VK=1` (native jobs, comparison jobs,
  comparison reduction, the public native oracle and the raw encoder),
  then all 12 active native-job cases with `DDGPU_VK=0`. Every GPU process
  checks pause status, runs serially and has a hard 60-second timeout.
- The paired **96-job** replay is byte-exact, exercises **44 actual raw
  passes**, checks one queue batch per accepted job and covers changing
  constants, clears, depth/stencil, async ordering and resource reuse.
- Held-buffer release creates **zero additional wgpu or raw pipelines**.
  The full native-job suite retains rejection atomicity, sampler identity,
  strip start/base/culling, depth bias, missing sampled channels and queries.
  Native comparison jobs include selected mip attachments and shadow reuse.
- Runtime light: initial inherited fullscreen-gamma failure; complete run
  excluding that named test: **212 pass / zero fail / five ignored**.
- Plain `cargo test -p ddgpu --no-run` also builds examples and encountered
  the inherited `module_cost` missing `pipeline_log` declaration. Final
  build used `--lib --tests`; codex-6 supplied independent fix `5da79cfb`.

The first development clear-pass run caught a missing HAL `LOAD_CLEAR`
flag; fixed before acceptance. A public-native stats assertion caught an
unintended oracle opt-in; the public API now stays on wgpu. All final GPU
cases were rerun after the pipeline-template and submission changes.

### Synthetic timings

Three alternating off/on pairs, same executable, 100,000 draws (99,968
for 64-draw jobs), two encoder workers. Fixture job creation is outside
measurement; consumption, destruction, prepare, recording, finish and
ordered submit are inside. GPU readback/completion checks are outside.
Minimal shaders/one pipeline and an 8x8 target: **not gameplay/FPS evidence**.
Both columns include the new submission and identical-upload fast paths.

| Workload | Native wgpu us/draw | Native raw us/draw |
|---|---:|---:|
| 5,000 draws/job, one pass, identical constants | 1.108 | 1.172 |
| 5,000 draws/job, one pass, changing constants | 1.249 | 1.197 |
| 64 draws/job, one pass, changing constants | 2.495 | 2.505 |
| 5,000 draws/job, 70 passes, changing constants | 1.313 | 1.423 |

**24 timing runs pass the same-image checks.** Raw is mixed here; no
universal speedup or game-FPS claim. A separately compiled split-submit
baseline isolates the queue-batch change. Its source modification was
temporary and production source was restored byte-for-byte before committing.
Three alternating pairs per row, same workload and matching image hashes:

| Changing-constant jobs | Split submit us/draw | Combined submit us/draw |
|---|---:|---:|
| 64 draws/job, native wgpu | 3.973 | 4.232 |
| 64 draws/job, native raw | 3.955 | 3.893 |
| 5,000 draws/job, native wgpu | 2.544 | 2.202 |
| 5,000 draws/job, native raw | 2.568 | 2.538 |

**24 additional timing runs pass** with no timeout/failure. Shared-machine
timings were about twice the earlier sweep, so compare only alternating
pairs within this window. Queue submits are reduced from two to one for
dirty jobs, but a reliable net timing gain is **not established**: the
small wgpu-job case regresses and the raw differences are small/noisy.
Head's native-game profile is still required. The per-job finisher wait
is unchanged; claude-2 agreed to take its source-slot/ordered-upload
follow-up on top of this bridge (board #12916/#12927/#12939).

The 256 KiB resident scanner microbenchmark (three alternating pairs):
unchanged **14.163 -> 5.812 us**, sparse changes **16.360 -> 16.514 us**,
all registers dirty **40.205 -> 39.180 us**. Changed cases are effectively
flat under shared-machine variation. An earlier 256-byte-region skip
experiment regressed all-dirty scans ~27% and was discarded.

Native constants retain the existing resident scatter/staged upload path.
The backend's mapped-ring counter does not mean this new native recorder
uses mapped constants. Warmup caches, worker lifecycle, refusal policy,
`update_constant_words` and nDraw decoding remain with their owners.

BLAKE3 receipts:

- 96-job paired clears/reuse:
  `001cc08e551cfe955bbd4e310422e718d91675a0909b85746ec1f846753b268e`
- Repeated 5,000-draw job:
  `4934a9ea2c92c39b42f60b8fbf6255122f412c977255f0f55bf767a2b022b21d`
- Changing 5,000-draw job (one or 70 passes):
  `77adb63c783ccd52bdd2ae668b3e9d7f07e2e82fd4eaad766a006616c537acc3`
- Changing 64-draw jobs:
  `46607aea6aa1b283858a07dc67fbfbc33baef5f92fcd7eca62768c1e481e40fa`

Receipts are outside the repo: `C:/ddda-targets/L152-final-gates/`
(`results-gate.json`, `results-bench.json` and individual logs),
`L152-bench-run.log`, `L152-scan-cost-final.log`, `L152-cpu-final.log`,
`L152-runtime.log` and `L152-runtime-rest.log` in the same parent directory.
The isolated submission receipts are `L152-submit-bench/results.json` and
`L152-submit-run.log`, also in that parent.
The build cache `C:/ddda-targets/L152` is removed at final handoff per head.

Reproduction: set `CARGO_TARGET_DIR`, `CARGO_INCREMENTAL=0`,
`CARGO_BUILD_JOBS=4`, `CARGO_PROFILE_DEV_OPT_LEVEL=3`; build
`cargo test -p ddgpu --lib --tests --no-run`, then run each selected GPU
case in its test executable with `--exact NAME --test-threads=1` and a
60-second process cap while unpaused. The ignored `native_job_encoding_cost`
accepts `L152_JOB_DRAWS=64|5000`, `L152_PASSES=1|70`, `L152_CHURN=0|1`.

Next: head's same-scene **DDDRAW_NATIVE=1, DDGPU_VK=0/1** A/B, then
**DDDRAW_NATIVE=compare, DDGPU_VK=1**. Require zero comparison mismatches,
inspect native `submit_batches`, pipeline creation/resource resets, raw
fallback/error counters, backend CPU and whole-game FPS. No game or shard
run was performed by this agent.

Final handoff: source remains `cf74be1b`; the later results commit changes
only this brief and the milestone. All GPU/build processes have exited.
The private build cache and temporary timing executables are removed per
head's disk-space instruction; text receipts remain outside the cache.
No push, game launch, installation change or generated-shard build occurred.
