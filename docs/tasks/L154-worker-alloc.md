# L154: render-worker allocation census and fixes

Assigned to codex-11, branch `task/L154-worker-alloc`, base `9c2c3592` on
`task/fps-line`. Runtime only; no game runs or shard builds by this agent.

## Scope and coordination

Rank render-worker host allocations/frees by caller, count and requested bytes,
remove avoidable temporary storage, and validate with light runtime, d3d9cmd,
and bounded ddgpu tests. claude-2 owns ndraw_native; claude-9 owns guest memory,
object table and D3D lock changes. Disjoint hunks require board agreement.

## Evidence

Private c24def text profile: worker 0x1034 has 128 busy samples, heap allocation
13.3% self and free 6.2%, Constants float-vector clone 1.6%, BindingPart drop
2.3% self / 8.6% inclusive. These are sampled time shares, **not** allocation
counts or byte ranks. No saved c24 stacks exist to recover callers.

The base already includes owned DrawCall transport and upload-buffer recycling
from L144/L147. Binding caches retain entries until 16,384 entries, so the
profile's cache-clear samples do not imply a clear on every frame.

## Source inventory (not a measured gameplay ranking)

| Construction site | Existing allocation events | Lifetime / next owner |
|---|---|---|
| `Shadow::prune` context deduplication | Temporary Vec grows at 4, 8, 16… unique addresses; 16 unique writes cost 3 allocation/realloc events, 224 requested bytes | Every nonempty context prune; in-place candidate prepared |
| `Mirror::textures` | Temporary Vec starts at 8 textures, grows if needed; conversion allocates immutable Arc slice | Whenever texture snapshot changes; fixed 21-slot candidate prepared |
| `stream_part` | Vec capacity 4, Arc slice, enclosing Arc StreamPart | Each distinct stream binding per frame; array candidate prepared |
| `binding_part` | Two float Vec clones (if nonempty), two Arc Constants, one Arc BindingPart on changed bindings | Exact equality already reuses the whole binding; independent stage reuse suggested to claude-2 |
| `binding_part` refusal | Scratch float Vecs dropped after `mem::take`, reallocated on subsequent attempts | Three uncovered-register exits; restoration candidate prepared |
| `flush_uploads` | Binding-Arc Vec taken and dropped at each flush | Capacity recycling suggested to claude-2 |
| `send_batch` | Replacement draw Vec capacity 64, later possible growth | Backend owns previous batch; requires coordinated recycling API |
| frame caches / statistics | Stream/target Arc drops every frame; binding clear above 16,384; Frame maps discarded by take | Retention needs identity/lifetime evidence; not changed |
| D3D9 snapshot fallback | Changed constant bank copy-on-write and Arc texture slice | Existing L144 sharing already present; object/lock work stays claude-9 |

New c25 default text profile has 267 busy worker samples: heap allocation
6.0% self, free 4.5%, `Shadow::prune` 5.6% inclusive, binding clear/drop
5.6% inclusive. The c25 zlib-fast variant instead shows allocation 12.8% and
free 3.7%. Both are text-only without heap-caller sections. Neither supplies
allocation counts or bytes; variation reinforces the need for the census.

## Diagnostic protocol

The unchanged System allocator now records successful allocation and free
call sites separately. Successful realloc is a free of its old layout and an
allocation of its full new request, not the size delta. Driver/OS/guest heap
allocation bypassing Rust's allocator is outside this census.

For the head's **diagnostic**, default-renderer gameplay run:

```
DDPLAY_ALLOC_LOG=1
DDPLAY_ALLOC_STACKS=1
DDRUNTIME_LOAD_LOG=1
DDPLAY_ALLOC_THREAD=guest thread 0x1034
DDPLAY_ALLOC_DELAY_MS=90000
DDPLAY_ALLOC_DURATION_MS=10000
```

Times are relative to census initialization near process startup; adjust delay
to the observed gameplay phase. The logger prints the selected window's system
uptime boundaries, every populated site once after the window ends, and exact
overflow calls/bytes. It keeps 65,536 slots with 16 PCs each. Bounded probe
failure is counted; do not call an overflowing table a complete census.
Thread filtering uses the exact Windows description (rechecked, so setting a
thread name after its first allocation is safe). It uses fixed buffers, OS
APIs and atomics inside the allocator; no symbol resolution or Rust allocation.

```
python scripts/alloc_sites.py <private-log> --exe <matching-exe> --sort calls --top 50
python scripts/alloc_sites.py <private-log> --exe <matching-exe> --sort bytes --top 50
python scripts/alloc_sites.py <private-log> --exe <matching-exe> --kind free --sort calls --top 50
```

`--frames N` divides counters by Presents during the selected window. This is
an interval average, not an exact single-frame trace; use matching frame-log
boundaries and disclose their resolution. Allocation stacks and thread-name
queries add overhead, so this run cannot establish FPS improvements. Request
a separate uninstrumented profile with `sample_profile.py --seconds 30 --hz
500 --depth 64 --threads "guest thread 0x1034" --top 50 --callers 30
--save-stacks <private-json> --symbols <matching-PDB-dir>`.

## Results

- Implemented timed, thread-filtered allocation/free census and count/byte
  ranking with optional per-frame normalization.
- Standalone allocator/parser: **3 tests pass**, including real global-allocator
  startup, aligned/realloc forwarding, separate free stacks, exact named-thread
  inclusion/exclusion, delayed capture, and one-shot reporting. No ddplay build.
- Baseline runtime light `--no-fail-fast`: **208 pass / 3 fail / 5 ignored**
  (excluding two successful nested child-test runs). Fullscreen gamma is the
  documented inherited failure; Present/interlocked timing thresholds both pass
  isolated reruns. Command transport **14 pass**, ddgpu CPU **26 pass**.
- GPU correctness sweep, resumed after the head's combo25 pause: **26 pass /
  2 ignored / 2 hard-timeout kills** across draw/cache/targets/upload_data.
  `depth_precision_matches_the_d3d9_driver` and
  `target_formats_match_the_d3d9_driver` each exceeded 60 seconds; neither is
  claimed verified. All tests ran one process at a time with a pause check;
  no active L154 build/GPU process remains.
- Prepared private candidates for context-write compaction, fixed-slot stream
  and texture staging, and binding scratch recovery; no ndraw source edits
  without ownership agreement. A standalone compaction prototype matches output
  and read order in **25,600 cases**. At 16 writes, **3 allocs / 224 requested
  bytes -> 0**; five-pair median **410 -> 225 ns** under shared machine load.
  This is prototype evidence, not an integrated runtime or gameplay measurement.
- The committed `scripts/bench_ndraw_alloc.py` extracts the actual prune method
  from a before revision and after source, replacing only its memory reader.
  Candidate source (still private pending ownership) matches **25,600 output /
  read-order cases**. Seven alternating pairs at 16 writes: **411.5 -> 149.4 ns**,
  **3 allocations / 224 bytes -> 0**. At 4/32/64 writes: 118.5/648.6/1018.3 ->
  44.9/265.2/550.7 ns. Shared-load local evidence, not a gameplay claim.
- Timed reporting now waits for in-flight accepted stack captures before its
  one-shot snapshot. Standalone allocator/parser tests remain **3 pass**.
- Isolated runtime candidate (own target directory; original dependency paths,
  workspace edition 2021, `--no-default-features --lib ndraw_native::tests`):
  **11 pass / 1 ignored**, including the four new regression tests. No source
  edits to the claimed repository file and no shards compiled. The first
  temporary manifest mistakenly used edition 2024; corrected to the repository's
  2021 before the successful test run.
- Allocation fixes and head gameplay evidence pending. Head ownership questions
  #12744 / #12831 explicitly list the four proposed ndraw hunks; file not edited.
  Blocker #12877 records the missing ownership decision and live census. The
  tested patch is `C:/ddda-targets/L154/ndraw_candidate.patch` until integrated.
- Next: head captures worker census; coordinate ndraw allocation hunks, rank
  measured sites, and gate targeted fixes. No FPS or image comparison claim.

## Head follow-up: c26 wait attribution (board #12885)

`profile_waits.py` reads saved stacks only and distinguishes wait syscalls,
wake syscalls, unresolved leaves and empty samples. It retains each nearest
lifted host return PC separately from the guest function identity. Numeric wait
handles and actual guest return addresses remain null in its JSON: neither was
captured. They must not be fabricated from a host stack address.

Private input `stacks_c26vk.json`, pid 41848, SHA-256
`745cf2d17b508d8d771da9ac87131d53eec6560101ab1dd2aaf0f57c25b62036`:
669 rounds / 30.030 seconds. Counts below are samples, **not durations**.

| Thread / mechanism | Samples | Guest return, where independently recovered |
|---|---:|---|
| Render 0x1034 request event | 323 | `00DAC0B4` |
| Render waiting for Present retirement, during upload | 7 | Executor outer return `00DAC13D`, not a direct guest wait call |
| Render blocked command-channel send | 2 | Same outer executor return |
| Render notification wake (not blocked wait) | 7 | Same outer executor return |
| Main residual job join | 512 | `00DBE66D` |
| Main graphics mutex during query GetData | 7 | `011482BA` |
| Main D3D object lock | 1 | Function `01128F40`; exact return not recovered |
| Jobs 1070/1074/1078/107C/1080/1084: request event | 320 / 361 / 375 / 394 / 420 / 442 | `00DBDCBC` |
| Thread 1038 Sleep(50) | 480 | `00DDDFEB` |
| d3d9-backend empty command receive | 397 | Host channel |
| d3d9-backend finisher barrier | 30 | 13 Present / 10 query_result / 6 upload / 1 draw |
| wgpu-finish empty batch receive | 494 | Host channel |
| wgpu-finish waiting for encode results | 107 | Host result channel |
| wgpu-encode-0 / -1 work receive | 578 / 593 | Host work channel |

The guest returns above were independently recovered by reading the matching
combo26 host binary's guest-return immediate before `x86::call_ext`. PDB line
lookup yielded no source lines. Bytes/disassembly remain private, never in this
repository. For example, the render wait reads render-object +0xD0; job workers
read +8. Completion notifications use render +0xD4 and worker +0xC respectively.
The L153 census independently maps these in **its separate runs** to request
handle `F0005C` and completion `F00060`; those numeric handles were not read from
the raw stack capture. L157 documents the two command streams and frame overlap.

The old profiler's `WAITS` tuple included `NtAlertThreadByThreadId`; this matches
notification syscalls such as `NtAlertThreadByThreadIdEx` in `SyncWaker::notify`.
Removed that entry. The new report keeps wake samples distinct and correctly
separates nested guest-lock checks from the surrounding event wait, plus Present
retirement receives from command sends. **7 wait-report + 9 sampler + 9 symbol
fixture tests pass**; all synthetic/offline, no game/GPU/shards.

Independent CPU deltas are main **16.328/30.030 s = 54.4%**, render
**16.609/30.030 s = 55.3%**. They are not the 22%/49% non-wait sample shares.
Sampling achieved **22.28 Hz**, close to gameplay FPS; sequential thread scans
can bias sampled state shares and may alias frame phases. There are no per-stack
timestamps or guest event transitions. These snapshots establish sites and
mechanisms, not a complete causal frame timeline or wait durations. Late-game
L153 measured join durations are the stronger time evidence; its worker callback
coverage gap must be repaired before identifying the longest worker jobs.

## c26 allocation caller samples

For render 0x1034, 23 stacks contain `__rust_alloc`: draw_call 8 (four float Vec
clones, four inlined allocations), Mirror::textures 4, Shadow::prune 3,
shadow_samplers 2, fallback texture snapshot 2, TargetPart 2, ConstantSnapshot 1,
stream_part 1. Nineteen contain `__rust_dealloc`: begin_frame 12, draw_call 3,
prune 2, textures 1, shadow_samplers 1. Three contain `__rust_realloc`: command,
draw_call and prune once each. This supports the prepared temporary-buffer
candidates but is **not an allocation-operation or byte census**. Exact live
counts/bytes and gameplay effects remain pending the head's instrumented run.
