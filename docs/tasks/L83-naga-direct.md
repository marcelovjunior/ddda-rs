# L83: shader module creation cost

Assigned to codex-4 by head, branch `task/L83-naga-direct`, based on
`task/opt-int`. Runtime-only; no game runs or lifted-shard builds.

Measure translation, WGSL parsing/validation and module creation over all
29,198 distinct installed game shaders. Reduce frontend cost without changing
shader behavior; keep driver equivalence and reflection checks passing.

## Method

`cargo run --release -p ddgpu --example module_cost -- <skip> <count> <cache-dir> [cpu]`
reads the player's shader archives, deduplicates bytecode in sorted archive
order, and measures each phase independently. GPU runs create modules only,
without pipelines, draws or a window. Run batches with a 60-second external
timeout after checking `pause.py status`. Cache files and logs stay outside
the repository. `DDDA_SHADER_ROOT` overrides the archive directory.
`DDGPU_BENCH_BACKEND=vulkan` selects only Vulkan for the probe; no override
uses normal wgpu discovery. Require `cache_hits=count` for warm-cache numbers.
The first pass creates missing entries; its immediate post-write reads are
correctness checks, not warm-cache timing. A second pass measures existing
entries. Standalone validation uses all Naga capabilities; both device paths
also perform wgpu's unchanged device-specific validation.

The proposed cache serializes Naga's exact parsed module, keyed by the full
generated WGSL. It retains translation and device validation; it does not
substitute approximate IR or bypass validation. Its schema namespace pins
Naga 29.0.4. Cache entries bind their source hash and payload checksum, and
are written by temporary-file rename. Missing/corrupt entries are misses.

## Results

In progress; full-corpus GPU timing and backend integration still pending.

- Focused cache tests: **2 pass** (exact IR, source-key binding, corrupt and
  truncated entries, disabled cache, miss/hit, non-writable directory).
- D3DX reflection: **29,198 shaders, zero differences**, both tests pass.
- Existing all-install WGSL variant test: **60,524 translations validated,
  zero failures** (default and backend option variants).
- Runtime light release suite: 11 unit tests passed, two existing timing
  assertions failed under concurrent machine load (Present throughput and
  Interlocked latency). No runtime source changes; serial rerun pending.
- Default adapter discovery exceeded the **60-second watchdog** before
  adapter info. L79 independently saw the same hang. Vulkan-only discovery
  works: RTX 4060, NVIDIA 610.62. This does not establish the failing backend
  or change the runtime's backend selection.
- Preliminary **100 pixel shaders**, indices 1000..1099 in the benchmark's
  sorted/deduplicated order, all 100 cache hits, optimized release:

| Phase | Mean ms |
|---|---:|
| D3D bytecode parse + WGSL translation | 0.409365 |
| WGSL parse | 4.678850 |
| Standalone validation | 0.323402 |
| Existing disk cache read + decode | 0.503874 |
| wgpu module from WGSL (includes parse/validation) | 5.105689 |
| wgpu module from cached Naga (includes validation) | 0.313754 |

Before including translation: **5.515054 ms**; warm-cache after including
translation and disk read: **1.226993 ms**, 77.75% lower on this sample.
No claim about whole-game loading time or pipeline compilation. The first
launch still parses WGSL and pays cache serialization/write overhead.

Private logs/cache: `C:/ddda-targets/L83/`. The all-shader CPU roundtrip is
in progress. Full GPU batches refused
to start when the head's pause became active; no GPU batch ran while paused.

After the pause lifted, the first **1,000-shader batch** completed: 764 vertex,
236 pixel, all 1,000 cache hits, exact serialized IR equality for each. Means:
translation **0.345757 ms**, parse **3.828870 ms**, validation **0.249730 ms**,
cache read/decode **0.451911 ms**, WGSL module **3.964825 ms**, Naga module
**0.250913 ms**. Including translation and cache I/O: **4.310582 -> 1.048581 ms
(75.67% reduction)**. Batch 1000 was interrupted by another shared pause and
killed by its 60-second watchdog; it contributes no measurements. Full
29,198-shader timing remains **unfinished**. The CPU seed process (40768)
is suspended by the head's pause; no L83 GPU process remains running.

Checkpoint: `3a956d94` on `task/L83-naga-direct`. The intended production
hook adds `mod shader_cache;` to ddgpu's lib.rs, obtains
`let source = shader_cache::source(t.wgsl)?;` **before** pushing the device's
error scope, and passes that source to draw-module creation. A separate
headless test adapter selection override is requested to permit Vulkan
driver-equivalence tests. Both await the head's stale-claim decision.
