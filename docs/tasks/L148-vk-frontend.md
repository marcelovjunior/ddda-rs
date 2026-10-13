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

## Results (checkpoint; final sweep pending)

Runtime light: **205 passed / inherited fullscreen-gamma failure / 5 ignored**
with --no-fail-fast. Earlier ddgpu CPU suite: **24 passed**. No runtime edits.

Before the cached-memory refinement: **13 focused GPU cases passed**, with
byte-exact raw/wgpu readbacks. Final cached-memory candidate: plain draw and
**144,000 changing-constant draws, at least two mapped-ring wraps**, pass,
including float32 readbacks with four encoding workers. All exercised raw
passes report zero encode errors. Final complete GPU sweep is pending the
head's combo24 run pause.

The first mapped allocation used HAL's CpuToGpu choice; total timings were
mixed/regressed and were not accepted as a speedup. Requiring cached coherent
memory, the empty-texture identity shortcut and one-stream inline recording
produced one provisional run at **0.551 us/draw** (one pipeline) and **0.964
us/draw** (40 pipelines / 200 texture sets), including finish and submit.
Producer-only intervals were **0.443 / 0.478 us/draw**. These are synthetic
4x4 images, not original-game/FPS acceptance. Stable alternating repeats are
still pending: the third pair was interrupted by the head's pause and its
60-second timeout is discarded, not reported as a correctness failure.

Readback digests printed by these tests are **BLAKE3**, not SHA-256 (the
older L138 prose mislabeled the same benchmark digests):

| Fixture | BLAKE3 |
| --- | --- |
| One pipeline | `120322b15e69f1243bae28f95644b7276d368c2bbddec86e72914546e8cc6160` |
| 40 pipelines / 200 texture sets | `1e3a9ab2a631563f429d70cc6e061b3a6a20f329890c534125e09a2d4d07d1d3` |
| 900 passes | `1ac0a064b588aeaedd03c1e34efb91b237938b411c5f501310adb6f556312045` |
| Mapped-ring wraps | `ca920d1a469337ed646ffcc4833ee434e066c7ad817a03fe61f81c64fe05f46c` |

Logs/executables remain local under C:/ddda-targets/L148*. No game data or
private research artifacts are committed. No game or lifted shard run.

Next: finish final bounded GPU sweep and three alternating timing pairs once
pause.py reports not paused, then request head gameplay image/FPS acceptance.

## L150 integration agreement

L150 owns staged Ring storage and new reserve/is_mapped methods, upload/copy
commands and finisher staging-page completion. L148's primary mapped uniforms
win under DDGPU_VK=1; L150 can handle ordinary wgpu or unsupported-memory
fallbacks. Preserve the mapped tail allocator and GPU reuse fence.
