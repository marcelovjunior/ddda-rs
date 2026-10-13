# L168: guest-memory locality

Assignment: codex-9, branch `task/L168-tlb`, from `task/fps-line`.
Scope: ddnative-base memory allocation, isolated CPU diagnostics, head-run
hardware-counter protocol. No game, GPU, shard build, or system-policy change.

## Findings

`PageTable` already has a contiguous 8 MiB array of packed pointer/permission
entries. Rearranging entries would introduce a new translation rather than
remove an existing scattered permission lookup. Guest backing was one
4 KiB-aligned allocator allocation per materialized guest page. The reported
20–35% accessor self time and L142's null gameplay result do **not** establish
that TLB misses are the cause. No hardware miss rate has yet been measured.

Host: AMD Ryzen 7 5700X3D. `GetLargePageMinimum()` returns 2,097,152.
`whoami.exe /priv` shows no `SeLockMemoryPrivilege`; a direct
`VirtualAlloc(NULL, 2097152, MEM_RESERVE|MEM_COMMIT|MEM_LARGE_PAGES,
PAGE_READWRITE)` returns NULL/error **1314** (privilege not held). Head
confirmed no policy change in board #13563. The API's privilege and
size/alignment requirements are documented by
[Microsoft](https://learn.microsoft.com/windows/win32/memory/large-page-support).

## Implemented, opt-in

- Default/unrecognized `DDNATIVE_BACKING`: original per-page allocation.
- `DDNATIVE_BACKING=regions`: reserve a 4 MiB host address range per touched
  2 MiB guest region, use its 2 MiB-aligned interior, commit only the requested
  4 KiB slots. Neighboring guest pages get neighboring host addresses even
  when materialized out of order. This is **ordinary 4 KiB OS backing**,
  not physically contiguous or large-page memory.
- `DDNATIVE_BACKING=large`: first try a real 2 MiB large-page allocation.
  On unsupported size/allocation failure, report the error once per table
  and use aligned regions thereafter. Requires an already enabled privilege;
  does not alter token/account policy. Successful large-page allocation is
  untested here; the failure/fallback path is tested.

Guest reads/writes, packed PTEs, permissions, atomics, lazy publication,
cross-page behavior and faults are unchanged. Allocation locking is only on
materialization. Region slots are never reused: a remap of a retired slot
gets a fresh ordinary page, retained until table destruction. This preserves
old pointers held by concurrent readers. All regions/remaps have owners.
Windows reserves at most 8 GiB virtual address space for first mappings of
the entire guest address space; commitment follows touched 4 KiB pages.
Remapping loses region locality for those pages. Sparse non-Windows fallback
uses zeroed aligned 2 MiB allocator blocks and has not been tested here.
VirtualAlloc allocations bypass Rust global-allocator diagnostics; compare
process commitment/working set when assessing memory. Loading/first-touch
overhead has not been benchmarked.

## Isolated measurements

Release CPU harness: two 32 MiB memories, one thread, 4,194,304 accesses per
sample, seven rounds with all four policy/prefetch variants rotated. The
page-walk uses a permutation of 8,192 pages and varying cache-line offsets;
it is not a dependent pointer-chain replay. Sequential stride is 64 bytes.
Software prefetch looks 16 accesses ahead with `_MM_HINT_T0`, including its
extra page-table lookup. Every sample produces the same checksum
17,177,772,032. Prefetch remains a benchmark experiment, not production code.

| Pattern | Prefetch | Pages ns/access | Regions ns/access | Median paired speedup | Faster pairs |
|---|---|---:|---:|---:|---:|
| Sequential read | off | 5.021 | 3.459 | 1.461x | 7/7 |
| Sequential read | on | 4.437 | 4.017 | 1.197x | 6/7 |
| Page walk | off | 7.614 | 7.570 | 1.027x | 5/7 |
| Page walk | on | 7.776 | 7.559 | 1.022x | 4/7 |
| Sequential read + write | off | 7.073 | 5.225 | 1.385x | 6/7 |
| Sequential read + write | on | 6.162 | 5.475 | 1.181x | 6/7 |

Median times and median paired ratios are different statistics. Shared-host
noise is material: an earlier shorter run regressed on scattered reads.
Prefetch helps the scattered-allocation sequential control but regresses
aligned sequential access in the final run; no general prefetch is justified.
No hardware cache or TLB miss rate can be inferred from these timings.

Also replayed the original E0E5E0 vectors and isolated lifted body using the
existing L158 harness, built only against this branch's base/mtcore. Four
tests pass: 528 original cases for lifted/port, 672 paired access/fault cases,
and page-table replay in serial/atomic modes. These are original-instruction
fixtures with synthetic input, **not a gameplay guest-memory snapshot**.
Separate leaf benchmark processes (pages then regions; not paired across
policies) give aligned lifted medians, ns/call:

| Bytes | Pages | Regions |
|---:|---:|---:|
| 16 | 60.578 | 57.807 |
| 64 | 176.922 | 181.827 |
| 256 | 643.894 | 670.392 |
| 1024 | 2745.161 | 2710.394 |
| 4096 | 10008.400 | 10225.600 |

No convincing improvement in this tiny working set; no hot whole-job speedup
or gameplay/FPS/image acceptance is claimed.

## Verification and reproduction

All cargo commands: `CARGO_TARGET_DIR=C:/ddda-targets/L168`,
`CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`.

```
DDNATIVE_BACKING=regions cargo test -p ddnative-base
DDNATIVE_BACKING=large cargo test -p ddnative-base --test mem --test locality
DDNATIVE_BACKING=regions cargo test -p ddruntime --no-default-features
cargo test -p ddnative-base --release --test locality -- --ignored --nocapture
```

- Base regions: **41 pass / zero failures / one ignored benchmark**.
- Large fallback focused: **five pass / zero failures / one ignored**.
- Runtime regions light suite: **250 reported pass / zero failures / seven
  ignored**. Runtime adapter, thread/Interlocked, memcpy, cross-page and
  source-vector integration tests included; no lifted shards.
- New tests include alignment, sparse lazy slots, zero-fill, wraparound,
  independent remaps, retained old bytes and concurrent publication.
- L158 isolated source is private at `C:/ddda-targets/L168/hash-replay`,
  adapted from `E:/ddda-re/notes/native-L158/isolated` solely by setting
  manifest dependency paths to this worktree. Run its manifest with
  `DDNATIVE_REQUIRE_VECTORS=1`, `DDNATIVE_BACKING=regions`, `--release`.
  No generated body or vector is committed.

Private logs in `C:/ddda-targets/L168`:

- `locality-final.log` SHA256
  `1e811739f4eca4c882b226686f1dcba636b25e5570b94d9223f7644e1efb5fbb`.
- `hash-regions.log` SHA256
  `5ce1781b8c2626faad996f72f057a5a7e53eee07fe7f0f78bd5b50ac43c5fbf9`.
- `runtime-regions.log` SHA256
  `44dd63ae3c82a807bea64c922f009050f52865e41d06279bcf9828cb262f6c79`.

## Head-owned in-situ measurement (pending)

`wpr.exe -pmcsources` exposes DcacheMisses, DcacheAccesses, CacheMisses,
InstructionRetired, among others; six selectable counters, no named DTLB
source. Generic CacheMisses is **not established as LLC misses** on this CPU.
Do not report it as such without verifying the PMU mapping. The profile
follows Microsoft's [PMU recording example](https://devblogs.microsoft.com/performance-diagnostics/recording-hardware-performance-pmu-events-with-complete-examples/).

`scripts/l168-pmc.wprp` validates with `wpr -profiles` and `-profiledetails`.
It requests four counters strictly, Loader/ProcessThread/PmcProfile,
128 x 1 MiB circular RAM buffers, no stacks. Capture start/stop has NOT been
executed here: head owns the next run after the user's playtest (#13563).
Start can require an elevated shell and hardware availability; syntax
validation alone does not prove counter collection works.

Head commands (check no unrelated recording first; never cancel one):

```
wpr.exe -status
wpr.exe -start E:/ddda-rs-wt/L168/scripts/l168-pmc.wprp!L168
# Head captures 20 seconds of stable gameplay, then:
wpr.exe -stop E:/ddda-re/notes/runs/L168-pages.etl
```

Repeat the same binary/route with `DDNATIVE_BACKING=regions` and output
`L168-regions.etl`. Omit `-filemode`: circular buffers bound collection to
128 MiB; reserve 512 MiB/output for merged metadata, which is not a strict
final-file bound. Head must stop after 20 seconds, including on run failure.
Match Present/frame counts and worker thread IDs; save binary/PDB hashes.

Analysis: select the same gameplay time range/process/guest threads; reject
event loss, missing sources or mismatched capture windows. Weighted count
estimate = overflow samples x source interval (miss events 65,536,
accesses/instructions 1,048,576); weighted DcacheMisses/DcacheAccesses is an
approximate event ratio, not an exact per-load architectural miss rate.
Attribute sampled IPs with the matching PDB, including accessor and caller
code, allowing interrupt skid. Compare count per frame as well as ratios.
Counters do not distinguish software page-table reads from guest-data loads.
No TLB miss-rate result is available without additional CPU-specific events.
Do a separate profiler-off matched FPS/critical-join/image run for acceptance.

## Results

Opt-in aligned backing is ready for the head's integration train; default
behavior is unchanged. Synthetic sequential gains are measured, random
access and isolated hot-leaf gains are not established. True large pages
are unavailable under the existing token. **Actual hot-job snapshot replay,
in-situ miss rates and gameplay acceptance remain pending head capture.**
Next action: head merges the opt-in and collects the matched PMC/game pair
above. No claim that this resolves the frame's 18 ms job-join waits.
