# L160: job worker sizing and wake latency

Owner: codex-11. Branch: `task/L160-job-threads`.
Assigned by head/user 2026-10-13: recover CPU sizing and job divisibility;
add a CPU-count override if it enables the proposed larger worker pool;
measure submission-to-worker latency using L153/L158 telemetry. Runtime
only; no game execution or shard builds by this agent.

## Results: worker sizing

**More reported CPUs cannot increase the current default six active job
workers.** This is an original-game limit, not a runtime CPU under-report.
Read-only original instruction disassembly and current lifted source agree:

| Source | Meaning |
|---|---|
| `00DBD235`, IAT `0139D248` | `GetSystemInfo`; `00DBD23B` reads SYSTEM_INFO+20 and `00DBD241` stores it at sMain+`1C040` |
| `00DBE95D..00DBE979` | Initial participants = min(logical CPUs, 8), then subtract one if the result is >=4 |
| `00DBE979..00DBE98C` | Save initial participation at sMain+`98` and configured participation at +`9C` |
| `00DBDF30` | Lazy foreground pool creation: slots 1..7, two auto-reset events each, `CreateThread(00DBDEF0, CREATE_SUSPENDED)`, ideal-processor hint equals slot index, then resume/initialization handshake |
| `00DBDEF0` / `00DBDC90` | Worker entry trampoline / dispatch loop; slot zero is main, not another host worker |
| `00DBE560` | Wake active slots 1..<participation, run main callbacks, join completion events; join return `00DBE66D` |
| `00DBFA90` | Copy configured +`9C` to active +`98` at frame start |
| `00DBD620` | Existing property setter clamps requested participation to 1..min(CPU count,8); writes +`9C` |

Initial CPU-count mapping (participants include main):

| Reported CPUs | Participants | Active foreground workers |
|---:|---:|---:|
| 1 | 1 | 0 |
| 2 | 2 | 1 |
| 3 or 4 | 3 | 2 |
| 6 | 5 | 4 |
| 7 | 6 | 5 |
| 8, 12, 16, 32 | 7 | 6 |

The original property names at `0142A8E4`/`0142A99C` are `JobThread` and
`mJobThreadNum`. The original setter could select eight participants
(seven workers) within its own limit, but its availability to a shipping
PC configuration/user is **not established**. No guest-memory patch or
setter interception is included. Raising the cap is not equivalent to
running the original on a machine with more cores: both worker arrays
are 8 x 0x20 bytes, and several loops have fixed eight-slot bounds.

There is also a **different background pool**: `00DBDFE0` creates
min(CPU count,8) threads starting at `00DBDF10`, dispatching through
`00DBDB80`, with priority -1 requested and a semaphore-backed FIFO.
`00DBDAE0` submits to this pool. Do not confuse its eight threads with
foreground event-driven workers. Changing CPU availability below eight
would change this pool too.

### Current runtime answers

`win32::get_system_info` reports host `available_parallelism().min(32)`
in parallel mode, one in serial mode. The active mask is `(1<<N)-1`.
On this 16-thread host the normal parallel answer is 16 / `0xFFFF`.
A read-only host probe on 2026-10-13 returned process and system affinity
`0xFFFF` / `0xFFFF`. This is a probe of this agent process, not a measurement
of the separately launched game process's affinity.

The original import table contains `GetSystemInfo` and
`SetThreadIdealProcessor`; no `GetActiveProcessorCount`,
`GetProcessAffinityMask`, or `SetThreadAffinityMask` import. The latter
APIs also have no runtime binding. Ideal-processor calls currently return
zero without forwarding a host affinity/ideal-processor change.
`SetThreadPriority` similarly succeeds without changing host priority.
These are inherited modeling limitations, not changes in L160.

`machine.rs` returns recorded CPUID topology (16 package logical CPUs,
including leaf 1 EBX and AMD extended leaf `80000008`). `00DBD170` uses
these to derive the separate +`1C03C` field; scheduler initialization uses
the OS count +`1C040`. Registry CPU 0 supplies name/MHz, not worker count.
Older comments saying the runtime always reports one CPU are stale for
parallel mode.

**No `DDRUNTIME_CPU_COUNT` switch is implemented in this checkpoint.** The
requested 6-vs-8/12/16-worker A/B is impossible through that switch. Its
only foreground effect here would be a downward participation experiment;
that is not evidence for the proposed larger-pool speedup. Head was
informed on board #13071/#13080/#13093 before this decision. If a separate
down-count experiment is wanted, it needs an explicit CPU availability vs
physical CPUID topology contract, and accounting for the background pool.

### How work is divided

`00DBD8D0` and `00DBDC90` claim the next queue entry with
InterlockedIncrement(+`90`) until +`8C` is exhausted. Entries are 8 or 12
bytes at +`C0`, selected by dispatch mode +`94`. Callback ABIs optionally
include the participant index and/or the queued parameter. This is dynamic
queue claiming, not a fixed equal slice for every worker. One queued
callback is not further split by this dispatcher.

Producer examples from the original instructions:

- `004152C0..004152F7` queues **one callback per participant**, target
  `004151E0`. That callback repeatedly claims a shared decrementing item
  index and invokes an item's vtable+`14` method. This batch has a second
  level of dynamic distribution within its callbacks.
- `00F79610..00F7963A` queues target `011BA470` for every linked-list node
  at owner+`34` (next at node+`30`), independent of participant count.
- `0116BC10` queues target `01169270` for non-null entries of its producer
  arrays, including its original first-array index bound of 25.
- `00E68390` queues target `0111E180` with per-item records; it does not
  derive a global worker-sized chunk count at those enqueue sites.

There is therefore no universal fixed chunk count. Some batches have ample
work: the existing L157 c26s1 aggregate records max_jobs=868 and
participants=7. That cumulative maximum does **not** establish available
parallelism in the late critical batches. L158's per-batch queue count,
last callback, and tail attribution are required before claiming benefit
from more participants. Long indivisible callbacks and producer dependencies
can still dominate; aggregate 34-51% worker busy percentages do not settle it.

## Wake measurement

L153's existing per-service durations cannot measure wake latency: worker
WaitForSingleObject duration includes time before any work was submitted.
The c26s1 late waits are approximately 415-529 us/call and 69.33 calls/frame
per worker; these are **not** submission-to-wake figures. The main's
17.813 ms/frame residual joins likewise cannot be attributed to kernel wakes.

L158 `973dbf3f` adds coordinated `Wake` records (board #13099):
SetEvent entry/return at guest return `00DBE603`, successful worker wait
entry/return at `00DBDCBC`, handle, thread, entry batch and return batch.
L160's `scripts/job_wake.py` consumes the raw `critical version=1` file:

```
python scripts/job_wake.py E:/ddda-re/notes/runs/L158.trace --min-frame 1000
```

It joins signal and successful wait by handle and the **return** batch,
since a wait can start in the preceding batch. Cross-thread record emission
order is irrelevant. The signal-return-to-wait delta is signed: a worker
may wake before SetEvent returns. It reports signal-entry-to-wait-return,
signal service duration, wait-return-to-first-callback, and total
signal-entry-to-first-callback; empty workers are retained without inventing
a callback time. Summaries include mean/p50/p95/p99/max and count >20 us,
plus per-thread signal-to-wait distributions and input SHA-256.

Only complete, nonnested, nonpanicked, zero-cumulative-loss batches are
accepted; missing/duplicate handle pairs, missing callback records,
unsuccessful operations and impossible ordering are rejected. Truncated
lines or concatenated process traces fail explicitly. These are instrumented
runtime-to-runtime elapsed times, **not pure kernel wake latency**. Do not
sum overlapping workers' delays as frame time or infer FPS from profile-on
captures.

### Source audit of likely latency contributors

- Default single-event wait spins for up to 20 us (`CS_SPIN`), then
  `wait_until` allocates key storage and an Arc/Condvar, registers under the
  shared runtime mutex, blocks, and reacquires that mutex on wake.
  `signal_key` scans registered sleepers under that same mutex.
- `DDRUNTIME_SYNC_FUTEX=1` uses WaitOnAddress for eligible single-event
  waits, retaining the initial spin. `DDRUNTIME_SYNC_CACHE=1` avoids repeated
  event-table lookup locking. L153 owns these candidates and later fixes.
- `ddplay/src/main.rs` unconditionally calls `host_timer::begin(1)` before
  constructing its window/event loop. `user32::time_begin_period` also
  forwards guest requests. Thus the 1 ms request is present in the process
  startup path; the wrapper ignores its host return code, so actual success
  and effective timer resolution are **not measured** here.
- Foreground job waits use INFINITE and event notification, not Sleep(1)
  polling. Timer granularity alone does not explain a signal-to-wake delay.
  Host scheduling, mutex contention and lifted dispatch after the wait must
  be separated by measurement. No >20 us finding or causal fix is claimed.

## Verification and handoff

- Seven offline parser tests pass: out-of-order emission, old-batch wait,
  wake-before-signal-return, no-work worker, missing/duplicate records,
  loss/failure/order guards, frame selection, malformed/multiple traces.
- No Rust runtime behavior changed; no cargo, GPU, game or shard build/run.
  No original logic was translated or patched. Only source/disassembly
  evidence and offline diagnostic tooling are delivered.
- Private analysis exe SHA-256:
  `a1831a438685c50ca530bbb91dc4cbdc36449ffa7d9183e345a8a106782286b5`.
  Current lifted evidence: `crates/ddlift/lift_27/src/generated/lifted_024.rs`.
  L157 c26s1-late.json SHA-256:
  `c5f6e7ef85243950d5918439c432635cd5ce629659bb0f4d75d77832e1ce15b9`.
  Raw inputs remain private; no game data/decompiler dumps are committed.
- **Pending:** actual signal-to-worker numbers require head's game capture;
  old aggregate logs cannot recover them. Run request posted #13126 after
  L158 owner request #13099. No performance/image acceptance claim.
- **Next action (head):** integrate L158 trace with current L153; same route,
  30 seconds late gameplay, `DDRUNTIME_SYNC_PROFILE=1` and
  `DDRUNTIME_CRITICAL_JOBS=<private path>`, baseline cache/futex=0/0 then
  1/1. Record executable hash and all env, analyze the paired traces, then
  profile-off FPS/image A/B if the correlated latency shows a bottleneck.
