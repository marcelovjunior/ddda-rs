# L12: the remaining lift rejections

Owner: Codex (same session). Branch `task/L12-remaining-rejections` from the
head's `main` (6708185 or later), worktree `E:\ddda-rs-wt\L12`.

## Why

After L11: 42,785 verified (75.6% of functions, 87.5% of code bytes). The
missing-entry scan still rejects 595 functions: 333 switch tables in an
unrecognised remapping form, three bound/table disagreements, x87 inputs a
mocked callee would have to provide (172 with the L11 additions), int3,
approximate reciprocals (rsqrtps/rcpps), segment overrides and a few
singletons. 485 liftable functions still have no fixture (outside-image
286, runaway 83, NaN/Inf 77, other 39).

## Task

1. Classify the 333 unrecognised switch remappings by shape and support the
   common ones strictly (bounds proven, every target an instruction start
   in the body), as in L01.
2. x87 inputs from mocked callees: where the caller consumes st0 that a
   mocked callee must have returned, the mock can push a value (L04's
   `x87_push` contract covers the immediate case). Extend it where the
   consumption is later in the block or after other x87 work, exactly and
   statically, or document why a case cannot be handled.
3. Look at the remaining single-cause rejections (int3 as a noreturn trap,
   `cpuid`/`rdtsc`/`pushfd` as platform-dependent: mock them through
   `call_ext`-style events, or reject with a reason) and the recorder
   failures with `trace_recorder.py`; fix only what keeps every guard.
4. Re-scan, record, adopt and report as in L11.

## Rules

As in L11 (`docs/tasks/L11-indirect-and-outside.md`, "Rules"). The head seeds
`LIFT_DATA=E:/ddda-re/notes/native-L12`.

## Results

(Codex fills this in.)
