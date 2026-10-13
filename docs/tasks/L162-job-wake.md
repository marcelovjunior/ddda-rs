# L162: wake latency and original participant setter

Owner: codex-11. Branch: `task/L162-job-wake`, based on fps-line
`3e72b984`, with `task/L160-job-threads` (`a791345c`) merged first.
Assigned by head/user 2026-10-13. Runtime only, no agent game/shard runs.

## Results

Implemented opt-in `DDRUNTIME_JOB_PARTICIPANTS=1..8`. Absent leaves the
original initialization unchanged; invalid values stop initialization.
After original sMain constructor `00DBE810` returns, the wrapper invokes
the existing `00DBD620` body with ECX=this and one stack argument (ret 4).
That body alone writes configured +`9C`, clamped to reported CPUs and eight.
Original `00DBFA90` copies it into active +`98` at frame start. No CPU
report, active-count patch, worker array, scheduler cap, or background
pool behavior is changed. Constructor CPU state and temporary stack words
are preserved. Startup stderr records requested/configured/active/CPUs.

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
The source trace SHA-256 is included in JSON.

Wake count is not configured participation: `DBE599` bypasses worker wakes
for a single queued job, and `DBE5E5` conditionally wakes each slot. Old
traces without the Batch `participants` field therefore report null for
active count and jobs/participant. L158 owner asked to add +98 at #13252.

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

- 13 offline parser tests pass, including overlapping tails, gap clipping,
  missing wakes and bad joins; original seven pairing guards retained.
- Initial light library run: 86 passed / one inherited Present throughput
  timing failure / five ignored. Both new ABI/configuration tests passed.
- Full `cargo test -p ddruntime --no-default-features --no-fail-fast` launched
  with CARGO_BUILD_JOBS=4, CARGO_INCREMENTAL=0, target `C:/ddda-targets/L162`;
  currently suspended by head's combo30 run. Final outcome to follow.
- No game, GPU, shard build or original-game image comparison by this agent.
- **Pending:** head trace, actual per-join numbers, 7/8 activation comparison,
  and any measured >20 us wake fix coordinated with codex-10 (L153).
  Existing aggregate logs do not establish wake latency; no speculative
  sync change or speedup claim is made.
- Next action: head integrates this checkpoint with L158 and captures the
  matched trace; codex-11 analyzes it and reports hashes and per-join numbers.
