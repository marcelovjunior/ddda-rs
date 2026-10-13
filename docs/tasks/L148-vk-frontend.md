# L148: Vulkan draw frontend

Assigned by head to codex-9, 2026-10-13. Branch `task/L148-vk-frontend`,
worktree `E:/ddda-rs-wt/L148`, base `4c5394ca` on `task/fps-line`.

## Scope

Merge L143 `e04b76ce` first, retaining L138 raw recording inside
`try_draw_buffered` and passing `&key.key` to `vk.pipeline`.
Profile and reduce DDGPU_VK=1 per-draw preparation, descriptor lookup,
constant upload and resource tracking toward 1 us/draw. Keep byte-exact
wgpu fallback and safe GPU lifetimes; opt-in remains DDGPU_VK=1.
No game runs, shard builds, installation writes or private data commits.

Owned implementation: ddgpu lib/backend_data/cache/encode/vk; focused
vk_encoder tests and backend benchmark; L143 prerequisite source files.
Other agents retain compute_quads, native worker and runtime work.

## Validation

CARGO_TARGET_DIR=C:/ddda-targets/L148, CARGO_INCREMENTAL=0,
CARGO_BUILD_JOBS=4. Light runtime suite uses --no-default-features.
GPU tests run individually only when pause.py reports not paused, each
with a 60-second subprocess timeout. Only head runs gameplay.

## Implementation

- L143 dependency merged as `c7f7e7c8` with both module declarations,
  `&key.key`, the raw recorder, and the Vulkan tracker's FxSet alias retained.
- DDGPU_ENCODE_PROFILE adds derivation, binding lookup, geometry, constant
  packing and raw recording timings. Unprofiled production_cost reports the
  producer interval separately from the total including finish/submit.
- Retained Derived/texture-list identities memoize both the raw pipeline /
  descriptor sets and wgpu fallback objects. Full identity checks resolve
  hash collisions. Empty texture lists use the Derived's OnceLock directly.
  Feedback snapshots bypass this cache; release clears its retained owners.
- Each raw draw retains one shared binding record. The common one-stream
  vertex list fits inline, removing its per-draw heap allocation. Pass
  preambles clone each distinct buffer once and visit each descriptor group
  once; dynamic bounds checks and whole-texture usage conflict checks remain.
- Under DDGPU_VK=1, constants are packed into a persistent HOST_VISIBLE |
  HOST_COHERENT | HOST_CACHED Vulkan allocation, initialized before managed
  import into wgpu. Wgpu owns buffer/allocation destruction. Writes only
  touch fresh tail ranges; queue submission publishes coherent host writes.
  Ring wrap waits for the ordered finisher, then GPU completion. Raw batches
  and fallback commands keep the same buffer alive until completion.
- Devices without cached coherent uniform memory retain staged uploads.
  No native queue submit, game behaviour, shader translation, or default
  DDGPU_VK setting changes. A whole-device wait at ring wrap is conservative;
  head gameplay must assess stalls and GPU performance of host memory.

## Combo24 initialization-history regression

The head's original-game c24vk run (pre-L148, fps-line `4c5394ca`) regressed
from its default path; `profile_c24vk.txt` in the private runs directory shows
**63.8% / 62.3% self CPU** on the two encoding workers cloning the global
HashSet<TextureView> in Pass::finish_batch. View drops add **11.2% / 7.2%**;
the clone holds the shared initialization mutex. This is direct CPU evidence
and a serialization mechanism; unresolved wait stacks do not prove that every
wait was this lock or exclude other GPU/driver waits. The new mapped-ring
reuse fence did not exist in that game build.

Follow-up `5f8c3931` removes global view history entirely. Each owned raw View
has an atomic initialized flag, published only after ordered queue submission.
A chunk records only its own earlier initialization passes locally. Buffer
membership queries inspect current-pass resources under the shared mutex;
there is no global history clone. Fresh feedback views leave Context's lookup
table immediately after descriptor construction; recorded descriptors and
submission keepalives retain them through GPU completion. `live_views` exposes
current ownership in the Vulkan stats. This fixes the measured mechanism;
only head can establish the resulting game FPS.

## Final results

Source commits: `c7f7e7c8` (L143 merge), `93d55d1f` (frontend/mapped constants),
`5f8c3931` (initialization-history/feedback ownership). Opt-in remains DDGPU_VK=1.

- **24 ddgpu CPU tests passed.**
- **14 focused byte-exact Vulkan/wgpu GPU tests passed**, each separately
  within 60 seconds. Includes plain/indexed/strip/scissor/MRT/depth/blending,
  900 dependent float32 passes (**2,698 raw / 2 initialization fallbacks**),
  resource recreation, query/copy fallback with one/four workers, and the two
  new stress cases below. No encode errors.
- **144,000 changing-constant draws / at least two mapped-ring wraps** match
  byte for byte. Additive float32 blending makes every earlier draw contribute
  to each frame's result; later opaque draws cannot hide an overwritten range.
- **640 dependent feedback draws** match byte for byte, with exactly **one
  persistent attachment view** after each of ten queue drains. Temporary
  snapshot views do not accumulate in either context or initialization state.
- **10 additional GPU cross-path tests passed** with DDGPU_VK=1: rejected
  draw/reused resources, compact constants/ring rollover, async upload ordering,
  instancing, deferred frame equality, render-to-texture, cube targets,
  sampling the active target, MSAA resolve/copy, depth/stencil copies.
- Runtime light, --no-default-features --no-fail-fast: **205 passed / one
  inherited fullscreen-gamma failure / 5 ignored**. No runtime changes.

No final GPU gate failure or timeout. An earlier third benchmark pair hit its
60-second cap during the head's combo24 pause transition; it was discarded.
No game or lifted shard run, installation changes, private artifacts or push.

### Paired microbenchmarks

Three alternating saved-baseline/final-binary pairs, with **no owned build or
other GPU jobs running**, each reporting median of nine frames after three
warmups. Baseline is the L143+L138 merge with the same optional profiling and
producer timer additions; profiling is off during timings. All three paired
runs improve both draw-heavy shapes. These are synthetic 4x4 workloads on a
shared machine, not game FPS or the prefilled raw API floor.

| Workload | Raw merged baseline total | Raw L148 total | Raw baseline producer | Raw L148 producer | L148 wgpu total |
| --- | ---: | ---: | ---: | ---: | ---: |
| 5,000 draws, one pipeline | 2.672 | 1.995 | 2.300 | 1.775 | 2.600 |
| 5,000 draws, 40 pipelines / 200 sampled sets | 3.647 | 2.985 | 2.362 | 1.847 | 4.245 |
| 900 draws / 900 passes | 7.331 | 7.226 | 2.562 | 2.026 | 24.965 |

Units: microseconds per draw/pass. Total includes preparation, ring upload,
finishing and submission; producer stops before Present. Per-run raw totals:

| Shape | Baseline runs | L148 runs |
| --- | --- | --- |
| One pipeline | 2.701 / 2.389 / 2.672 | 2.176 / 1.995 / 1.933 |
| 40 pipelines | 3.727 / 3.647 / 3.549 | 3.261 / 2.904 / 2.985 |
| 900 passes | 7.668 / 6.870 / 7.331 | 7.226 / 6.956 / 7.488 |

Median total reductions: **25.3% / 18.2%** for the two draw-heavy shapes.
Tiny-pass results overlap: no additional pass-heavy speedup claim.
Baseline/final wgpu one-pipeline medians are 2.598/2.600 us, illustrating the
similar load during this paired set. A previous lighter-load cached-memory
checkpoint produced **0.551 / 0.964 us/draw total** (producer **0.443 / 0.478**)
in one run; that is a single observation, not the final repeated result or a
sustained 1 us guarantee. The first generic HAL CpuToGpu mapped allocation was
mixed/slower; requiring cached coherent memory replaced that experiment.

Final instrumented one-pipeline phase medians (5,000 draws): derive **2.664 ms**,
bindings **0.320**, geometry **1.448**, constants **3.083**, raw recording **2.083**,
finish CPU **1.107**. Instrumentation takes clocks and a mutex per sample;
these phase numbers identify costs and must not be summed as uninstrumented
CPU latency. The prototype's texture lookup/hash stage is now the smallest
reported frontend phase for an empty texture set.

### Image hashes and reproduction

Tests print **BLAKE3**, not SHA-256 (older L138 prose mislabeled the same
benchmark digests). Every paired benchmark matches both paths exactly.

| Fixture | BLAKE3 |
| --- | --- |
| One pipeline | `120322b15e69f1243bae28f95644b7276d368c2bbddec86e72914546e8cc6160` |
| 40 pipelines / 200 texture sets | `1e3a9ab2a631563f429d70cc6e061b3a6a20f329890c534125e09a2d4d07d1d3` |
| 900 passes | `1ac0a064b588aeaedd03c1e34efb91b237938b411c5f501310adb6f556312045` |
| Dependent float32 ping-pong | `8c0b60b4289cc3eb5ed326d8697b80d331da37bab674b57dda1923fe4bd980cb` |
| Additive mapped-ring wraps | `f0ab5a6f57ace5127864caa78176c46c05a03e1c393ba68ea2b83509fa064952` |
| Feedback retirement | `e6b9903192df0b056bd9065633495bb0913f436a55e10ad92963f777b6710761` |

Build with `cargo test -p ddgpu --test vk_encoder --no-run --config
"profile.dev.package.ddgpu.opt-level=3"` and the environment above, then run
the test executable per case, --exact NAME --nocapture --test-threads=1.
Benchmark adds --ignored --exact production_cost. DDGPU_ENCODE_PROFILE=1
adds phase diagnostics. Check pause and impose a 60-second process timeout.
Local logs: C:/ddda-targets/L148-final-gpu-*.log, L148-cross-*.log,
L148-result-pair-{0,1,2}-{before,after}.log, L148-result-profile.log,
L148-unit-final.log and L148-runtime-full.log.

Next action: head integrates the source commits, then repeats original-game
DDGPU_VK=0/1 image/FPS/profile capture. Check live_views stays bounded, the
history clone disappears, mapped_rings/restarts are exercised, and whether
host-memory GPU reads or conservative ring-wrap fences introduce stalls.

## L150 integration agreement

L150 owns staged Ring storage and new reserve/is_mapped methods, upload/copy
commands and finisher staging-page completion. L148's primary mapped uniforms
win under DDGPU_VK=1; L150 can handle ordinary wgpu or unsupported-memory
fallbacks. Preserve the mapped tail allocator and GPU reuse fence.
