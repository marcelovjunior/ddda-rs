# L158: critical scheduler jobs

Owner: codex-7. Branch: `task/L158-critical-jobs`, from fps-line combo27.
Scope: measured job ranking, correlated diagnostics, exact hot inner ports.
No agent game runs. Ghidra unavailable; source is the verified lifted code
and private original-instruction oracles.

## Results (first batch)

The existing capture cannot identify the last callback at each join:
L153's old census omitted worker return sites, and its per-target rows are
aggregate main-thread durations. Raw stacks are sequential samples, not
simultaneous frame snapshots. No per-job speedup is claimed yet.

Initial ranking posted to head before implementation (board #13019).
Late `run_c26s1.log` complete snapshots 17 to 22 cover 628 Presents:

| Main callback | Calls/Present | Mean us/call | Inclusive ms/Present |
|---|---:|---:|---:|
| DB7620 | 44.102 | 242.14 | 10.679 |
| 4B9FF0 | 69.503 | 74.52 | 5.179 |
| 1090F70 | 59.782 | 65.80 | 3.933 |
| BAF4F0 | 1.035 | 1450.09 | 1.501 |
| BB2890 | 1.304 | 970.46 | 1.266 |
| B53650 | 0.710 | 1456.19 | 1.034 |
| 5EE640 | 1.005 | 647.00 | 0.650 |
| E0CE50 | 1.511 | 130.54 | 0.197 |

These are not critical-path worker rankings and must not be summed:
same-frame tail redispatch was double counted (e.g. B54050/B66110).
DB7620 calls virtual slot +0x2c, with a separate BFC7D0 alternate branch;
4B9FF0 and 1090F70 tail-dispatch virtual slots +0x20/+0x24. Thus those
target addresses aggregate many classes. L153 `792dc8a1` fixes worker
coverage and tail duplication for the next capture.

Private inventory names further identify BAF4F0/BB2890 as `uNpc::vf08/vf09`,
B53650 as `uCmc::vf08`, 5EE640 as `uGUIActionPallet::vf08`, and E0CE50 as
`uGUI::vf09`. EA3130/EA5320 are `uBaseModel::vf11/vf26`; B61280 is
`uPlayerBase::vf08`. Names are evidence-backed labels, not inferred jobs.

`scripts/critical_jobs.py stacks_c26vk.json` independently admits only
stacks with DBDC90: 3,939 dispatcher samples, 2,641 without a visible
job, 1,298 with a visible job. Outermost visible DB7620: 593 samples,
including 211 fast loads, 74 fast stores, 31 flags, 23 x87, 12 SSE.
BAF4F0/BB2890/B53650: 91/56/32 samples. These are counts, not self times
or CPU percentages. The nearest visible lifted function may have missing
or optimized-away callees; unresolved PCs remain explicitly unresolved.

Nearest visible functions: E12A20 72, FEE320 59, 1054700 39, FF1920 35,
FEFB90 30, FEDD60 29, E0E5E0 17. E12A20 includes the already ported
E0EC50 dependency expansion. 1054700 itself contains no SSE arithmetic:
its sampled SSE helpers are below indirect callees, not its own loop.
FEDD60 is a stable pair-key mergesort; E0E5E0 is a word-wise dependency
hash. Packed SSE helpers already execute real packed host instructions,
preserving MXCSR and operand order.

## Correlated trace

`DDRUNTIME_CRITICAL_JOBS=E:/ddda-re/notes/runs/L158.trace` opts in.
Use alongside `DDRUNTIME_SYNC_PROFILE=1`, with L153's repaired census.
The wrapper for 0041DD60 labels application frame iterations independently
of successful producer Presents. DBE560 gets a batch ID before it wakes
workers. All eight callback returns record target, ECX, vtable, candidate
virtual target, thread, guest job-record pointer, start/end. Same-stack
tail redispatch is suppressed, and the marker is restored on unwind.
The DBE66D service records main wait start/end/result.

DBE603 SetEvent and DBDCBC worker waits additionally record handle, thread,
entry/return timestamps and both batch IDs (a wait begins before its next
batch exists). Signal entry precedes publication; signal return can occur
after the worker wakes. L160 owns wake-latency interpretation.

One bounded nonblocking channel feeds a dedicated buffered file writer;
overflow drops diagnostics instead of blocking guest workers. Cumulative
loss and panic markers are explicit. The parser rejects incomplete,
nested, lossy, unsuccessful, or temporally inconsistent joined batches and
requires the number of observed callbacks to equal the queue count.
Serial and empty batches have no residual join and are excluded. Shutdown
may truncate the last buffered batch, which the parser also excludes.
The engine's single global scheduler is the observed contract; overlapping
scheduler invocations are unsupported and must not be interpreted.

```
python scripts/critical_jobs.py E:/ddda-re/notes/runs/L158.trace --trace
python scripts/critical_jobs.py E:/ddda-re/notes/runs/L158.trace --trace --json
```

Use `--first-frame N --last-frame M` to isolate the gameplay interval, and
`--dti E:/ddda-re/notes/dti-registry.json` for exact last-job vtable labels.

Each admitted join includes the longest-duration job and the last-completing
job separately, the penultimate completion, wait after other jobs finish,
last-job overlap with that wait, and delay after all callbacks complete.
The latter includes scheduler epilogue, event signalling, wake scheduling
and diagnostic overhead; it is not purely OS wake latency. Instrumented
durations require matched instrumentation for A/B. The virtual target field
is only a candidate, particularly for DB7620's alternate path.

## Validation and evidence

- Four focused runtime diagnostics tests pass with tracing enabled,
  including a synthetic two-job batch, tail suppression, guest rounding/
  sticky flags, guarded reads, and unchanged guest registers/stack.
- Four Python tests pass; synthetic trace round-trips through the parser.
- Light runtime suite: **226 pass / one inherited fullscreen gamma failure /
  six ignored**. No shards, GPU tests or game execution.
- Raw stacks SHA-256:
  `745cf2d17b508d8d771da9ac87131d53eec6560101ab1dd2aaf0f57c25b62036`.
- c26s1 log SHA-256:
  `73fbb3c37045e5eff233fbd2452548a3b73636a47d46627cb73752b536f1c787`.

Next: head captures corrected census + trace on combo27, then rank the
actual last-completing jobs per frame. An exact E0E5E0 inner hash port is
being evaluated with original vectors and isolated lifted replay; it is
not yet implemented or measured in this checkpoint.

## E0E5E0 candidate (second batch)

Implemented the complete integer hash in `mtcore::job_hash`, with optional
registration through `DDRUNTIME_JOB_HASH=1` (default off). The source's
four multiply-then-XOR byte steps, salt, byte-tail truncation, guest spills,
register results, defined flags and ordered memory accesses remain exact.
Arithmetic uses local intermediates; the existing `Pages` helper performs
each permission/atomic check inline in this one optimized readable port.
No generic memory policy, lifter output or scheduler behavior changes.

In c26vk, 16 of its 17 visible worker samples are below DB7620, including
`DB7620 -> EA3130 -> EA5320 -> FF1920 -> E12A20 -> E0E5E0` (six samples).
It is a bounded first candidate inside a leading job family, not evidence
that it dominates the long residual joins.

**Verified:** 528 original-instruction returns match the port and isolated
lifted body separately. Inputs cover 44 byte lengths (0 through 4096),
aligned/unaligned/page-edge pointers, stack aliasing, random/zero/all-one
bytes, random registers and four MXCSR rounding modes. An additional 672
paired injected access positions match lifted registers, flags, writes,
ordered accesses and fault/completion outcomes. The isolated gate reports
three passing tests; no shard build is needed.

The bounded CPU benchmark compiles the lifted body at opt1 and base/port
at opt3, as a production native override would run. It uses atomic page
accesses, alternating baseline/candidate order, seven pairs per setting.
All **105/105 pairs improve**; median speedup across 15 length/alignment
settings is **1.63–2.01x**. Aligned examples:

| Input bytes | Lifted ns/call | Port ns/call | Ratio |
|---|---:|---:|---:|
| 16 | 86.84 | 48.28 | 1.80x |
| 64 | 238.96 | 125.31 | 1.91x |
| 256 | 868.09 | 430.93 | 2.01x |
| 1024 | 3236.05 | 1644.29 | 1.97x |
| 4096 | 12245.00 | 6654.40 | 1.84x |

These are synthetic leaf timings, not measured whole-job/frame gains.
Supplemental page-table replay and own-crate registration tests are queued
behind head's combo30 hold. The combined native gate and a matched
`DDRUNTIME_SYNC_PROFILE=1` / hash=0 versus hash=1 game run remain required.

Reproduce (research recorder runs through `heavy.py`):

```
PY research/native/record_job_hash.py
python scripts/check_job_hash.py --bench
DDNATIVE_REQUIRE_VECTORS=1 DDRUNTIME_JOB_HASH=1 cargo test -p mtcore -p ddports
```

Private original vector SHA-256:
`af8d16a118856af29bcfce8de1a6c86c01f9dd5d15a4fb71ae4a074cd815b209`.
Original leaf code-range SHA-256:
`56ebab093475b4475b5b5b27703d0a283386f58f282ce3cc28b1561cf5fd2cf7`.
Gate/benchmark log `E:/ddda-re/notes/L158-isolated.log` SHA-256:
`2f0611782706de71e7e76f0b62acc12bfc254ccdba029a40c6439449c6621006`.

Trace follow-up also adds queue mode/record identity validation, exact DTI
labels and frame filters. Those changes are not part of the original
973dbf3f trace checkpoint; focused tests will be repeated after the hold.
