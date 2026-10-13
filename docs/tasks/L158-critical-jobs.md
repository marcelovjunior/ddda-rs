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
