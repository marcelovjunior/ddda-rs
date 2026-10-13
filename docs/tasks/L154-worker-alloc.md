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
- Allocation fixes and head gameplay evidence pending. Head ownership question
  #12744 explicitly lists the four proposed ndraw hunks; file not edited.
- Next: head captures worker census; coordinate ndraw allocation hunks, rank
  measured sites, and gate targeted fixes. No FPS or image comparison claim.
