# L157: guest frame wait chain

Owner: codex-7. Branch: `task/L157-waitchain`, based on `task/fps-line`.
Scope: read-only guest/runtime analysis and this report; no game runs,
shard builds, or runtime behavior changes.

## Results

Late-game c26s0/c26s1 counters put main's residual job joins at
**18.644/17.813 ms per frame across 69.254/69.322 joins**. Main's late
render-completion wait is only **0.262/0.795 ms/frame**. Optimize/measure
the repeated job waves first; removing render handoff waits is not supported.
These are instrumented non-Vulkan runs, not an A/B acceptance or a measured
breakdown of c26vk's approximately 45 ms frame.

The frame is already pipelined. Main produces a new command stream while
the render worker executes the previous stream. In the c26vk sample,
**512/669 main-thread samples are residual job joins**, whereas no sample
shows main blocked on the render-completion event. The render worker is
waiting for its next request in **323/669** samples; the backend is waiting
for commands in **397/669**. These idle periods are consequences of upstream
dependencies, not independently recoverable frame time.

This is a source-backed dependency map and sampled attribution, not a
timestamped per-frame trace or an original-game comparison. Do not remove
the joins, combine unit lines, increase participating workers, or change
frame buffering on the strength of these counts.

## Source map (Steam 2017-12-13 guest addresses)

The stack order in the brief is leaf-to-root. Actual call direction is
`004079E0 -> 00DF0D20 -> 0041DD60`: entry calls the application loop,
which pumps messages/checks the device and calls the frame virtual method.
`00DF0D20` sleeps 1 ms on inactive/lost-device paths; it is not a mandatory
per-gameplay-frame sleep. The normal frame clock/cap is `00DBFA90`.

The ordering inside `0041DD60` is established by the private L108 dump,
the lifted bodies, and [FRAME.md](../FRAME.md): clock and pre-frame work;
render kick; game state/input/AI/unit lines/collision/animation and other
systems; render production/sort; render completion/Present; tail work.
Repeated fan-outs are barriers between individual systems/lines, not one
large once-per-frame fan-out. `004B97F0` is the unit-line move pass.

### Render handoff

Let `R = *(u32*)0x018D08F4` (sRender); numeric runtime handle values are
capture-dependent, distinct from these guest field addresses.

| Operation | Guest function / return site | Object |
|---|---|---|
| Early main reset completion | `00DA9CD0 / 00DA9D6B` | handle at `R+0xD4` |
| Early main signal request | `00DA9CD0 / 00DA9D78` | handle at `R+0xD0` |
| Worker wait request, infinite | `00DAC090 / 00DAC0B4` | `R+0xD0` |
| Worker reset request | `00DAC090 / 00DAC0D0` | `R+0xD0` |
| Worker execute stream | `00DAB480`, return `00DAC13D` | nDraw executor |
| Worker signal completion | `00DAC1A4`, skip-work `00DAC0F4` | `R+0xD4` |
| Late main wait completion, infinite | call `00DAEE3E`, return `00DAEE40` | `R+0xD4` |
| Main device Present | call `00DAEE53`, return `00DAEE55` | device at `R+0x98`, vtable `+0x44` |

`00DA9CD0` increments epoch `+0x33DBC`, toggles slot `+0x33DB8`, marks
pending `+0x21`, snapshots threaded mode `+0x26` into `+0x27`, then kicks
the worker. `00DA6DA0` selects `slot XOR 1` for threaded execution from
root-pointer/count arrays `+0x433DD8/+0x433DE0`. Main's `00DAF000`
constructs the next stream in the other slot. See [NDRAW.md](../NDRAW.md).

Using stream generation N as the label: main updates/builds N while the
worker executes N-1. On the ordinary threaded path, main joins N-1 late
in this iteration before its Present and before the next iteration can
kick N. It **does not wait for N before starting to build N+1**, because
the kick of N occurs early in that next iteration. Worker completion means
guest command execution/enqueue has finished, not GPU completion.

There is an explicit exception to a blanket per-iteration completion claim:
`00DAED10`, when `+0x26`, pending `+0x21`, and `+0x22` are all set, polls
completion (return sites `00DAED6A/00DAED98`) against the base-frame time
budget, and can return without presenting if still pending. The next
`00DA9CD0` does not toggle/kick again while pending remains set. Mode values
were not recorded in c26vk. Synchronous mode executes on main instead.

The critical section `R+4` is entered after Present for resource cleanup
(`00DAEEC3`); it is not the request/completion handoff primitive.

### Job scheduler

Let `S = *(u32*)0x018D0B28` (sMain). Worker record i is
`S+0x1BDC0+0x20*i`, i=1..7; main participant is record zero.
Worker `+8` is the start-event handle, `+0xC` its completion-event handle.
`00DBDF30` creates two initially nonsignalled auto-reset events per worker
and seven workers. The seventh observed thread, 0x1088, had zero CPU time
and only 100 idle samples; creation is not evidence of participation.

`00DBE560` resets queue claim index `S+0x90`, uses job count `+0x8C`,
dispatch kind `+0x94` and participation bound `+0x98`. For enabled worker
records (`+0x18 & 1`) below that bound, it resets completion (return
`00DBE5F9`) and signals start (`00DBE603`). Main calls `00DBD8D0`
itself before assembling the enabled completion handles and calling
`WaitForMultipleObjects(wait_all=1, INFINITE)`, return **00DBE66D**.
Empty queues return directly; a single job or participant runs on main.

Worker entry `00DBDEF0 -> 00DBDC90` waits at **00DBDCBC** for record `+8`,
resets it at `00DBDCCE`, claims jobs with InterlockedIncrement on `S+0x90`,
then signals record `+0xC` at **00DBDEC9** and returns to waiting. Its
dispatch loop is in `00DBDC90`; main's corresponding loop is `00DBD8D0`.
Therefore join duration is the residual after main's own dispatched work,
including scheduling/wake delay. Worker start waits are idle gaps between
waves; summing them with the main join would count overlapping time.

### Backend, Present and queries

`d3d9cmd::DeferredBackend` consumes a bounded command FIFO on d3d9-backend.
Present is enqueued, with a completion receiver retained by the producer.
Before any command when two Presents are outstanding, `command()` retires
the oldest receiver. The c26vk render wait under `upload -> command -> recv`
is this **two-Present backpressure**, not an upload-result wait; `upload()`
only enqueues. It can stall while the shared graphics backend is held.

`ddgpu::wait_finisher` waits for prior CPU encoding/submission, not by
definition for GPU idle. Backend samples show this under Present (13),
query result (10), upload (6), and draw (1). wgpu-finish waits for incoming
batches (494) and encoder-pool results (107); these overlap backend waits.
GPU execution can overlap all CPU lanes after submission. No precise GPU
fence timing is available from the CPU-stack capture; ~35% GPU utilization
is the head's separate observation.

Occlusion `GetData` appears on main at `01148290`, return `011482BA`,
under `00E271A0 -> 00E277A0 -> 0041DD60`. Seven samples are blocked
acquiring `d3d9draw::graphics`, not directly waiting for query data.
The deferred query API schedules a result request and returns pending;
the asynchronous ddgpu path polls the GPU and returns None until ready.
Starting a resolve can still require the backend to wait for prior CPU
finisher submission. The synchronous query option has different behavior.

## c26vk measured attribution and timeline estimate

Capture: 669 rounds in **30.030144 s**, about 22.3 rounds/s. Samples are
leaf-first host stacks, with no guest registers/handles, frame IDs, or
per-sample timestamps. Threads are suspended/walked sequentially and are
not simultaneous lane snapshots. Resolved symbol data was refreshed during
this analysis; the final capture hash below anchors these counts.

`sample_profile.py`'s legacy WAITS matcher includes
`NtAlertThreadByThreadIdEx`, which is a wake syscall. Exclude these leaves
from blocked time (main 4, render 7, backend 1). Unresolved leaves remain
unknown. The nearest lifted function plus runtime stack identifies a site
family; static code establishes exact return sites/objects.

| Lane / wait | Count / samples | Share | At a hypothetical 44.84 ms period |
|---|---:|---:|---:|
| Main residual job joins | 512/669 | 76.53% | 34.32 ms |
| Render awaiting main request | 323/669 | 48.28% | 21.65 ms |
| Backend awaiting commands | 397/669 | 59.34% | 26.61 ms |
| Backend awaiting finisher | 30/669 | 4.48% | 2.01 ms |
| Render Present retirement | 7/669 | 1.05% | 0.47 ms |
| Render blocked sending commands | 2/669 | 0.30% | 0.13 ms |
| Main graphics mutex in GetData | 7/669 | 1.05% | 0.47 ms |
| Main D3D lock under `01128F40` | 1/669 | 0.15% | 0.07 ms |
| Main render completion / Sleep | 0/669 | 0 observed | not a zero-duration proof |

**The last column is only sample-share scaling, not measured stage latency.**
In particular it cannot be assembled into a 45 ms sequential frame budget:
main's actual CPU delta is **16.328125 s / 30.030144 = 54.37%**, versus
145/669=21.67% legacy busy samples. Render CPU is 55.31%, backend 35.85%.
At 44.84 ms, CPU occupancy alone corresponds to approximately 24.38 ms
main, 24.80 ms render and 16.07 ms backend, overlapping each other. Main
CPU plus the naive 34.32 ms sampled join already exceeds a frame. Sampling
perturbation/phase bias is substantial; near-frame-rate sampling is another
possible contributor, not a proved explanation. Exact boundaries require
the service census or timestamped frame trace. FPS supplied in the brief
is not a measured frame count for these exact 30 seconds.

Dependency timeline (ordinary threaded path; lengths deliberately unscaled):

```text
main:    clock/pre -> kick N-1 -> update/build N [fanout/work/join] x many
         -> sort/publish N -> join N-1 -> enqueue Present N-1 -> tail
jobs:                         [start -> claim/work -> done] x many
render:             [execute N-1 / enqueue draws] -> done -> await kick N
backend:            [consume commands / encode / submit] -> Present -> ...
GPU:                         [execute submitted batches] -> display ...
```

Ranked investigation priorities: (1) main's repeated job-wave residuals,
especially caller `004B97F0` **143**, `004B99D0` **111**, `00DB63E0`
**100**, `00DDBDC0` **36**, `00479CB0` **30**, `00E68390` **22** samples;
(2) backend finisher dependencies, whose contribution depends on whether
they delay Present retirement/query progress; (3) render Present retirement
and main graphics mutex. Render-start and backend-input idle are diagnostic
evidence of upstream gaps, not additional costs to add to this ranking.
The first three caller families account for 354/512=69.14% of join samples.

The six active job workers 1070/1074/1078/107C/1080/1084 have respectively
320/361/375/394/420/442 direct start-event-condvar samples (codex-11's
final leaf classification). Other waits in scheduler wrapper locks are
separate. Avoid counting 0x1088's 100 inactive samples as 30 seconds of
uniformly sampled activity: the profiler stops sampling inactive threads.

## Verification and next action

### Late-game service census, coordinated with L153

Codex-10 recovered complete c26s0 snapshots despite a truncated final row
in an unfinished snapshot; its parser fix is in that agent's scope.
Use snapshots **17 -> 22**, not the startup-heavy whole-run denominator:

| Quantity | c26s0 cache/futex off | c26s1 cache/futex on |
|---|---:|---:|
| report time | 62.787 -> 98.027 s | 63.034 -> 97.020 s |
| frame delta | 633 | 628 |
| Join calls/frame, `00DBE66D` | 69.254 | 69.322 |
| Inclusive service us/join | 269.216 | 256.955 |
| Residual join ms/frame | 18.644 | 17.813 |
| Render request wait ms/frame, `00DAC0B4` | 14.306 | 12.585 |
| Main completion wait ms/frame, `00DAEE40` | 0.262 | 0.795 |

Both captures report overflow zero and participation bound 7: main plus
six active workers. The join's arg0 is 6 (handle count). The seventh
created worker is not in this participation range. `max_jobs=868` is a
cumulative maximum, not the maximum for this interval.

Handle matching confirms request **0x00F0005C**, signalled by main at
`00DA9D78` and consumed/reset by render at `00DAC0B4/00DAC0D0`;
completion **0x00F00060**, signalled at `00DAC1A4`, waited/reset by main
at `00DAEE40/00DA9D6B`. The s1 interval independently parsed here has
629 request waits, 631 main completion waits, and 43,534 joins. Small
per-frame count differences reflect snapshot boundaries, not a new protocol.
There are no `00DBFDC3` Sleep rows with calls in this s1 late interval;
head's whole-run ~1 ms/frame pacer estimate included earlier phases.

This supports a per-frame budget of main's own update/work interleaved
with approximately **18 ms residual job barriers**, overlapping render
execution, followed by **less than 1 ms residual render join** before
Present. Render's 12.6-14.3 ms request wait is the gap after completing
its stream until main's next kick, overlapping main's current work/joins.
Backend/finisher/GPU work overlaps those lanes; c26vk sample attribution
locates their dependencies but does not supply same-run stage durations.
These service times are inclusive wall time (including contention and
scheduling), not primitive overhead recoverable by replacing every wait.

The census emits about 15,000 rows per report and observed report spacing
grew to roughly 7 seconds; codex-10 estimates 4-5 seconds of formatting.
Consequently do not add these values to uninstrumented CPU budgets or
claim an FPS gain from s0 versus s1. Snapshots are approximate boundaries,
and waits completing after a boundary can carry earlier time into a delta.

The old scheduler-job filter observes only main's `00DBD8D0` call sites,
not worker `00DBDC90` sites. Worker job duration/imbalance histograms are
therefore missing, not zero. Codex-10 is extending coverage. The next
useful head run is a lighter late-game census including worker dispatch
sites, followed by logging-off matched cache/futex timing/image checks.
It should distinguish the longest worker job per wave from wake-to-resume
delay before any attempt to change scheduling. Existing line dependencies
remain authoritative.

Read-only cross-checks: lifted main/application, scheduler initialization,
fanout/main dispatch/worker dispatch and render handoff bodies; private
L108 decompile/disassembly; FRAME/NDRAW docs; runtime deferred queue,
query, finisher and event implementation. c26vk main join counts and
render/backend waits were independently checked with codex-11.
No Sleep polling or lost-wakeup defect is established in the hot waits:
infinite waits use condition variables with registration/recheck, not a
fixed 1 ms polling loop. No speculative opt-in runtime fix was added.

Next: the corrected worker census described above, keeping
non-VK runs distinct from c26vk. CPU stacks alone cannot
separate slow jobs from wake latency or recover per-frame critical-path
length. No cargo/GPU tests are required for this analysis-only change;
no target directory was created. Game/image/FPS acceptance remains with head.

Private evidence stays outside the repository:

- `E:/ddda-re/notes/runs/stacks_c26vk.json`, SHA-256
  `745cf2d17b508d8d771da9ac87131d53eec6560101ab1dd2aaf0f57c25b62036`.
- Derived counts and inspected lifted excerpts: `E:/ddda-re/notes/L157/`.
  No raw stacks, decompiler output, game data, or executable bytes committed.
- `run_c26s0.log`, SHA-256
  `3146c97e4a512f01205314903001556d1dbd1b6335f1aab33801697cfb2c12e1`.
- `run_c26s1.log`, SHA-256
  `73fbb3c37045e5eff233fbd2452548a3b73636a47d46627cb73752b536f1c787`.
  Independently parsed s1 snapshot delta: `L157/c26s1-late.json` (private).
