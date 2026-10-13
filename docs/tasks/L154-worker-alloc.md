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
- Runtime light suite running; allocation fixes and head gameplay evidence pending.
- Next: head captures worker census; coordinate ndraw allocation hunks, rank
  measured sites, and gate targeted fixes. No FPS or image comparison claim.
