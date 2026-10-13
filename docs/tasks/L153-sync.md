# L153: game-thread synchronization

Assigned to codex-10 by head, 2026-10-13. Branch `task/L153-sync`, base
FPS integration line. Runtime only; no shard builds or game/GPU runs.

## Brief

Measure Win32 synchronization calls, contention, latency and guest return
sites; measure the game-thread fan-out join and job distribution. Reduce
runtime synchronization overhead without changing guest scheduling logic.
Use the light runtime suite, primitive tests and before/after microbenchmarks.
Head owns game runs and final image/FPS acceptance.

## Implementation

`DDRUNTIME_SYNC_PROFILE=1` enables an independent census (default off).
Completed calls accumulate by guest thread, service target, guest return
address and first argument. Every two seconds stderr receives complete,
cumulative `sync-census` / `sync-row` / `sync-census-end` snapshots.
Fields include calls, inclusive wall ns, maximum ns, primitive misses,
guest-service-lock contention, blocking attempts, timeouts and 16 duration
buckets (<1us, <2us, ..., <16384us, >=16384us). Successful D3D device or
swap-chain Present services supply the frame denominator. These are producer
Presents, not proof that an image was displayed.

Rows are registered once, then updated through per-thread cached atomic
counters. The process-wide limit is 16,384 rows; `overflow` counts omitted
calls and must be checked before interpreting completeness. Reporting takes
the row-registry lock only to clone ownership, not during formatting/I/O.
Inactive/in-flight calls are not timed until they return. Snapshots are
approximate atomic observations, not stop-the-world frame boundaries.
Diagnostic overhead includes timer reads, counter updates and full host FPU
boundaries for normally integer-only services; profile-on FPS is not the
performance acceptance run.

Primitive contention means an actual failed event/mutex/semaphore readiness
check or foreign critical-section owner. Repeated misses count a call once;
blocking attempts may exceed one. TLS and Interlocked do not expose a
separate primitive-contention outcome; their zero miss count must not be
read as zero hardware/cache contention. `service_contended` is distinct.
Sleep(0) separates naturally by argument. SleepEx/APC behavior is unchanged.
SwitchToThread and WaitFor*Ex are not currently bound runtime services;
the census recognizes their names if a later binding is added.

`python scripts/sync_profile.py RUN.log --first 2 --last -1 --top 30`
subtracts complete snapshots and prints calls/frame, ns/call and top sites.
Omit `--first` for cumulative-from-start; `--json` includes histograms.
Truncated trailing snapshots are ignored. Use one process log per invocation.

`DDRUNTIME_SYNC_CACHE=1` enables a 64-entry per-host-thread event lookup
cache. Hits take no table lock or allocation. CloseHandle invalidates caches
through an epoch; distinct runtime IDs prevent address-reuse aliasing.
Handles are monotonic, negative results are not cached, and aliases share
the same EventCell. `0`/unset uses the original RwLock lookup.

`DDRUNTIME_SYNC_FUTEX=1` enables Windows WaitOnAddress for blocking single
event waits in parallel mode when the caller holds no guest lock. The
existing spin interval is unchanged. Per-event address-waiter accounting
avoids wake syscalls when no address waiter exists. Wait-all and serial or
nested locked waits retain the shared sleeper protocol, including mixed
event/semaphore waits. Both paths recheck atomics after registration, take
auto-reset events once and retain an absolute deadline across spurious wakes.
The candidate is opt-in; no game acceptance is claimed.

Default-path corrections: ready/polling single-object waits avoid temporary
vectors and clock reads; WaitForMultipleObjects uses a stack handle array
and rejects counts outside 1..64; terminating guest threads abandon owned
mutexes, and the next successful wait transfers ownership and reports
WAIT_ABANDONED (including wait-any index and wait-all semantics).

The Win32 contracts are documented in Microsoft's
[WaitForMultipleObjects](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitformultipleobjects),
[WaitOnAddress](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitonaddress)
and [WakeByAddressAll](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-wakebyaddressall).
This is not a claim that the entire inherited kernel-object model has been
proved Win32-exact. In particular, general wait-all still uses the inherited
auto-event take/rollback protocol while single-event waiters run independently;
atomic multi-object acquisition needs a separate audit. Do not infer a FIFO
guarantee for events or critical sections.

## Scheduler evidence

Read-only Ghidra decompile plus disassembly, Steam 2017-12-13 executable:

- `0x00DBE560` resets completion events, signals worker start events, calls
  `0x00DBD8D0` on the main participant, then waits for all active workers.
  The WaitForMultipleObjects return site is **0x00DBE66D**. Its latency is
  residual join time, not total fan-out time, since main has already run jobs.
- `0x00DBD8D0` claims queued jobs through InterlockedIncrement of scheduler
  `+0x90`. Four indirect job returns are **0x00DBD958, 0x00DBD9CB,
  0x00DBDA4C, 0x00DBDABC**. The census times these dispatched lifted job
  calls by target and guest thread, excluding the preceding optional observer
  calls. This hook covers direct lifted bindings reached through dispatch,
  not step-executor interior instruction entries. Duration histograms show type/worker imbalance, not CPU time.
  Job times include nested sync calls; do not add those totals together.
- `0x00DBDF30` creates **seven** worker threads and two auto-reset events per
  worker; the main participant is slot zero. Participation is scheduler
  `+0x98`, queued jobs `+0x8c`; the join census records observed participation
  and maximum job count. Initialization's seven workers is not evidence
  that every fan-out uses all seven.
- Runtime `GetSystemInfo` reports available host logical processors capped
  at **32** in parallel mode, **1** in serial mode. This does not imply a
  16-worker game pool on this host. CPU-count-to-participation selection has
  not been recovered; do not increase worker counts based on the profiler
  alone. Scheduler job claiming already dynamically balances queued jobs;
  a long indivisible job may still dominate the residual join.

The c23/c24 sampled inclusive stacks motivate this census; they cannot
establish per-call latency, per-frame counts or exact job durations.

## Results

- Initial light suite after census/cache/address-wait changes: **219 pass,
  inherited fullscreen-gamma failure, 6 ignored**. Final suite including
  abandonment/count-bound tests: **220 pass, 2 failures, 6 ignored**:
  inherited fullscreen gamma and the existing Present scheduling timing
  assertion. No checks were weakened. Focused final sync suite with all
  three env options on: **24 pass, 1 benchmark ignored** (14.93 s), including
  that timing test. It also passes alone with diagnostics/candidates off
  (0.46 s). Four Python census parser tests pass.
- Primitive coverage: cache close/alias/collision/session identity, manual
  and auto reset, timeout, 5,000 address ping-pongs, existing 4x5,000 guest
  event round trips and 3,000 competing-auto-reset deliveries, mixed
  multiwaits, recursive CS contention, semaphore polls, invalid handle/count
  errors, abandonment and recursive reacquisition. Diagnostic tests include
  job-site filtering, duration buckets, completed counters and guest MXCSR
  preservation (profile-on gate exercises the latter).
- Optimized `play` runtime, no game feature/shards, three repeated paired
  microbenchmark runs: single-thread lookup+set+take median before/after
  **19.907/16.450**, **23.916/21.853**, **24.039/22.578 ns**. One earlier
  sample regressed 23.872->38.444 ns; shared-host noise/code layout matters.
- Eight threads with separate events, sharing the lookup table: aggregate
  wall ns/op medians **67.704->9.229**, **61.118->10.831**,
  **69.398->11.266** (5.64–7.34x). This isolates table contention and is
  not end-to-end sync cost or game FPS.
- Four pairs x5,000 guest event round trips, three process runs each:
  flags cache/futex **0/0: 6.67,5.95,11.22 us**;
  **1/0: 11.18,10.62,9.06 us**;
  **1/1: 11.08,6.05,6.24 us**. Mixed/no stable broad gain; both candidates
  remain opt-in. The reported denominator is 5,000 concurrent rounds,
  not 20,000 sequential round trips. Spins can satisfy most exchanges before
  either blocking implementation executes.
- Private test/microbenchmark logs: `C:/ddda-targets/L153-*.log`,
  `L153-ping-*`, `L153-lookup-repeat*`; target `C:/ddda-targets/L153`.
  All cargo commands use CARGO_INCREMENTAL=0 and CARGO_BUILD_JOBS=4.

### Remaining / next action

Head: integrate the commit, build once in the FPS line, then capture the
same gameplay route with `DDRUNTIME_SYNC_PROFILE=1` (cache/futex off first).
Inspect `site=0xdbe66d`, scheduler-job duration histograms and service totals;
repeat diagnostic with cache/futex=1/1. Run profile-off matched 0/0 versus
1/0 versus 1/1 for image/FPS/loading acceptance. No agent game executable
was built or run. Actual game calls/frame, join time and per-job distribution
remain pending those runs. TLS/semaphores/general waits still take the
service lock; the census must establish their cost before a larger redesign.
