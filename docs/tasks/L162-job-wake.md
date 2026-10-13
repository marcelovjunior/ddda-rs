# L162: wake latency and original participant setter

Owner: codex-11. Branch: `task/L162-job-wake`, based on fps-line
`3e72b984`, with `task/L160-job-threads` (`a791345c`) merged first.
Assigned by head/user 2026-10-13. Runtime only, no agent game/shard runs.

## Results

### L162c: distribution and original host scheduling requests

Merged `task/fps-line` before this follow-up. Read-only Ghidra audit confirms
`00DBE560` signals enabled workers in ascending slot order, then invokes the
main dispatcher `00DBD8D0`. Main and `00DBDC90` workers claim the same FIFO
through InterlockedIncrement at scheduler+90. There are no per-worker queues
to steal from. A running callback cannot migrate or split. Reordering entries
or making main skip long callbacks would change original game scheduling;
the evidence does not justify such a runtime patch. `00DC5C20` around the
parallel batch only writes two engine flags, not OS thread priority.

New complete-batch census, c31 h0 frames 6600..6749 (150 frames), same source
hash recorded below; 12,600 complete batches, zero rejected selected batches:

| Guest execution lane (thread ID) | Callbacks | Callback time ms/frame |
|---|---:|---:|
| Main, 1000 | 65,186 | 22.051 |
| Worker, 1070 | 88,096 | 24.911 |
| Worker, 1074 | 78,545 | 23.460 |
| Worker, 1078 | 64,249 | 24.029 |
| Worker, 107C | 55,879 | 19.843 |
| Worker, 1080 | 52,116 | 17.493 |
| Worker, 1084 | 42,930 | 16.747 |

Times are inclusive callback wall time, not thread CPU time; lanes overlap.
Earlier-signaled workers get more callbacks, but job counts do not represent
equal work. Trace IDs identify execution lanes; the analyzer does not invent
slot numbers by sorting IDs. Seventh worker 1088 is absent from this p7 trace.
The census retains awakened workers with zero callbacks and specifically
counts p8 batches in which seven distinct workers obtain callbacks.

Main owns the final callback in 2,080 of 11,250 nonempty batches. Its final
solo tails total **2.66139 ms/frame**; those precede main's join and therefore
are not contained in the previous 14.819 ms/frame join measurement. Workers'
unclipped solo tails total 7.13795 ms/frame (the earlier 7.08581 figure is
clipped to main's join). The five largest main tails are DB7620 callbacks:
start **92.0..106.7 us** after batch entry, duration **14.650..18.009 ms**,
solo tail **6.934..9.381 ms**. Five largest worker tails start **95..134.3 us**
after entry, duration **14.805..18.510 ms**. They start early, not near the
end of their batch. Workers can finish while main remains in its long callback;
this does not establish runnable queued work left behind or kernel idleness.
General final-callback start delay p50/p95/p99 is 42.6/383.9/1127 us, but
includes naturally late short jobs in large queues. No queue-order fix is proven.

**Implemented candidate:** independent `DDRUNTIME_THREAD_PRIORITY=1` and
`DDRUNTIME_THREAD_IDEAL=1` forward original Win32 requests. Both default off.
`00DBDF30` foreground construction requests ideal CPUs 1..7 only; foreground
priority stays normal. `00DBDFE0` background construction requests -1 priority
and ideal CPUs 0..7. Both API calls were previously success stubs. No affinity
API is imported by the original or bound here; ideal processor is only a hint.
API contracts: [SetThreadPriority](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setthreadpriority)
and [SetThreadIdealProcessor](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setthreadidealprocessor).

CreateThread retains its host JoinHandle before returning, allowing scheduling
requests before ResumeThread without an asynchronous registration race. Final
guest-handle close detaches/releases it; aliases keep it alive. Pseudo-current
handles resolve directly on the calling host thread. Native errors are returned
through guest last-error; x86 ideal-query sentinel 32 maps to the x64 sentinel
64. Non-Windows opt-in calls report not-supported. No boost chosen by the
runtime, no affinity pinning, no synchronization primitive or game code changed.

**Tested:** 19 parser tests; four focused Windows tests, including real suspended
guest creation in both serial and parallel modes, prove callback-entry priority
-1/ideal CPU 0 before first work, handle/alias/close behavior and stdcall/error
results. Full runtime light gate before the final lifecycle test/close-scan
refinement: **254 reported passes, zero failures, seven ignored**; final four
focused tests pass after those changes. Four build jobs, incremental off,
`C:/ddda-targets/L162`; no game/GPU/shard run. No timing speedup claimed from
these semantic tests, and no original-game image comparison performed.

Private outputs: `L162-job-wake/c-lanes-h0.json`, `c-light.log`, and
`c-scheduling-final.log` under `E:/ddda-re/notes`. The analyzer reads the
compressed trace and retains its decompressed hash. Counts posted to head
on board #13573. First-frame p8 run requested #13526; available c31p8 files
still predate the setter follow-up, so **eighth-lane work remains unconfirmed**.

**Next head action:** in the next integration build first capture p8 activation
and per-thread callbacks with the existing trace. Independently compare
priority/ideal 0/0, 1/0, 0/1 with participants/hash/cache/futex held fixed;
then profile-off image/FPS acceptance. The API candidates restore ignored
requests but are not yet a demonstrated remedy for the long callback tails.

Implemented opt-in `DDRUNTIME_JOB_PARTICIPANTS=1..8`. Absent leaves the
original initialization unchanged; invalid values stop initialization.
After original sMain constructor `00DBE810` returns, the wrapper invokes
the existing `00DBD620` body with ECX=this and one stack argument (ret 4).
That body alone writes configured +`9C`, clamped to reported CPUs and eight.
Original `00DBFA90` copies it into active +`98` at frame start. No CPU
report, active-count patch, worker array, scheduler cap, or background
pool behavior is changed. Constructor CPU state and temporary stack words
are preserved. Startup stderr records requested/configured/active/CPUs.

Follow-up: reapply the original setter once on the first `00DBFA90` entry,
after the rest of `0041B400` initialization, before the original frame body
promotes configured to active. Log the resulting active count after that
frame body. Subsequent frames do not enforce the override. This is a
candidate activation correction: c31's ctor-only option logged configured
eight, but its seventh worker stayed idle. A later configuration reset is
suspected, not yet established; the follow-up needs an instrumented head run.

Evidence: `crates/ddlift/lift_27/src/generated/lifted_024.rs`, raw body
registry in its `mod.rs`, and L160 original instruction audit. This is an
invocation of existing game code, not a new translated setter.
On the shared 16-logical-CPU host the requested 7/8 should configure
main+6/main+7 workers. Actual activation and image/FPS acceptance await head.

`scripts/job_wake.py` now includes per-batch queue count, signaled lanes,
active participants (when supplied by L158), workers obtaining callbacks,
jobs/participant, each worker's
signal-to-wait and signal-to-first-callback latency, callback gaps while
some callback remains unstarted, last callback identity, last callback's
solo tail after all others finish, its overlap with each join wait, and
last callback completion to batch end. Missing/lossy batches are rejected.
The source trace SHA-256 is included in JSON. `--summary-only` omits raw
rows while retaining wake distributions, job-count/participant histograms,
counts of batches with at least eight jobs, and tail/gap percentiles.

Wake count is not configured participation: `DBE599` bypasses worker wakes
for a single queued job, and `DBE5E5` conditionally wakes each slot. Old
traces without the Batch `participants` field therefore report null for
active count and jobs/participant. L158 owner added +98, configured +9C
and dispatch mode at #13257 (commit pending their post-pause gate).

The gaps are **not proof of kernel idleness or unclaimed queue entries**:
claiming and dispatch are outside callback boundaries. They include wake,
claim/dispatch, telemetry, and scheduling costs. Gaps across workers overlap
and must not be summed into frame time. Signal start is SetEvent entry,
not the earlier enqueue time. This schema cannot recover enqueue timestamps.

Original splitting is dynamic, not eight equal chunks: `DBD8D0`/`DBDC90`
claim queued callbacks one at a time. `004152C0` queues one callback per
participant, so it can use the eighth; other producers queue per item/node
independently of participant count. A single callback cannot be subdivided
by this dispatcher. Existing maximum 868 jobs/join shows some substantial
batches, but cannot establish divisibility of current critical joins.
Per-join measured queue counts and tails remain pending the trace.

## Exact head capture request

Integrate L158 trace (`973dbf3f` or its successor) and this branch in the
next normal build. Only head launches the game. Keep route and other
rendering settings identical. Baseline PowerShell environment:

```powershell
$env:DDGPU_VK = '1'
$env:DDRUNTIME_JOB_PARTICIPANTS = '7'
$env:DDRUNTIME_SYNC_PROFILE = '1'
$env:DDRUNTIME_SYNC_CACHE = '0'
$env:DDRUNTIME_SYNC_FUTEX = '0'
$env:DDRUNTIME_CRITICAL_JOBS = 'E:/ddda-re/notes/runs/L162-p7.trace'
```

For the matched participant run change only JOB_PARTICIPANTS to `8` and
the output to `L162-p8.trace`. Include about 30 s on the same late-game
route, preserve stderr (configuration line and sync snapshots), and post
trace paths, executable SHA-256, build commit, full env and late-frame
boundaries. L158 labels application frames, not producer Presents.

```powershell
python scripts/job_wake.py E:/ddda-re/notes/runs/L162-p7.trace --min-frame N > E:/ddda-re/notes/runs/L162-p7.wake.json
python scripts/job_wake.py E:/ddda-re/notes/runs/L162-p8.trace --min-frame N > E:/ddda-re/notes/runs/L162-p8.wake.json
```

Use each run's actual late-game frame N. Next, if wake delay warrants it,
hold participants fixed and compare SYNC_CACHE/FUTEX=0/0 against 1/1 with
separate trace names. Profile-off runs are required for FPS conclusions.
Requests/coordination posted to head, codex-7 and codex-10 (#13153, #13215).
Codex-7 authorized narrow dispatch/lib integration (#13162).

## Verification / remaining work

- 17 offline parser tests pass, including overlapping tails, gap clipping,
  missing wakes and bad joins; original seven pairing guards retained.
- Initial light library run: 86 passed / one inherited Present throughput
  timing failure / five ignored. Both new ABI/configuration tests passed.
- Full `cargo test -p ddruntime --no-default-features --no-fail-fast` completed
  with CARGO_BUILD_JOBS=4, CARGO_INCREMENTAL=0, target `C:/ddda-targets/L162`;
  **234 reported pass results / two inherited failures / seven ignored**.
  Failures: Present throughput timing and fullscreen gamma. Isolated Present
  retry (`--lib ... --exact --test-threads=1`) passes. No gamma fix in scope.
  Private logs: `E:/ddda-re/notes/L162-job-wake/light.log` and
  `present-retry.log`. Full log SHA-256:
  `4db8f6045c599e322a2c6e8aa796ef9f1ad925729b468900ae92d469ceaa87ef`.
- No game, GPU, shard build or original-game image comparison by this agent.
- **Pending after c31:** instrumented 7/8 activation comparison and matched
  cache/futex capture to identify a causal wake fix, coordinated with codex-10
  (L153). No speculative sync change or speedup claim is made.
- Next action: head integrates the first-frame setter follow-up and checks
  active participation, then holds hash/participants fixed for sync A/B.

## Combo31 measured wake windows (2026-10-13)

Head build `01c7f74d`, VK=1; `L158_h0.trace` with JOB_HASH=0 and
`L158_h1.trace` with JOB_HASH=1, both SYNC_PROFILE and CRITICAL_JOBS on.
Cache/futex inherited values await head confirmation. These are not a sync
A/B and include diagnostic overhead and shared-machine load.

Each sample has **150 late gameplay frames, 12,600 complete batches,
62,100 paired worker wakes, zero rejected selected batches**. All report
seven active/configured participants. h0 frames 6600..6749; h1 6819..6968.
The two-pass file reader hashes both scans and rejects changing captures,
retaining only selected batches. Explicit `--allow-truncated-tail` discards
43/70 unterminated EOF bytes; unfinished batches are still rejected.
The 1.7/1.8 GB originals are never loaded wholesale.

Times below are microseconds, median / p95 / p99:

| Metric | h0 | h1 |
|---|---:|---:|
| Signal entry -> worker wait return | 11.5 / 52.2 / 131.8 | 10.6 / 25.0 / 75.4 |
| Signal entry -> first callback (workers obtaining work) | 16.7 / 38.1 / 84.7 | 15.0 / 24.8 / 52.0 |
| Wait return -> first callback | 5.0 / 9.7 / 25.9 | 4.3 / 8.2 / 12.2 |
| SetEvent service duration | 3.1 / 5.9 / 19.0 | 2.9 / 5.0 / 9.4 |
| No-callback gap while another callback unstarted | 1.4 / 19.6 / 42.7 | 1.4 / 17.0 / 25.9 |
| Last callback solo tail | 2.3 / 433.7 / 3790 | 1.3 / 401.5 / 3675 |

Signal-to-first-callback exceeds 20 us in **13,402/48,436 (27.67%)** h0
and **7,796/49,547 (15.73%)** h1 working-worker samples. Signal-to-wait
means are 17.825/11.786 us. Tails exceed 20 us, but medians and means do
not establish a uniformly slow wake implementation.

Per-frame critical-path accounting (overlapping quantities, not additive):

| Quantity, ms/frame | h0 | h1 |
|---|---:|---:|
| Main join waits | 14.819 | 13.526 |
| Join overlap with final callback running alone | 7.086 | 6.887 |
| Join time after last callback completes | 1.633 | 1.223 |
| Join overlap with latest wait return after all callbacks complete | 0.307 | 0.142 |

Both last rows are clipped to the main join interval. Unclipped values
(1.826/1.401 and 0.408/0.221 ms) included time before main entered its join
and are not join-cost attribution. The last row is a lower bound on late
idle-worker completion costs, not total synchronization cost. L158's
independent analysis agrees with the clipped join and solo measurements.
The largest five h0 solo tails overlapping a
join are **7.084..7.730 ms**, all callback `00DB7620`, candidate resolved
target `00EE2C90`. Four of those batches have maximum wake delay only
13.5..17.9 us. Thus the dominant long joins are callback tails; these data
do not justify changing L153 primitives by attributing the whole join to
kernel wakes. Results posted to codex-10 and codex-7 (#13370/#13375/#13431).
Next synchronization experiment: hold JOB_HASH and participants fixed and
capture cache/futex 0/0 vs 1/1 before choosing a wake-path change.

### Jobs versus participants; eight-participant caveat

In h0, 84 batch entries/frame include nine empty and six single-job batches;
**69/frame wake workers**. **51/frame have >=7 jobs; 40/frame have >=8**
(57.97% of parallel batches). Queues reach 529 in this sample. Eight lanes
therefore have potential work in many batches, but cannot subdivide the
millisecond-scale final callback. These counts establish available queued
callbacks, not a speedup guarantee.

The profile-off `run_c31p8.log` confirms the original setter produced
requested=8/configured=8/active=7 at construction on 16 CPUs. However,
`profile_c31p8.txt` reports the seventh worker (`guest 0x1088`) at **0.0 CPU
seconds / zero busy / 100 wait samples**, as in the default run. The other
six workers are busy. Therefore c31p8 does **not** verify active eight-lane
execution or its FPS effect. Head reports 19.8 vs 21.1 average FPS and warns
that differences below 8% are noise on this busy machine. No gain claimed.
The first-frame reapplication/activation log is the next candidate, with
**three focused runtime tests passing**, including configuration overwritten
between base construction and first frame and respecting later changes.
The complete follow-up light gate reports **242 passes / one inherited
Present throughput timing failure / seven ignored**; gamma passes this time.
Private final log: `E:/ddda-re/notes/L162-job-wake/first-frame-light.log`.

### Reproducible private outputs

Full per-join rows (wake pairs, gaps, last job, join overlaps):
`E:/ddda-re/notes/L162-job-wake/h0-6600-6749.json` and
`h1-6819-6968.json`. They contain the original trace source hashes:

- h0: `6804199b37027679b142ade4ef6d70f0d330379046cee60c595f241fdb1eecfa`
- h1: `c515d899df51c41497d05e4f832cfef71c8d8931f48e2be53d76531c132a9f65`

Example: `python scripts/job_wake.py PRIVATE.trace --min-frame 6600
--max-frame 6749 --allow-truncated-tail > PRIVATE.json`. Add `--summary-only`
for small output. Head authorized trace compression after both owners finish;
codex-7 notified that L162 no longer needs uncompressed originals (#13432).
Codex-7 completed compression and verified decompressed SHA/length before
removing the two originals (#13466): `runs/L158_h0.trace.gz` **191,142,074 B**,
`runs/L158_h1.trace.gz` **204,890,226 B**, net space recovered
**3,121,002,352 B**. Receipt: private `native-L158/trace-compression.json`.
The analyzer reads `.trace.gz` directly; `source_sha256` hashes the
decompressed trace, preserving the original hashes above.
