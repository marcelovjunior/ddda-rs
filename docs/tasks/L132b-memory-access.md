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
- Aligned byte/word/dword loads and stores use the same Acquire/Release
  operation in serial and parallel mode. They no longer load the mode
  flag. Unaligned accesses retain serial copies versus parallel byte
  atomics exactly as before. The x64 aligned instructions remain MOVs.

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

Benchmark-only checkpoint: `56006311`; packed-table checkpoint:
`2422747c`. The final checkpoint adds aligned-access specialization and
the same-process measurements below.

The separate-executable measurements were too noisy: even the unchanged
`page_table` control drifted substantially. `scripts/bench_memory_accessors.py`
therefore compiles baseline `941f5d2c` and current **repository memory code**
into two distinctly named crates in one process. There are no shards or
game inputs. Each case runs 11 alternating AB/BA pairs, 5 million accesses
per sample, pinned to logical CPU 2 (mask 4). Three repetitions use the
same executable. It reports minimum/median/maximum wall time, thread
cycles and the median paired new/old ratio. This follows head #11686.

Final timing (ns/access; median of the three process medians). Ratios are
the median of each process's **paired** ratios, not the quotient of the
two aggregate medians. Shared-machine scheduling/frequency noise remains;
especially the small aligned-read gain needs the head's gameplay profile.

| Access | Serial before -> after | Paired ratio | Atomic before -> after | Paired ratio |
|---|---:|---:|---:|---:|
| page_table control | 1.880 -> 1.814 | 0.990 | 1.964 -> 2.071 | 0.987 |
| u32 aligned hit | 3.221 -> 3.045 | 0.960 | 3.729 -> 3.700 | 0.941 |
| set_u32 aligned hit | 3.955 -> 2.889 | 0.752 | 4.147 -> 3.618 | 0.853 |
| u32 unaligned hit | 3.158 -> 3.304 | 1.048 | 4.398 -> 4.082 | 0.945 |
| set_u32 unaligned hit | 4.169 -> 3.638 | 0.851 | 4.431 -> 3.822 | 0.865 |
| u32 straddle | 12.073 -> 12.518 | 1.004 | 12.194 -> 11.972 | 1.004 |
| set_u32 straddle | 9.856 -> 8.758 | 0.872 | 10.165 -> 8.876 | 0.895 |
| u8 hit | 2.703 -> 2.505 | 0.921 | 2.883 -> 2.623 | 0.907 |
| u32 absent table | 10.750 -> 10.316 | 0.893 | 10.810 -> 10.502 | 0.985 |
| set_u32 absent table | 8.807 -> 7.902 | 0.897 | 8.999 -> 7.689 | 0.895 |
| u8 absent table | 4.085 -> 4.123 | 1.010 | 4.001 -> 4.144 | 0.975 |
| u32 permission miss | 10.918 -> 10.483 | 1.032 | 10.741 -> 10.252 | 0.896 |
| set_u32 permission miss | 8.661 -> 7.734 | 0.886 | 8.674 -> 7.940 | 0.880 |
| u8 permission miss | 4.407 -> 4.208 | 0.942 | 4.183 -> 3.630 | 0.982 |

The serial unaligned read regresses about 5%; straddling reads are flat.
No claim that every case improves. Byte accesses cannot straddle. The
synthetic page_table control has no runtime PREEMPT poll; it checks
measurement stability, not the cost of runtime scheduling.

Reproduction:

```
python scripts/bench_memory_accessors.py --baseline 941f5d2c --iterations 5000000
C:/ddda-targets/L132b/paired/paired.exe 5000000
C:/ddda-targets/L132b/paired/paired.exe 5000000
```

Local logs: `C:/ddda-targets/L132b/paired-ratio-{1,2,3}.txt`. The original
helper_cost remains runnable with `HELPER_MEMORY_ONLY=1` and profile play.

Verified final code: **31 base tests pass** in debug and play;
**188 runtime light tests pass, one inherited fullscreen-gamma Reset
failure, three ignored**. The complete light suite was rerun after the
final aligned-access change; the same gamma assertion is the only failure.
The runtime regression compares full values, fault strings, lazy backing
counts and page contents across mapping/protection changes with the same
cached table. No assertion was weakened.

Commands (with the environment above): `cargo test -p ddnative-base`,
`cargo test -p ddnative-base --profile play`, and
`cargo test -p ddruntime --no-default-features --no-fail-fast -- --test-threads=1`.
Final local logs: `base-ship-debug.log`, `base-ship-play.log`, and
`runtime-ship.log` under `C:/ddda-targets/L132b`.

Not yet verified: combined native vectors and gameplay/image/FPS. No game,
GPU or shard run by this agent. Next action: codex-7/head's next combined
native replay, then head's same-scene A/B. Milestone summary goes to the
head because codex-7 owns `docs/MILESTONES.md` during L134.
