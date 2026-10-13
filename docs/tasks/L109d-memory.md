# L109d: gameplay memory and save-load time

Assigned to codex-6 by head, board 11529 (2026-10-13), continuing L109c.
Branch `task/L109d-memory`, worktree `E:/ddda-rs-wt/L109`, base
`task/fps-line` at `941f5d2c`.

Read `AGENT-RULES.md`, `../NATIVE_PIPELINE.md` and `../NATIVE_RENDERER.md`.
Only head runs the game. Runtime/light checks use four build jobs and an
isolated target; no independent shard builds or GPU work during head runs.

## Assignment

1. Attribute the save-load process-private-memory jump (head observes
   2.6 -> 15.7 -> 17 GB in roughly ten seconds, then a plateau) and reduce
   it to a few GB without changing the game's output or resource lifetime
   semantics.
2. Then reduce save-load time (head's approximate menu 21 s -> gameplay
   55 s), preserving original behavior. Obtain matched head measurements.

Current scope is allocator/host-buffer diagnostics and evidence-backed
runtime fixes. Coordinate backend/transport hunks with their owners and
page-table changes with codex-11's L132b. Never commit captured game data.

## Results

### Inherited measurement infrastructure

L109c `e7099da7` adds `memory_totals` under `DDRUNTIME_LOAD_LOG=1`:
process private commitment/residency, live guest-backed bytes and cumulative
observed unmap churn. Weak diagnostic handles retain no guest session.
That commit is handed to head but was not in this branch's starting ref.

CPU-only measurement (`L109c-memory-bench.log`, private task target): each
8 MiB guest page allocation adds approximately 17.62 MB of process private
memory. Unmap removes live pages but retires allocations until the whole
page table drops. Three cycles retain 52.82 MB with zero live pages.
This proves an allocation overhead/retirement mechanism, **not** the
cause of the gameplay spike. Live-page telemetry alone is insufficient.

### Rust allocator ownership counters

`DDPLAY_ALLOC_LOG=1` enables a forwarding wrapper around the unchanged
System allocator. With `DDRUNTIME_LOAD_LOG=1`, `allocator_totals` reports
live/peak requested Rust bytes, live allocation count, cumulative requested
bytes and live allocations whose size/alignment are both 4096 bytes.
That last class includes guest backing pages, including retired ones;
it is a shape-based counter, not an exclusive type attribution.

The configuration is sampled without allocating, preserves Win32 last-error,
and remains fixed for the process. No Rust environment/formatting/locking
code runs inside the allocator. Successful alloc/zeroed/realloc and dealloc
forward the original pointer/layout to System. Counter snapshots are not
transactional across threads; realloc peak accounting observes the returned
ownership, not temporary allocator internals. Driver/OS allocations and
System allocator overhead are outside the requested-byte totals.

`scripts/test_alloc_log.py` compiles only this module: two allocation/switch
unit tests pass in both off/on modes, and a normal console fixture with the
actual global allocator passes both modes. It checks alignment, zeroing,
realloc contents and allocation/release accounting. No ddplay/game or shard
build/run was needed. Timeline parser checks remain part of the handoff.

Read-only backend audit: the deferred stream bounds commands (2048), not
upload bytes; pending image uploads retain owned buffers until first GPU
use or resource release. These are candidates, not measured causes. Backend
and transport diagnostic hunks are being coordinated with their owners.

Next action: head captures save load with both log switches. Compare the
rise in private commitment, Rust-owned bytes, aligned page bytes and live
guest pages before selecting the allocation path to change. Save-load
optimization remains behind memory attribution.
