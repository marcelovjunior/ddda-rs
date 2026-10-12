# L42: parallel guest threads (lifting the one-thread-at-a-time limit)

- **Owner:** claude-4
- **Branch:** `task/L42-parallel-threads` (worktree `E:\ddda-rs-wt\L42`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md), merge-train rule

## Why

`crates/ddruntime/src/threads.rs` runs one guest thread at a time (the
guest lock). Threads hand over only in services: waits, contended
critical sections, Sleep, and API calls after a 2 ms slice. That model is
exact and simple, and it was the right first step. It also means the
game's worker threads never run in parallel. sMain's job threads
(`mJobThreadNum`; FRAME.md section 3: units on a line run as jobs), the
renderer worker 0x00DAC090 and the resource loaders all take turns on one
core. At gameplay that caps the frame rate, which the player cares about.

The lifted code's synchronisation is all imported calls (Interlocked*,
critical sections, events, semaphores; docs/RUNTIME.md, threads row).
Guest memory is shared. So true parallelism may be possible without
changing lifted code, if the memory model and the runtime's own state
allow it.

## Task

1. **Analysis** (Results):
   - Which runtime state is touched from guest threads (services, the
     registry, GuestMemory's page tables, lazy mapping, the D3D9 layer,
     audio) and how it is locked.
   - What x86's memory model (TSO) requires of the lifted code's plain
     loads/stores when it runs on the host. Rust compiles them as
     non-atomic accesses through `Mem`. Check whether x86-64 hardware
     gives TSO for them as emitted, and where compiler reordering could
     break it. The game's lock-free patterns, if any, decide this: find
     any `lock`-prefixed instructions and the lifter's translation of
     `xchg`, `cmpxchg` and `xadd`.
   - Which threads would benefit, and how much (job threads per frame).
2. **Design:** parallel guest threads, with a fallback to the current
   exact mode by a switch. Keep the exactness arguments explicit.
3. **Prototype:** behind the switch; tests: the existing thread stress
   test in both modes, plus new tests that run two guest threads
   concurrently on Interlocked counters and critical sections.
4. **Measure:** the static initializers and a synthetic job workload,
   serial vs parallel.

## Results

### Summary

A switchable parallel mode is prototyped and tested. Set
`DDRUNTIME_PARALLEL=1`, or call `threads::set_parallel(true)` before
threads start; it is off by default, and the serial mode is the exact
model as before. In parallel mode translated code of different guest
threads runs at the same time, while every service stays serialised
under the guest lock. On a synthetic job workload with 7 workers it is
5.3x faster than serial. The branch builds on claude-9's L25 page-table
prototype (`task/L25-perf-prototypes`, train 2), as agreed with them.

### 1. Analysis

**The game's own synchronisation.** Every lifted basic block was decoded
from its start to its terminator (530,128 blocks from the generated
shards, 3,446,726 instructions):

- no `lock`-prefixed instruction, no `cmpxchg`, `cmpxchg8b` or `xadd`;
- three `xchg` with a memory operand, all on the thread's own stack
  (`0x012E1F99`, `0x012E1FCC`: `[ebp-4]`; `0x012EFA52`: `[esp]`, CRT and
  xapobase SEH frames), so there are no cross-thread atomics in
  translated code (the lifter emits xchg as load and store, which is
  exact there);
- four `sfence` (`0x00E77191`, `0x011A7616`, `0x011A7BDE`, `0x012C4D5F`,
  after non-temporal stores), which the lifter emits as a real host
  `sfence` in inline asm, also a compiler barrier; `movnt*` are lifted as
  ordinary stores, which is stronger than x86;
- no `mfence`, `lfence` or `pause`.

So all cross-thread synchronisation goes through imported calls:
Interlocked*, critical sections, events, semaphores, mutexes, waits. A
first linear sweep of `.text` found 142 candidate locked/xchg sites;
apart from the three above, every one was data decoded as code.

**Runtime state touched from guest threads.** All of it sits in
`Runtime`'s `GuestCell`s (services state, kernel objects, files, Steam,
user32, D3D9, audio, input, movies). It is reached only from services,
and those are entered only through `Registry::call` for a
`Binding::Service`. The registry is immutable (an `Arc`) and the call
target cache (`call_lifted`) is per host thread. Translated code touches
only its `Cpu` (per thread) and guest memory. So "services hold the lock,
translated code does not" covers every runtime state with one boundary.

**Guest memory.** The page table was a plain `Option<Box<page>>` array
behind an `UnsafeCell`. With concurrent threads, materialising a lazy page
races (two allocations, one thread's writes lost), and every plain access
is a data race under the Rust model even though x86-64 hardware is TSO.

**The memory model.** x86's TSO, for single accesses, is exactly
acquire loads and release stores: loads are not reordered with loads,
stores not with stores, and a store may pass a later load in both. On
x86-64 `AtomicU*::load(Acquire)` and `store(Release)` compile to plain
`mov`s; they cost only the compiler's freedom to merge or move them.
Locked instructions are full barriers, i.e. SeqCst. Unaligned accesses
are atomic on x86 only within a cache line; the prototype splits them
into byte atomics. The game uses Interlocked* (which Windows requires to
be aligned) for shared counters, and no unaligned shared access is known.

**Which threads would benefit** (thread creation sites: the
`CreateThread` IAT slot `0x0139D0B0`, 11 references):

- sMain's job threads: `0x00DBDF30` always creates 7 (ideal processors
  1–7, start `0x00DBDEF0`). `setJobThreadNum` (`0x00DBD620`) clamps the
  number used to 1..min(processors, 8), with the processor count from
  GetSystemInfo (`0x00DBD235` stores it at sMain+0x1C040). The original
  machine reported 7 (ORACLE live read). The serial mode reports 1
  processor, so the game uses a single job thread there.
- The resource system (ctor `0x00DBB690`): one loader thread
  (`0x00DBA9A0`) plus min(processors − 1, 3) resource workers
  (`0x00DB9F40`): 1 in serial mode, 3 in parallel.
- The renderer worker `0x00DAC090` (sMain ctor `0x00DB1A50`), sound
  (L16/L19), and the other sites (`0x0041B400`, `0x0041FD70`,
  `0x00CFE700`, `0x00CFE860`, `0x011A4610`, `0x00DBDFE0`).

FRAME.md section 3 runs the units of each line as jobs, so per frame up to
8 job threads plus the renderer worker and 3 loaders could run at once,
against one core today.

### 2. Design (what the prototype implements)

- **One boundary**: `Registry::call` takes the guest lock around a
  service (`threads::service()`, a guard), re-entrantly, because a
  service may call guest code (window procedures, APCs, XAudio2
  callbacks, guest XAPOs) that reaches another service. The guest code
  inside such a callback runs serialised, as before. Waits, sleeps and
  yields hand the lock over exactly as in the serial mode, keeping the
  nesting depth.
- **Per-thread state** (last error, TLS) is swapped only when a thread
  takes the lock from another thread, not at every hand-over, in both
  modes.
- **Threads**: in parallel mode a guest thread starts and runs
  translated code without the lock, and takes it to record its exit or
  fault. `Session::call` runs guest code without the lock
  (`threads::without_lock`).
- **Memory** (`ddnative-base` `PageTable`, shared with the lifted code's
  inline fast path): atomic page pointers to 4 KiB-aligned zeroed
  allocations, published once by compare-exchange (the loser frees its
  allocation); a page taken away is retired, not freed, until the table
  is dropped; atomic permission bytes; data accesses as acquire loads and
  release stores once any runtime in the process runs parallel
  (`mem::ATOMIC_ACCESSES`), and plain copies before that. The plain
  copies are exact for one thread at a time, ordered by the lock
  hand-over.
- **Interlocked\***: host atomics on the guest word (`fetch_add`, `swap`,
  `compare_exchange`, SeqCst) through `<dyn Mem>::atomic_u32`; an
  unaligned or faulting operand takes the ordinary accessors.
- **GetSystemInfo**: the host's logical processors (at most 32) in
  parallel mode, so the game sizes its pools as it would on that machine;
  one in serial mode.

Exactness, explicitly:

- Exact as before: all runtime state, every service's semantics, and the
  order of service effects (still one at a time).
- Now as on a multi-core x86: translated code interleaves at instruction
  granularity, with TSO order per access. The game's synchronisation is
  all services, which stay atomic.
- Not modelled in parallel mode:
  - SuspendThread stops a thread at its next service call, not at once
    (noted in the code).
  - Racing unaligned accesses may tear per byte, where hardware tears
    only across cache lines.
  - Which thread runs when is up to the host scheduler (as on Windows),
    so runs are not reproducible. The serial mode remains for
    reproducible and differential work.

### 3. Prototype and tests

`crates/ddruntime/tests/parallel.rs`, light (`--no-default-features`):

- InterlockedIncrement under 4 concurrent guest threads (80,000
  increments, none lost), in both modes;
- a critical section around a read-modify-write spread over translated
  code (8,000 increments, none lost; unowned at the end), in both modes;
- an overlap detector: translated code of two threads was never inside
  at once in serial mode (max 1), and was in parallel mode (max ≥ 2 on a
  multi-core host);
- compare-exchange (match, mismatch, unaligned fallback, lazy page);
- the processor count in both modes.

Every call goes through the registry as translated code does. The
existing stress test (`runtime.rs`
`guest_threads_share_memory_and_synchronise`) calls services directly,
so it stays a serial-mode test. The whole light suite passes. The real-
time audio tests are flaky under machine load on the base as well; run
directly, both trees pass 6 of 6. The lifter's codegen fixture
(`test_lift_codegen.py`) is updated to the atomic table (all 6 pass).

### 4. Measurements

Synthetic job workload (`examples/parallel_jobs.rs`, release): 7
workers, 64 jobs per frame of 40,000 guest accesses, a semaphore and an
event per frame, InterlockedIncrement per job. The machine was shared
with other agents (16 host threads):

| | ms/frame |
|---|---:|
| base (before L42), serial | 9.35–9.59 |
| L42, serial | 10.18–10.65 |
| L42, parallel | 1.81–2.10 |

(Interleaved runs of the final code, release.) Parallel is 5.3x faster
than serial. The serial mode pays about 9% on this access-only workload:
the permission and page loads are atomic now, which LLVM cannot merge,
plus a flag test per access. Three fixes brought it down from 38%:
inlining the page lookup on stores, plain copies until a parallel
runtime exists, and swapping the per-thread state (last error, TLS) only
when a different thread takes the lock instead of at every hand-over.
That last fix also took parallel mode from 2.8 to 1.9 ms per frame,
since every service call there takes the lock. Micro (`perf_micro`, release, noisy):
inline fast read 0.7–0.85 → 1.5–1.7 ns, trait read 1.6–2.0 → 2.9–3.6 ns,
fast write 1.2–1.7 → 1.3–2.2 ns.

Static initializers (real lifted code, debug build): not measured. The
run was queued in the heavy slot and cancelled when the session was wound
down (2026-10-12). The script is in the private notes
(`E:\ddda-re
otes\L42\measure_init.sh`: base vs serial vs parallel, three
runs each).

### State at hand-off (2026-10-12)

- Committed on `task/L42-parallel-threads`; head's plan: train 3, after
  L25 (train 2), L21 and L27 settle.
- **Required before train 3 (head):** the serial mode must cost about
  nothing; today it is about 9% slower on the access-only job workload.
  Not started. Plan:
  - choose the access path once instead of per access: a flag read at
    function entry next to `pt` (claude-9's lowering already reads
    `m.page_table()` once per function), or two `Mem`/accessor variants
    selected at session start, so serial-mode code is the pre-L42 plain
    path with no atomic loads of perms/pages and no per-access flag;
  - measure serial against pre-L42 with ddperf's replay and init
    workloads, and the static initializers (script above).
- Merge notes, also sent to the owners:
  - codex's L27 TEB set-up in `run_thread` must run inside
    `threads::enter`/`leave` in parallel mode;
  - L27b's `dispatch.rs`/`mem.rs` edits must keep the service guard in
    `Registry::call` and the atomic `PageTable` (board #9480);
  - L21's VirtualFree release must use `PageTable::unback` (board #9476).

### Left open

- The initializer timing (one heavy run queued).
- A real-game parallel run (title, then a gameplay route), comparing
  frame times, faults and differential state against the serial mode.
- L21 (claude-2) adds VirtualFree decommit/release. In parallel mode it
  must use `PageTable::unback`, which retires the page instead of freeing
  it, because another thread may still read it.
- If the serial mode's 8% matters, the access functions could be split
  into two monomorphised variants chosen once per function, instead of
  testing a flag per access.
