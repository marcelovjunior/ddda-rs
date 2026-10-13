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

### Emulator allocation fix

Head's `run_c18mem.log` / `c18mem_private.log` show process private memory
reaching 17.05 GiB, while Rust live requested bytes stay around 0.64 GiB
(peak 765,697,504 bytes); aligned guest-page allocations reach 422,445,056
bytes. This excludes live Rust buffers as the dominant owner. Process/guest
`memory_totals` were not present in this run; the external process sampler
supplies private commitment.

Pinned Unicorn 2.0.1's `qemu/accel/tcg/translate-all.c` uses a 1 GiB default
translation buffer on 64-bit hosts, allocated with Windows
`MEM_RESERVE | MEM_COMMIT`. Runtime retains an engine per thread/nesting
level. Its Rust hooks also hold strong `Rc` references back to the engine,
preventing destruction after the external owner drops. Both mechanisms
were reproduced without guest/game execution:

| CPU-only two-engine probe | 2.0.1 | 2.1.1, 32 MiB cache |
|---|---:|---:|
| Private-byte increase, both alive | 2,153,340,928 | 10,113,024 |
| Increase after dropping both | 2,153,340,928 | 561,152 |

The runtime now pins upstream **2.1.1**, builds only x86, and sets each
translation cache to **32 MiB before CPU initialization**, matching the
existing vector harness's host-cache policy. This is not a game constant.
Unicorn regenerates evicted translations. The newer Windows allocator
commits code pages on demand and Rust hooks use weak ownership. Thread-local
pooling and independent nested engines remain intact. No game instruction,
guest allocation lifetime, renderer setting or original boot hold changes.
`emu_engine_created` load-log events expose cumulative engine creation and
the configured per-engine cache budget (not a live committed-byte gauge).

Six emulator integration tests pass before and after: register/stack/FS
call-outs, repeated reuse, hot loops, 100,000 short calls, x87/SSE state and
new nested fallback coverage. The isolated memory probe passes; a unit
test reads back the actual cache budget. Single unpaired short-call times
were 2.005 s old / 2.333 s new on the shared host; no CPU speedup is claimed.
Logs are under `C:/ddda-targets/L109d-emu-*`; no shards or game were run.

Full serial `cargo test -p ddruntime --no-default-features --features emu
--no-fail-fast -- --test-threads=1`: **194 passed, one inherited fullscreen
gamma failure, four ignored** (`L109d-light.log`). The gamma Reset result
is the same pre-existing 2289436780-versus-0 failure; no check was changed.

2.1.1 builds using the existing toolchain. Later upstream packaging adds
an unconditional symlink step that fails on this machine; no registry
source patch or elevated privilege is required for the chosen pin.

Head's c18 combined build records guest start **0.751034 s**, first Present
**1.743526 s**, and exact geometry call **70.703 ms**. This improves the
earlier combined boot boundary (5.84–5.86 s), but is not an isolated A/B.
Title/main-menu and post-fix save-load/gameplay timings remain unmeasured.

Next action: head integrates this change and captures the same save load
with `DDPLAY_ALLOC_LOG=1 DDRUNTIME_LOAD_LOG=1` and the external private-byte
sampler, then compares image, process memory and visible phase timings.
Codex-10's separate `2a5ad811` removes six hot fallback duration readers;
remaining save-load CPU work should be profiled after both changes land.
