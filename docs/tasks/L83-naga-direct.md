# L83: shader module creation cost

Assigned to codex-4 by head, branch `task/L83-naga-direct`, based on
`task/opt-int`. Runtime-only; no game runs or lifted-shard builds.

## Implementation

Production draw-module creation now uses `shader_cache::source`. It translates
with the existing options and interface, then loads the exact Naga frontend
output from disk or parses and saves it on a miss. Cache/parse errors are
resolved before opening the device error scope. wgpu's device-specific
validation remains enabled. No shader arithmetic, linking, or pipeline
compilation changes.

The key is BLAKE3 of the complete generated WGSL, so every translation option
and translator output change invalidates the entry automatically. Interface
metadata is not serialized. The serialization schema is pinned to Naga
29.0.4, with source and payload hashes, a size limit, and temporary-file rename.
Corrupt/missing entries regenerate; I/O failures do not prevent module creation.

- Default: `%LOCALAPPDATA%/ddda-rs/shader-modules`.
- `DDGPU_SHADER_CACHE_DIR=<dir>` overrides the directory.
- `DDGPU_SHADER_CACHE_DISABLE=1` restores the original WGSL module path.
- Headless adapter selection honors `WGPU_BACKEND`; without it, defaults are
  unchanged. Validation flags are unaffected. This permits Vulkan checks when
  all-backend discovery stalls on this host.

L90 coordination: `lib.rs` edits agreed with codex-7 (board 10316); shader
identity/Arc changes are separate. Future compact-constant WGSL changes need
no cache version bump. No whole-game loading-time claim.

## Verification

Target `C:/ddda-targets/L83`, release, incremental disabled, jobs=4:

- Production cache unit tests: **2 pass**, including exact IR, corruption and
  replacement, wrong-source file, disabled caching, miss/hit, and failed writes.
- D3DX reflection rerun: **29,198 shaders, zero differences**, two tests pass.
- Existing all-install WGSL variants: **60,524 validated, zero failures**.
- CPU corpus: **29,198 exact serialized IR roundtrips**, 2,128 vertex and
  27,070 pixel. All cache entries are now seeded for the paired GPU pass.
- `cargo test -p ddruntime --no-default-features --release -- --test-threads=1`:
  **133 pass, zero failures**. This resolves the prior two timing assertion
  failures from the concurrent test run; no runtime test was weakened.
- Production GPU checks: initial cache-populating pass has **29 executed tests
  passing**, two existing exploratory tests ignored, before yielding the GPU
  to L90's head-prioritized gate. Remaining targets and the full warm repeat
  are pending. Each test runs in a fresh process with a hard 60-second timeout,
  after `pause.py status` reports not paused. The interrupted case will rerun.

## Measurement

`cargo run --release -p ddgpu --example module_cost -- <skip> <count> <cache-dir> [cpu]`
reads the player's shader archives, deduplicates bytecode in sorted archive
order, and times each phase independently. No pipelines, draws or window are
created. `DDGPU_BENCH_BACKEND=vulkan` selects only Vulkan for the probe;
`DDDA_SHADER_ROOT` overrides the archive directory. Run bounded batches with
an external 60-second timeout, checking pause status before each.

Require `cache_hits=count` for warm-cache numbers. The seed pass creates
missing entries; immediate post-write reads are correctness checks, not warm
load timing. Standalone validation uses all Naga capabilities; both device
paths additionally perform unchanged device-specific validation.

Completed preliminary **1,000-shader sample**, 764 vertex/236 pixel, all cache
hits, release on RTX 4060 / NVIDIA 610.62 / Vulkan:

| Phase | Mean ms |
|---|---:|
| D3D bytecode parse + WGSL translation | 0.345757 |
| WGSL parse | 3.828870 |
| Standalone validation | 0.249730 |
| Existing disk cache read + decode | 0.451911 |
| wgpu module from WGSL (includes parse/validation) | 3.964825 |
| wgpu module from Naga (includes validation) | 0.250913 |

Including translation and cache I/O: **4.310582 -> 1.048581 ms (75.67% lower)**.
The complete 29,198-shader paired GPU timing remains pending. Full-corpus
seeding showed file-read variability (5.936 ms averaged over hits and misses);
a subsequent 1,000-shader warm CPU sample loaded at 0.341 ms. These observations
are not interchangeable: warm performance depends on filesystem cache state.
The first launch still parses WGSL and pays serialization/write overhead.

Private logs/cache: `C:/ddda-targets/L83/`. No shader data is committed.
Next: finish production GPU checks and the full paired timing pass after
L90 releases the GPU, then post final evidence to head.
