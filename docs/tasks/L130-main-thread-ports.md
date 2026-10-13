# L130: main/job thread readable ports

Assigned to codex-10 by head/user, 2026-10-13. Branch
`task/L130-main-thread-ports`, based on `task/fps-line`.

## Scope

Select hot whole game functions on the main thread and workers 1070,
1074 and 1078, translate to readable Rust, verify original Unicorn and
isolated lifted execution including machine state at call boundaries,
register through ddports, and benchmark recorded inputs. Complement L129
dead-flag elimination. No game runs, shard builds, or game data committed.

Files: mtcore `hot.rs`, module registration, `tests/hot.rs`, ddports registry,
`research/native/{record,check}_main_thread.py`, this brief. Milestones
currently owned by codex-7: send a merge-ready entry to head.

## Profile evidence and limits

Private source: `E:/ddda-re/notes/runs/profile_c15b.txt`, combo15,
364 rounds over 30.1 s. Busy samples: main 257, workers 173/186/170.
Main measured CPU: 19.0 s (63%); busy sampling: 70.6%. These are different
measurements, not interchangeable percentages. Workers measured CPU:
14.0/13.6/13.2 s (46/45/44%).

All nearest-lifted sections say 100% no lifted frame even though the
inclusive rows contain Rust v0 symbols. Thus helper costs cannot be
assigned to leaf functions from this summary. Requested corrected and
longer capture in board #11386; L131 delivered matcher/retained-stack
tooling in `63fd9c23`, new head capture pending.

Main inclusive leaders: DBE560 55.6%, DBD8D0 49.8%, DB0340/DAF000 24.9%,
DB63E0 24.1%, DC4770 21.0%, DB7620 20.6%, 11787C0 19.1%.
Worker DB7620 inclusive: 22.0/37.1/18.8%; 11787C0:
13.9/9.1/11.8%. These overlap: **never sum them as removable work**.

Read-only Ghidra checks:

- DBD8D0 distributes jobs using InterlockedIncrement and invokes callbacks.
- DB7620 selects a virtual update call or another update helper.
- 11787C0 dispatches geometry types to intersection helpers.
- FEE320 builds/copies render state and calls several allocation/shader
  helpers; its worker inclusive 9.2/9.1% does not establish self cost.
- E0EC50 walks packed dependencies, with five unrolled levels followed
  by recursion. Its worker **self** samples are 1.7/1.1/2.9% (3/2/5
  samples), a candidate signal only. Memory/flag helper attribution and
  main-thread share remain unknown.

Head #11403 clarifies DBE560 is job fan-out plus own DBD8D0 work and a
WaitForMultipleObjects join. Main waits on workers; render waits on main.
Eliminating worker work can matter even without measurable main self cost.

## E0EC50 implementation and verification plan

`mtcore::hot::expand_dependencies` preserves original unrolled traversal,
per-iteration descriptor-count reloads, output ordering, conditional EDI
save, stack spills, overwritten argument slots, recursive return address
E0EDD6 and call dispatch. No guest tables are copied into the port.
Only flags overwritten before any call/return are omitted; ADD ESI before
recursion and ADD ESP before return materialize the observable flags.

The recorder maps this original function and synthetic data only. Trees
cover zero/one/multiple children, depths beyond two recursive boundaries,
zero/nonzero packed/terminal values, 63 terminals, randomized untouched
bytes, and output/value-table overlap. Every recursive entry/return and
final state carries registers, XMM, flags, MXCSR, unchanged x87 state and
written memory. Rust additionally checks the complete write set. The
isolated runner extracts the existing lifted body into private scratch;
it does not build any ddlift shard.

## Results

Implemented and registered E0EC50 through `mtcore::hot::PORTS` in ddports.
The separate catalogue preserves the existing core-edge corpus coverage
assertions. Head authorized bounded recording during L129 in #11469 and
selected this port for the combined integration in #11483.

- **112 original Unicorn cases / 1,674 recursive entry/return boundaries**
  pass for both the readable port and the unmodified extracted lifted body.
  **60/60 conditional branch outcomes** exercised; the recorder now rejects
  a corpus missing any. Includes 63 children, 63 terminal values, depths
  0..12, future-value output overlap, randomized GPR/XMM and eight nonempty
  x87 sentinels. All written bytes (including stack), registers, arithmetic
  flags/DF, XMM, x87 and MXCSR match at each boundary and final return.
- `cargo test -p mtcore -p ddports` with required private vectors:
  **9 tests pass / 1 benchmark ignored**, including the existing core edge
  and lifted corpus. Latest isolated replay passes after strengthening
  x87 sentinels and overlap fixtures.
- Runtime light suite: **182 pass / 1 existing fullscreen-gamma failure /
  3 ignored**. First run hits gamma; second explicitly filters that one
  test to complete the remaining suite. No checks weakened.
- `cargo clippy -p mtcore --test hot --no-deps -- -D warnings` passes.
  An initial strict transitive run stops at 11 existing ddnative-base lints.
- No shard build, GPU test, game run, image-equivalence or FPS claim.

Recorded synthetic input microbench: same atomic backed-page memory path,
lifted body opt1, port and base helpers opt3; 20 warmups, 1,000 calls per
body per round, three alternating rounds. Input memory/CPU restored outside
the timed call, original final-state gate before timing. Recursion executes
the respective implementation on both sides. Latest median ns/call:

| Depth / branching / terminals | Lifted | Port | Ratio |
|---|---:|---:|---:|
| 0 / 1 / 0 | 157 | 145 | noisy, no claim |
| 0 / 1 / 3 | 307 | 180 | 1.71x |
| 1 / 2 / 3 | 608 | 341 | 1.79x |
| 3 / 2 / 3 | 3,193 | 1,183 | 2.70x |
| 5 / 2 / 3 | 16,972 | 7,936 | 2.14x |
| 12 / 2 / 3 | 43,013 | 15,511 | 2.77x |

Shared-machine scheduling matters: the earlier three-round pass gave
0.584 -> 0.201 us at depth1, 3.240 -> 0.704 us at depth3,
12.453 -> 5.455 us at depth5, 37.483 -> 13.957 us at depth12.
These are microbenchmarks, not gameplay inputs or frame-time predictions.

**Expected main-thread share removed: not yet quantifiable.** The old
summary cannot attribute memory/flag helpers to E0EC50; its ten worker
self samples do not bound total cost. For a measured exclusive busy share
`s` and a representative speedup `r`, the potential CPU share saved is
`s * (1 - 1/r)`. The current loop-heavy microbench ratios suggest 44..64%
of this function's attributable cost, but neither `s` nor the gameplay
input mix is known. Main scheduling and worker balance also limit FPS.

Reproduce (research Python for recording; system Python for isolated build):

```
E:/ddda-re/tools/pyenv/Scripts/python.exe E:/ddda-re/tools/heavy.py codex-10 -- E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/record_main_thread.py
python research/native/check_main_thread.py
python research/native/check_main_thread.py --bench
```

Private vectors: `E:/ddda-re/notes/native-L130/dependencies.json`.
Logs: `C:/ddda-targets/L130/{record,differential,bench,core-tests,runtime-light,runtime-light-rest}.log`.
Next action: head's combined integration gate and gameplay run, followed by
the retained-stack L131 profile to select the next ports and quantify share.

## Continuation: geometry dispatcher 11787C0

Rebased onto `task/fps-line` (ebd50da3) after E0EC50 adoption. DB7620 was
inspected again: it is a small two-way update dispatcher, with no measured
self-cost justification for a separate port. Continued the previously
identified 11787C0 candidate while awaiting `profile_c17draw.json/.txt`.

Whole `dispatch_geometry` reads the original two dispatch tables and SIMD
masks at runtime. It preserves all 14 call targets, their return addresses,
the aligned frame, the AABB temporary and scratch writes, XMM lanes and
observable CMP flags. The AABB path performs bitwise operations only;
there is no float reassociation or guessed geometric behavior. Registration
is through the existing `mtcore::hot::PORTS` / ddports chain.

- **2,080 original Unicorn cases and isolated lifted replays pass**, with
  **1,120 callee entry/return boundaries**, every one of 14 callees and
  **6/6 conditional outcomes**. Inputs include tags 0..23, FFFFFFFF and
  80000000; five mode words (testing low-byte semantics), all 16 stack
  alignments, arbitrary SIMD masks/object bits and nonempty x87 sentinels.
  Exact registers, arithmetic flags/DF, XMM, MXCSR, x87 and complete written
  memory match. Callees are explicit synthetic ABI effects, not a claim
  that their intersection implementations were executed or ported.
- Original recipes and comparison runner remain in the existing L130 files.
  Private vectors: `E:/ddda-re/notes/native-L130/geometry.json`.
  Commands add `--geometry` to recording and `--bench --geometry` to
  `check_main_thread.py`; ordinary check runs both function suites.
- Atomic-page opt1 lifted / opt3 port medians, five alternating rounds of
  10,000 calls (ns/call): default **85.5 -> 59.1**, type2 **256.8 -> 233.3**,
  AABB normal **436.9 -> 416.8**, AABB alternate **450.9 -> 441.6**, type8
  **267.1 -> 231.5**, type21 **273.9 -> 237.2**. Callee replay overhead is
  included equally. Shared CPU outliers and small AABB deltas prevent a
  strong AABB speedup claim; no gameplay/FPS claim. First implementation
  used generic stack helpers and regressed; final uses the same page-table
  stack accessors as the generated code and passes the full oracle again.
- **Expected share removed remains unquantified**: old inclusive shares
  (main19.1%, workers13.9/9.1/11.8%) include the large intersection callees,
  which this port does not accelerate. The fresh nearest-self profile and
  actual tag distribution are necessary for a responsible estimate.

Local `cargo test -p mtcore -p ddports`: **10 pass / 2 benchmark ignores**,
including the existing core original-edge and lifted-vector suites. Strict
`cargo clippy -p mtcore --test hot --no-deps -- -D warnings` passes.
Logs are `C:/ddda-targets/L130/geometry-{core,clippy,final-differential,final-bench}.log`.
No shard, game or GPU build/run. Next: head's combined gate/gameplay check
and the corrected profile, then port the largest exclusive main/job costs.
