# L132b: guest memory accessor cost

Owner: codex-11. Base: task/fps-line `941f5d2c`. Branch:
`task/L132b-memory-access`, worktree `E:/ddda-rs-wt/L132`.

Head assignment: measure `fast_u32`, `fast_set_u32`, and `fast_u8` hits,
straddles and misses; reduce their hit-path cost without changing faults,
permissions, lazy memory, fixtures or watch behavior. Runtime-only light
tests; native vectors ride the next combined gate, after L134's current
batch. No game runs or shard builds by this agent.

## Implementation

- Replace separate pointer and permission arrays with one atomic entry
  per page: PAGE-aligned pointer, full eight-bit permission value, and
  two backed-access bits. Each backed-access bit certifies both a non-null
  pointer and its corresponding permission. An unbacked lazy page retains
  its public permission byte but cannot pass the fast test.
- Permission updates and materialisation use atomic read-modify-write
  operations so neither can overwrite the other's state. Unback atomically
  clears pointer and backed-access bits while retaining permissions.
- Preserve page allocation, retirement and freeing: removed pages remain
  alive until table destruction. The table arrays shrink from 9 to 8 MiB
  on the x64 host; this is not a fix for retained backing allocations.
- Preserve the public Mem/page-table API and out-of-line fast accessors.
  `GuestMemory::page_table()` still polls PREEMPT at every lifted entry;
  removing that poll would change serial guest scheduling. No change there.
- Slow trait calls are cold. Optimized assembly has one indexed atomic
  entry load, a single backed-permission test, and no pointer-null test.
  An internal `Option<*mut u8>` is fully eliminated by optimization; the
  experimental raw-null return generated a redundant null test and was
  rejected. Width constants fold the straddle check (none for `u8`).

## Verification and measurement

`helper_cost` now includes byte hits, read/write straddles, absent-table
fallbacks, and permission-denied table lookups. The synthetic Mem fallback
uses byte-wise methods and deliberately does not panic on permission miss;
these numbers measure dispatch/access cost, not fault unwinding. A byte
access cannot straddle a page. Both serial and atomic access modes run.

All cargo commands use `CARGO_INCREMENTAL=0`,
`CARGO_TARGET_DIR=C:/ddda-targets/L132b`, `CARGO_BUILD_JOBS=4`.

New tests cover every permission byte, all helper widths, page boundaries
and address wrap, retirement/reback zeroing, concurrent permission updates
and materialisation, plus runtime fast-versus-checked values/fault strings
with a cached table across lazy/protect/unmap/remap transitions.

## Results

Benchmark-only checkpoint: `56006311`. Base debug and play tests pass;
new cached-table runtime regression passes. Full runtime light serial
suite completed with only the inherited fullscreen-gamma Reset failure.
Final timing results pending the end of head's game window. Initial noisy
measurements were discarded. No native replay, game equivalence or FPS
improvement claimed; next action is the combined native gate and head A/B.
