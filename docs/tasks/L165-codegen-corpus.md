# L165 codegen benchmark corpus

Owner codex-6, branch `task/L165-load-next`. Assignment: head board #13696.
Build a 50-function real-input yardstick for generator/memory changes. This
supersedes the remaining worker ports; completed checkpoints are documented
in `L165-worker-speed.md`. Only the head runs the game or builds shards.

## Implemented

`scripts/worker_corpus.py rank` reads c26 raw stacks and c31 text reports.
It selects main plus worker threads 0x1070..0x1084, excludes recognized OS
wait leaves, retains unknown leaves in the busy denominator, and attributes
split `f_*_pN` bodies to their entry. It reports direct self, nearest ownership
(including host helpers), and inclusive counts separately. Ranking score is
literal self + inclusive, with nearest counts as the tie-breaker. Inclusive
counts overlap and are not CPU seconds. Text reports are truncated lower
bounds; no reconstruction of missing raw stacks is claimed.

This ranking includes scheduling/process/frame wrappers. Some will not
return in a bounded capture, exceed the event budget, or depend on concurrent
writes. Those rows must stay explicitly unavailable. A leaf-oriented subset
may be useful, but is not silently substituted for the requested ranking.
`--metric nearest` emits that separately labeled alternative. The c31
inclusive truncation particularly under-ranks leaf kernels: FEDD60 has 90
nearest samples but only 41 reported inclusive observations in the inputs.

The `run` command extracts just the 50 bodies (including split parts) into
two private crates, baseline and candidate, both at **opt-level 1**. The
common base/host and harness use opt-level 3. It never builds ddlift shards.
It prints one row per ranked function and writes private body hashes, raw
timings and a coverage report. Common-base selection means this isolates
generated-body changes; it does not by itself A/B two base implementations.
`--candidate-bodies PRIVATE_DIRECTORY` accepts isolated generator output
files `f_XXXXXXXX.rs`, including any split parts. This avoids regenerating
or compiling complete shards merely to try a generator change.

Real CPU84/Mem transcripts use the schema of codex-11's `shadow_fee320`
recorder (`711b9576`). The converter:

- Requires `origin=runtime-lift`, capture mode and complete nonpanicking
  records. Synthetic fixtures never count toward real coverage.
- Builds initial bytes, final writes and opaque external-call contracts.
  Nested contracts collapse into the direct caller's seam.
- Rejects reads inconsistent with prior observed writes, including across
  callees; these imply concurrency or unobserved service effects.
- Rejects caller-side allocation/probe contracts and more than 64 MiB of
  touched pages. These are diagnostic limits, not game behavior changes.
- Checks callee-entry CPU and all observed memory, then applies recorded
  callee writes and return CPU. The callee implementation is not timed or
  claimed equivalent by this caller benchmark.

The isolated Rust harness checks both variants using guarded sparse Mem
and atomic page-backed Mem, comparing return CPU, defined arithmetic flags
and their validity, XMM, saved/live MXCSR, x87 state and memory. Raw host
x87 FIP/FDP words alone are excluded because ASLR changes them; guest
FNSAVE supplies original metadata. Page replay checks entire mapped pages,
including untouched poison padding; guarded replay rejects reads before
captured initialization and writes outside recorded destinations.

Timing uses page-backed memory only, seven alternating baseline/candidate
pairs per case. Input reset and CPU restoration occur outside the timer.
Both inclusive wall nanoseconds and time excluding opaque replay seams are
saved; table ratios use the latter. Cases receive equal weight, not inferred
gameplay frequency. Host scheduling/timer overhead remains; no FPS claim.

## Capture prerequisite

The current shared recorder observes `call_ext` only. Generic corpus capture
also needs `call_local` and `call_lifted` boundaries; otherwise an extracted
caller would request a seam that was never recorded. Replay fails on such a
missing/incorrect seam. It never fabricates a callee result. Head/shared
owner coordination is on board #13743/#13768/#13802. A normal-build overhead
change is not justified just to enable diagnostics; a capture-only feature
is the proposed hook.

## Commands

All outputs must be outside any Git worktree:

```
python scripts/worker_corpus.py rank E:/ddda-re/notes/runs/stacks_c26vk.json E:/ddda-re/notes/runs/profile_c31def.txt E:/ddda-re/notes/runs/profile_c31p8.txt --out E:/ddda-re/notes/native-L165/corpus/rank50.json
python scripts/worker_corpus.py run --rank E:/ddda-re/notes/native-L165/corpus/rank50.json --captures E:/ddda-re/notes/native-L165/corpus/real --baseline-ref 4ae43a7c --candidate-root E:/ddda-rs-wt/L161
```

The build sets `CARGO_TARGET_DIR=C:/ddda-targets/L165`, incremental off,
four jobs. No game, GPU, Ghidra mutation or full native gate is invoked.
Missing/rejected/failing rows return exit code 2; only complete coverage
returns 0. Capture logs are never timings; this command times offline replay.

## Results

- Nine Python attribution/converter tests pass, including recursive frames,
  split parts, unknown leaves, waits, nested seams and incoherent histories.
- Initial private build compiles all 50 baseline + 50 candidate bodies.
- **Real coverage: 0/50.** No head capture has been supplied. Initial report
  correctly prints 50 MISSING rows and returns 2. This is tooling readiness,
  not completion of the requested real benchmark corpus.
- Final harness self-test passes: guarded/page memory, cross-page alias
  writes, a callee seam, timing and five deliberately corrupted cases
  rejected. These fixtures are explicitly synthetic and add no real coverage.
- Private final log `C:/ddda-targets/L165/corpus-final.log` SHA-256
  `b77dca5aa313a6ef0ecaa351d4e3c225f1f3ff89b412aea10644be2f5b5364e0`.
  Literal rank manifest SHA-256
  `51ba528b94805bb3a9c3abf5450411e3009380af1b6f7793f8ed68a7998cf528`;
  alternate nearest manifest SHA-256
  `43c8aa1f4c59570c65c6e9b33ed09a798077dc4615c916ff8d10b3c51d6dbbae`.

Next: agree the complete call-boundary hook with the shared owner, perform
the head-only capture, then reject incomplete histories and replay/time all
eligible real calls. Never replace missing real inputs with synthetic ones.
