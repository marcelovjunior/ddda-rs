# L158: critical scheduler jobs

Owner: codex-7. Branch: `task/L158-critical-jobs`, from fps-line combo27.
Scope: measured job ranking, correlated diagnostics, exact hot inner ports.
No agent game runs. Ghidra unavailable; source is the verified lifted code
and private original-instruction oracles.

## Results (combo31 follow-up)

Merged `task/fps-line` `01c7f74d` before this batch. The two complete head
captures establish a critical-path ranking, but **no port has a cleanly
isolated causal join-wait improvement yet**. Head reports shared-machine
load and treats changes below 8% as noise; NPC counts also differ.

Primary windows, agreed with L162: hash=0 frames **6600–6749**, hash=1
**6819–6968**. Each has 150 frames, **10,350 valid joins**, 69 joins/frame,
and 2,250 serial/empty batches excluded. No joined batch was rejected.
The stream parser checks the entire source's line structure and hashes it;
only the unterminated final 43/70 bytes are excluded. Worker callback work
is inclusive and overlaps; it must not be added to join time.

| Metric, ms/frame | JOB_HASH=0 | JOB_HASH=1 |
|---|---:|---:|
| Main join waits | 14.818612 | 13.526252 |
| Last callback solo tail overlapping join | 7.085813 | 6.887015 |
| Join time after all callbacks finish | 1.633497 | 1.223178 |

The last two intervals are clipped to the actual join and callback start.
The older `critical_jobs.py` tail formula could include a gap before the
last callback started; this batch corrects it and adds a regression case.
Per-frame rankings and all callback family totals remain in private
`native-L158/c31-h0-ab.json`, `c31-h1-ab.json`, and `c31-comparison.json`.

| Callback / candidate virtual (class) | Calls/frame 0→1 | Work us/call 0→1 | Last-family join ms/frame 0→1 | Solo tail ms/frame 0→1 |
|---|---:|---:|---:|---:|
| DB7620 / EE2C90 (uSkyStableShadow) | 1→1 | 13420.1→11819.1 | 4.2679→3.9584 | 3.9046→3.6513 |
| 4B9FF0 / C70790 (uSoundGenerator) | 1→1 | 555.2→490.9 | 0.5551→0.4969 | 0.4662→0.4176 |
| 4B9FF0 / BAF4F0 (uNpc) | 8→7 | 1524.0→1381.0 | 1.2447→1.0501 | 0.4603→0.7998 |
| FED7E0 (cDraw) | 54.52→54.04 | 7.85→6.85 | 0.2710→0.2403 | 0.2475→0.2174 |
| 4B9FF0 / B53650 (uCmc) | 3→3 | 1500.0→1248.4 | 0.7546→0.4806 | 0.2402→0.0983 |
| 1090F70 / BB2890 (uNpc) | 8→7 | 1054.2→973.1 | 0.4635→0.7521 | 0.1416→0.2762 |

The shadow family ranks first by total solo tail in **123/150** hash=0
frames and **121/150** hash=1 frames. Its observed join change is -7.25%,
inside head's noise threshold. NPC work per call improves while its solo
tails grow: faster callbacks do not automatically shorten the last wait.
The candidate virtual field and exact vtable class do not identify all
nested callees. E0CE50/5EE640 do not occur as direct/candidate targets in
these admitted parallel batches; that does not prove their nested absence.

Sensitivity window: hash=0 6400–6699 vs hash=1 6600–6899, 300 frames each,
20,700 joins each. Join **15.685949→13.369194**, solo tail
**7.658606→6.374987**, post-callback **1.871323→1.220800 ms/frame**.
The window-sensitive size of the apparent gain reinforces the causal limit.
Head's whole runs: instrumented hash 0/1 **17.8/19.9 average FPS**;
default **19.8**, participants=8 **21.1**. These are separate, noisy runs.
L162 found the extra worker still idle; p8 is not verified eight-way work.

Reproduce the retained frame/family data without expanding the gzip:

```
python scripts/critical_ab.py E:/ddda-re/notes/runs/L158_h0.trace.gz --first 6400 --last 6749 --out E:/ddda-re/notes/native-L158/c31-h0-ab.json
python scripts/critical_ab.py E:/ddda-re/notes/runs/L158_h1.trace.gz --first 6600 --last 6968 --out E:/ddda-re/notes/native-L158/c31-h1-ab.json
```

Head authorized compression (#13390); L162 finished reading (#13432).
Both gzip streams were independently decompressed and checked against the
original SHA-256 and length, then each raw file was rehashed before removal.
**3,121,002,352 bytes recovered**, retaining 191,142,074 / 204,890,226-byte
gzip files and `native-L158/trace-compression.json` receipts.

## Next exact leaves: shader dependency lookups

`mtcore::job_lookup` translates **01008200/01008260** completely and
registers them only with **DDRUNTIME_JOB_LOOKUP=1**. Source chain:
FF1920/FEE320 → E132B0 → E12A20 → E0F3C0/E0F4B0 → these leaves.
They traverse the original bucket/link layout; single-key lookup rejects
the sign-bit index while pair-key lookup masks it. Guest spills, access
order, arithmetic wrap, registers and defined flags are preserved.
No borrowed guest slices, caching, table copies, or algorithm replacement.
Live call frequency and critical-path benefit of these branches remain
unmeasured; the trace does not resolve nested leaves.

**Verified:** 1,380 original-instruction vectors (690 per leaf), empty and
colliding buckets, head/middle/tail hits, misses, sign-bit indices, paired
key mismatches, unaligned/page-edge records and link-address wrap to zero.
Randomized CPU/memory state includes four MXCSR modes. Isolated lifted
and port replay agree; **11,520 paired access/fault positions** also agree.
All vectors additionally replay through Pages in serial and atomic modes.
The isolated gate has **four passing tests**, and builds no shards.

Synthetic atomic-page lookup benchmark: opt1 lifted body vs opt3 readable
port, seven alternating pairs at chain lengths 0/1/2/3/8/32/64. **98/98
pairs improve**, median speedup **1.43–2.68x**. For 64-node misses,
single-key medians **1114.69→484.58 ns**, pair-key **1395.01→643.01 ns**.
These timings include shared-machine noise and are not whole-job gains.

Required-vector mtcore/ddports suite with lookup enabled: **19 passed,
zero failed, nine ignored**. Python critical-analysis tests: **nine pass**.
Runtime light suite: **242 reported passes, zero failures, seven ignored**.
Final focused registration checks pass with lookup both disabled and enabled;
the updated 690-cases-per-leaf corpus requirement also passes.
The head's combined native gate must include these opt-ins; L161's already
pinned generator gate intentionally excludes them (#13424). No agent game,
GPU or shard execution. Next: combined gate, then head's matched hash/lookup
A/B with fixed scene counts and all unrelated settings held constant.

Reproduce the original oracle via heavy.py and isolated gate:

```
python E:/ddda-re/tools/heavy.py codex-7 -- E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/record_job_lookup.py
python scripts/check_job_lookup.py --bench
```

SHA-256 receipts (all raw material stays private):

| Artifact | SHA-256 |
|---|---|
| h0 original trace | `6804199b37027679b142ade4ef6d70f0d330379046cee60c595f241fdb1eecfa` |
| h1 original trace | `c515d899df51c41497d05e4f832cfef71c8d8931f48e2be53d76531c132a9f65` |
| lookup original vectors | `f067c47144e1a470e62db339b27bc7e785fc47138241aee2ee0a8747d3c0e5e9` |
| 01008200 code range | `047be61631b6956b159fb5f412d5733fbeb8fb26f610de5062cc1804c4f1de99` |
| 01008260 code range | `0f4dfdb1bfa76ddc10602193c41e16b4df8400cebd21b90b8a6ae55aa4f4c3c2` |
| isolated benchmark log | `51231442d5f09faf9232103dca52ab8c4cbff2ec28274c52b9d47098545f8043` |
| final isolated gate log | `9e77aedb79f5cdd464a0e4a93b0bf12c0eb1c7264e80bd0f8675ba7369f6ec67` |
| light runtime log | `0b607308d44e84809bc4855916c212e17623edf6ea7f70a8c3710c8c162dad4d` |
| mtcore/ddports gate log | `680fa736b37631c78560da2d9c85dc6923fcb5e4d31f9120c1897db84a12f501` |
| c31-h0-ab.json | `0d14312ee3f28293629229c1edeeb0b1fa7af25b928b004c031d4ab948de8c58` |
| c31-h1-ab.json | `63e32ba017ccb1ebe7e7b1daa8abf1bd9d136a041483262a09e5fdb267cf2b24` |
| run_c31t0.log | `a359efd0d6fcaa24ee9683ab168bb46abc8690f78fad7b0194f1bf923eed3b1f` |
| run_c31t1.log | `7b966f6816e3062def3414c7a2c9c1f52c66ee4aa81ceb3cf3de793470fa7abc` |
| run_c31def.log | `3df851123feed6c29d40c2e3ce05790884d4232b27ad92f5eaff7d999eb7d785` |
| run_c31p8.log | `ce80693d3aa24b64b28de833d62ecb78c3c5cbb6839feda90c041c0760acee6d` |

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
Supplemental page-table replay passes under serial and atomic access
policies (four isolated tests pass, one benchmark ignored). Required-vector
`mtcore`/`ddports` tests with the override enabled report **18 passed,
zero failed, nine ignored**. The combined native gate and a matched
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
labels and frame filters, plus entry participant/configured counts for
L162's wake analysis. Those changes are not part of the original 973dbf3f
trace checkpoint. Final verification: **four focused runtime tests and
five parser tests pass**; the synthetic trace round-trips with one accepted
join. Tracing alone now enables the service hooks even without sync-profile.

## Results and handoff

Trace checkpoint: `973dbf3f`; hash candidate: `509e5636`. No game, GPU or
shard build was run by this agent. The exact original/lifted leaf gate and
synthetic speedup are established; the requested actual per-frame critical
path ranking and before/after whole-job durations require head's capture.
Head has the run request: corrected L153 census, `DDRUNTIME_SYNC_PROFILE=1`,
`DDRUNTIME_CRITICAL_JOBS=<private trace>`, matched hash=0/1 runs through the
same gameplay phase, with all other settings fixed. Keep the hash opt-in
until the combined native gate and that comparison pass.
