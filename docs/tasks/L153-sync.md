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
address and first argument. After each two-second reporter sleep stderr receives cumulative `sync-census` / `sync-row` / `sync-census-end` snapshots. Report formatting time adds to that interval.
Fields include calls, inclusive wall ns, maximum ns, primitive misses,
guest-service-lock contention, blocking attempts, timeouts and 16 duration
buckets (<1us, <2us, ..., <16384us, >=16384us). Successful D3D device or
swap-chain Present services supply the frame denominator. These are producer
Presents, not proof that an image was displayed.

Rows are registered once, then updated through per-thread cached atomic
counters. Part 2 collapses transient Interlocked operand addresses to arg0=0
while retaining service/site/thread totals and all other actual arguments.
The first/every thirtieth report is full; other reports set sparse=1 and emit
only changed cumulative rows. The parser carries unchanged rows from the last
complete baseline and rejects gaps in a sparse capture. This avoids repeatedly
printing startup-only rows. Adopt the updated parser with the new reporter. The process-wide limit is 16,384 rows; `overflow` counts omitted
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
  16-worker game pool on this host. L160's source audit (board 13071) locates
  selection in `0x00DBE95D..0x00DBE979`: participants = min(N, 8), minus one
  if that result is at least four; stores scheduler +0x98/+0x9c. Reporting
  8, 12 or 16 CPUs therefore still selects main plus six workers. L153 does
  not modify that selection. Job claiming already dynamically balances jobs;
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

### Part 1 handoff

Head integrated part 1 and captured combo26 with census enabled, first with
cache/futex off, then both on. The late-game analysis below supersedes the
initial pending-capture status. Profile-off matched image/FPS acceptance remains
outstanding. No agent game executable was built or run. The initial batch left
TLS/semaphores/general waits on the service lock; the part 2 follow-up below
removes the TLS and single-semaphore bottlenecks.


### Part 2: thread state and semaphores

The abandoned-owner correction announced in board message 12677 was already
included in `5a8c01bb`. Part 2 adds native Win32 wait-any/wait-all comparisons
and actual guest-thread exit coverage in serial and parallel modes. Recursive
ownership is reset on exit; the next acquisition reports abandonment once.
No game scheduling functions or worker counts are changed.

One const-initialized host TLS record now supplies the cached runtime owner
pointer, guest tid, service nesting and D3D thread key/held depth. Runtime pins
keep their pointer/depth in the const TLS record and save that record pointer
in the guard, avoiding heap-owner lookup or another TLS lookup on drop.
Their ordinary path performs no `RefCell` borrow or Arc/Rc retain; cold runtime
creation, explicit `current()` ownership and runtime replacement still retain
owners deliberately. Replaced runtimes remain alive until the final pin drops,
including reentry and unwinding. Scoped service/wait helpers use pins instead
of cloning the runtime Arc; spawning a host thread still clones ownership.

TLS values and last-error live in stable thread-owned boxes keyed by a unique
runtime identity and guest tid, with a cached pointer after first use. The
boxes live until host-thread exit (including values for sessions that thread
previously entered); they do not retain runtime Arcs. Cross-thread migration
of a guest thread is not supported by the existing one-host-thread-per-guest
model. Get/Set use a single atomic allocation-generation read and thread-owned
Cells. Allocate/free serialize only allocation metadata, changing the generation
so other threads see zero after reuse. Service-lock hand-offs no longer clone
TLS vectors or save/restore last-error. GetLastError/SetLastError and TLS Get/Set
are lock-free service bindings, retaining the existing host FPU boundary and
serial-mode yield points.

Microsoft's [TlsGetValue](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-tlsgetvalue)
and [TlsSetValue](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-tlssetvalue)
contracts plus the host Windows oracle verify zero-initialization and error
preservation/clearing. The host also accepts unallocated indices throughout
0..1088; part 2 corrects the inherited allocated-only validation. Invalid
indices fail with 87, failed free with 87, exhaustion with 8; the 1088-slot
runtime limit is unchanged. Values are guest u32s, never dereferenced.

Semaphore release and single-object wait no longer acquire the global service
lock. Counts use atomics for readiness plus a narrow per-runtime semaphore gate
for mutation. A general wait holds that gate across readiness and commit,
preventing competing single waits from consuming semaphore tokens during a
wait-all. Mixed waits recheck newly-ready semaphores as well as events after
sleeper registration, preserving wakeups when release occurs outside the service
lock. The gate is released before signalling or sleeping. Aliases share the
count object; CloseHandle removes the lookup entry. General mutex/thread waits
and object creation still use the service lock. Existing auto-event wait-all
rollback limitations described above remain; no FIFO/exhaustive kernel-object
exactness claim is added.

[ReleaseSemaphore](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-releasesemaphore)
requires a positive signed release count. The host oracle additionally verifies
87 for zero/negative count before handle validation, 298 for overflow, unchanged
previous-count output on failure and preserved last-error on success. This fixes
the old zero/negative-count error result while retaining semaphore limits.

#### Part 2 verification and timing

- Final full light gate: **229 passed / 1 inherited fullscreen-gamma failure /
  7 ignored**, including **84 library passes**. Present timing passes in this
  run. No checks were weakened. Nine census-parser tests pass.
- Final PROFILE/CACHE/FUTEX=1 library gate: **84 passed / 0 failed / 5 ignored**
  (14.57 s), including all eight callback sites, tail suppression, nested job
  unwind and MXCSR preservation. No game, GPU or shard builds/runs by this agent.
- Verified: actual import bindings complete while another
  thread keeps the service lock; 8-thread TLS/error isolation (8,000 rounds),
  runtime/tid switches, allocation reuse, invalid indices, 1088-slot exhaustion,
  aliases/close, limits/previous count, 4,000 tokens per semaphore under competing
  single/wait-all consumers, four-thread repeated allocation reuse, two
  semaphore ping-pong pairs, native Win32 TLS/abandonment, actual guest exit in
  serial and parallel modes, nested pin unwind and host-thread owner teardown.
- Identical `sync_thread_access_benchmark` source compiled in light `play`
  against parent `5a8c01bb` and the candidate implementation, no game feature or
  shards. Three alternating paired processes, five samples per process,
  50,000 pairs/thread/sample; numbers below are medians of process medians.

| Synthetic operation | Before ns | After ns | Ratio |
|---|---:|---:|---:|
| Runtime pin + service scope + D3D-held lookup | 29.881 | 31.805 | 0.940x |
| D3D DeviceLock acquire/drop | 26.170 | 27.863 | 0.939x |
| TLS set/get, one thread | 460.388 | 358.990 | 1.282x |
| TLS set/get, four threads, aggregate wall ns/pair | 2068.019 | 109.594 | 18.870x |
| Semaphore release + immediate poll, one thread | 806.076 | 478.260 | 1.685x |

The four-thread TLS case isolates global service serialization and context
copying; it is not a forecast of FPS. Final local bookkeeping and DeviceLock
medians regress about 6%; no speedup is established for those paths. Per-process
DeviceLock medians are 24.512/27.852/26.170 before and 27.372/46.649/27.863
after, showing shared-host variability. The structural removal of lazy-owner
TLS access and hot retains still needs a matched game profile. Earlier local
measurements were near neutral; final values above supersede interim timings.
Semaphore numbers include service dispatch, handle
lookup and wake scan, but no contended blocking. Original event cache/futex
options remain OFF by default. Whole-game correctness and FPS are unmeasured
for part 2; combo26 contains only part 1.

Final CPU test executable SHA-256 (not game executables): baseline
`03a5d8f68dd367403c6baef8ec0c7254e1a8fb848c73270bbe2e374b669c0072`, candidate
`37b2a36d14dbc20b6514b6e017c3fecb5d40e4025d0f42352cd8798a99408450`.
Logs, repeat samples and census interval JSON are preserved privately under
`E:/ddda-re/notes/L153-part2-20261013/`. Owned C: build targets and benchmark
binaries are removed after the handoff; no private artifacts are committed.


### Head combo26 evidence: late-game critical path

Head ran part 1 on FPS pin `f0019a20`, separately from the `c26vk` sampled
capture. `c26s0` uses PROFILE=1/CACHE=0/FUTEX=0; `c26s1` uses 1/1/1. These
are diagnostic runs, not profile-off performance/image acceptance.

`sync_profile.py --first 17 --last 22` isolates comparable late-game windows:

| Capture | Reporter ms interval | Producer frames | Overflow | Participation |
|---|---|---:|---:|---:|
| c26s0 | 62,787..98,027 | 633 | 0 | 7 |
| c26s1 | 63,034..97,020 | 628 | 0 | 7 |

The metadata is latest participation (main plus six joined workers); maximum
queued jobs 868 is cumulative, not a maximum proven for this interval. The
initialization still creates seven worker threads. The first ~5,400 frames
precede gameplay, so whole-run per-frame averages materially dilute gameplay.
Report rows are read sequentially; these are approximate interval boundaries,
not simultaneous frame traces. c26s0 has 23 complete snapshots plus a final
row truncated mid-histogram. Parser repair discards that unfinished snapshot
while still rejecting malformed completed snapshots; c26s1 has 24 complete.

| Main/render operation | c26s0 | c26s1 |
|---|---:|---:|
| Main scheduler join DBE66D, calls/frame | 69.254 | 69.322 |
| Join inclusive us/call | 269.216 | 256.955 |
| Join inclusive ms/frame | 18.644 | 17.813 |
| Render request wait DAC0B4, ms/frame | 14.306 | 12.585 |
| Main render-completion wait DAEE40, ms/frame | 0.262 | 0.795 |
| Main Sleep calls in this interval | 0 | 0 |

The render request handle is `0x00F0005C`: main SetEvent at return
`0x00DA9D78` feeds render WaitForSingleObject at `0x00DAC0B4`, then render
ResetEvent at `0x00DAC0D0`. Completion handle `0x00F00060`: render SetEvent
at `0x00DAC1A4` feeds main wait `0x00DAEE40`, with main reset `0x00DA9D6B`.
Both handshakes occur approximately once per producer frame. L157's source map
shows double buffering: render consumes the prior stream while main builds the
next. Therefore the request wait is an upstream gap; it is not an additional
serial cost to add to the main join. These handles are run-local observations,
not runtime constants.

The main join waits on six worker completions. Its median bucket improves
32..64 us -> 16..32 us, but p95 stays 1,024..2,048 us and p99 4,096..8,192 us.
Only 6.81% of candidate joins are at least 1,024 us; their histogram-bin lower
bounds already account for **at least 8.547 ms/frame, 47.99% of join time**.
The >=4,096-us tail is 1.286% of joins and at least 4.018 ms/frame. Baseline is
similar (7.32%, at least 8.889 ms/frame for >=1,024 us). This prioritizes the
long batch tail for measurement, rather than assuming all time is cheap polls.
The call duration includes service-lock acquisition/scheduler overhead and
instrumentation, not only sleeping for jobs. Lock contention occurs in 6.129%
/ 5.164% of join calls; it does not measure that contention's duration.

Census coverage defects found and fixed in part 2:

- c26 job rows capture only main `0x00DBD8D0`. Workers execute their own loop
  at `0x00DBDC90`; callback returns **0x00DBDD4C, 0x00DBDDBF, 0x00DBDE40,
  0x00DBDEAD** are now included, excluding preceding observers. Source:
  Steam executable's verified lifted control flow in `lift_hot_00/lifted_004`.
  Thus c26 cannot establish worker job-type distribution or which job causes
  a particular long join. An absence of worker rows is not idle-worker proof.
- Tail-dispatched lifted wrappers preserve the caller's stack/return slot;
  e.g. `0x00B54050` tail-jumps to `0x00B66110`. c26 counted both for one job.
  The next census suppresses redispatch at the same active `(ESP, return site)`,
  restores its marker on unwind, and retains genuine nested scheduler frames.
  Do not add arbitrary c26 target durations together.
- c26 reported ~15,000 cumulative rows, including thousands of startup-only
  critical-section rows; report spacing grew to ~7 seconds. Sparse cumulative
  reports plus Interlocked operand grouping reduce repeated diagnostic output.

Individual main job observations in c26s1 (inclusive; not summed): `0x00DB7620`
44.102 calls/frame x 242.138 us = 10.679 ms/frame; `0x004B9FF0` 69.503 x
74.520 us = 5.179 ms/frame; `0x01090F70` 59.782 x 65.797 us = 3.933 ms/frame.
Their p95 buckets are 512..1,024, 256..512, and 256..512 us respectively.
They are leads for L158, not permission to change scheduling or worker counts.
Worker start waits at `0x00DBDCBC` occur ~69.33 times/frame, ~414.8..529.1 us
per wait in s1, and mostly represent time outside that worker's work phase.

TLS GetValue occurs only 0.092/0.360 times per late-game frame; TlsSetValue and
ReleaseSemaphore have no calls in either interval. The part 2 import-level
microbench improvements therefore do **not** establish a gameplay FPS gain.
Host `runtime::pin`, D3D thread key and service bookkeeping are separate costs
not counted as guest TLS imports. Main Sleep(1..4) in cumulative reports came
from earlier phases, not this gameplay interval.

Raw sample identities from L154/L157 corroborate the sites but do not supply
numeric handles or a synchronized timeline. The c26vk sampler collected
669 rounds in 30.03 s (~22.28 Hz, near the game's frame rate) and samples
threads sequentially; phase/scan-order bias is possible. In particular,
NtAlertThreadByThreadIdEx is a wake syscall, not a blocked wait. Do not use
sample-share percentages as CPU percentages or scale them directly to ms/frame.
See `docs/tasks/L157-waitchain.md` and L154's raw-leaf report for host backend
and finisher dependencies that the Win32 census does not observe.

Private source logs: `E:/ddda-re/notes/runs/run_c26s0.log` SHA-256
`3146c97e4a512f01205314903001556d1dbd1b6335f1aab33801697cfb2c12e1`; c26s1
`73fbb3c37045e5eff233fbd2452548a3b73636a47d46627cb73752b536f1c787`.
Next: head adopts corrected worker/tail census and parser; L158 owns the separate
batch-correlated target/start/end trace (including last worker completion).
Repeat a late-game capture and correlate long joins with worker jobs. Keep the
original scheduler and event-cache/futex defaults unchanged until a matched
profile-off image/FPS run proves a benefit.
