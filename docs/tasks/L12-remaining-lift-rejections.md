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

Implemented cross-register byte switch remapping, delayed mocked x87 returns,
terminal INT3/UD2 traps, LEAVE, PINSRW/PEXTRW and REP RET. CPUID/RDTSC/PUSHFD
now explicitly reject without a platform event/state contract. No shared data
was written and no recorder acceptance guard was relaxed.

**Switch classification.** All 333 unknown remaps use
`cmp input,N; ja/jae default; movzx output,byte [input+TABLE];
jmp [output*4+CASES]`, or the suffix without a proven bound, with distinct
input/output registers. The largest groups are ECX from EAX (147), EDX from
EAX (88), EDX from ECX (54), and ECX from EDI (12); the other 32 use 13 other
register pairs. The input bound sizes the byte table, whose maximum sizes
the dword table; every case must remain a decoded instruction start. No
table contents are embedded in source. Four unbounded suffixes remain
rejected (`0x0044D1F0`, `0x004593E0`, `0x004594A0`, `0x00686AD0`), along with
three existing bound/table disagreements (`0x00561040`, `0x00B5A030`,
`0x00D96F50`). Of 329 remap rejections removed, one exposes another
unsupported instruction: **328 net new liftable switch functions**.

**x87.** `returns_float` starts from the current x87 depth, follows supported
x87 effects and direct control flow, and identifies a deficit of one value.
It stops at another call, unknown indirect jump, return or trap. For example,
the call at `0x00458634` is followed by FILD and JNS before FMULP at
`0x0045864B` consumes its return; `0x004588CF` has the same delayed pattern.
These two bodies now lift but still lack usable fixtures. The existing
`x87_push`/`st_push` trampoline contract records and replays the value.
Multiple missing values and overflow still reject. This unlocks 148 bodies,
140 of which acquire fixtures. Of the residual x87-input rejections, 25
contain non-switch indirect jumps conservatively targeting every discovered
instruction start (e.g. `0x006051C0`, `0x00605210`, `0x00CF91D0`): an apparent
jump directly to FSTP can underflow. `0x012F05C0` consumes caller-provided st0
at `0x012F05D2`, without a mocked producer. Two other cases now explicitly
reject unsupported FLDLN2 (`0x00E22520`) and FLDL2E (`0x010359F0`).

**Singletons and recorder.** INT3/UD2 terminate decoding and emit traps;
fixtures taking them reject through the existing noreturn guard. No SEH
behavior is invented. LEAVE restores ESP/EBP, PINSRW/PEXTRW select the word
with the low three immediate bits and preserve the other lanes, and REP RET
is recognized by both emitter and recorder. Approximate reciprocals, segment
overrides and SFENCE retain explicit unsupported behavior.

An instruction/ESP trace of `0x008AA2D0` identifies `sub esp,8` at
`0x008AA2D5` as delayed local-frame allocation counted as outgoing arguments:
the mock popped it and the own epilogue popped it again. `0x00782D15` allocates
12 bytes with the same problem. Exclude a straight entry-prefix allocation
only when matching ADD ESP immediately precedes an own return and no store
or further ESP adjustment except pushes intervenes before the first call.
This is a mock ABI heuristic, not a recovered callee contract. The traced
`0x008AA2D0` objects-v1 seed 1 now succeeds; both functions acquire fixtures.
Retrying the same random seeds on the 580 failed functions after this fix
adds 14 functions/34 cases. All return-address, stack, memory, instruction,
time, flags and floating-point guards remain unchanged.

**Scan and recording.** The initial scan found 501 missing liftable functions
and 595 rejections; the final scan finds 1,010 candidates and 86 rejections,
with no old candidate lost. The initial 501 comprise the prior 485 plus 16
new supplementary inventory entries. Net new liftable bodies: switches 328,
x87 148, INT3 27, LEAVE 2, PEXTRW 2, REP RET 1 and PINSRW 1 (total 509).

Three deterministic seeds, 20 seconds per function, six recycled workers;
gains are disjoint and depend on pass order:

| Strategy | Tried | Added functions | Added cases |
|---|---:|---:|---:|
| Random, before local-frame hint | 1,010 | 430 | 1,232 |
| Random retry, with local-frame hint | 580 | 14 | 34 |
| Zero | 566 | 67 | 201 |
| Small words | 499 | 2 | 2 |
| Zero objects | 497 | 0 | 0 |
| Mixed objects | 497 | 0 | 0 |
| **Total** | | **513** | **1,469** |

The additions comprise 306 switches, 140 x87 cases, 20 INT3 bodies, two
LEAVE, two PEXTRW, one REP RET, one PINSRW, and 41 previously liftable
functions. All 42,781 baseline records and 127,303 cases remain unchanged.
The final 497 candidates without fixtures have first-attempt mixed-object
reasons: outside-image 287, runaway 88, NaN/Inf 78, other guarded failures 44.
The historical no-vector cache drops 477 -> 450; it is not the full census.
Initializer `0x0137E490` still lacks a usable fixture.

Final scan rejections: x87 input 26; RSQRTPS 23; segment overrides 9; RCPPS 9;
indirect jumps 7; platform-dependent 4; SFENCE 3; unsupported x87 2;
disassembly overlap 1; x87 overflow 1; RCPSS 1. Four existing quarantines
remain unchanged (`0x00C1D790`, `0x00D9EF20`, `0x00FB9A10`, `0x00F276C0`).

**Validation and totals:** all 56 native
Python tests and seven supplementary-inventory tests pass (63 total).
Ten new tests cover remap source bounds/targets, delayed and branched x87
returns, intervening calls, entry arguments, traps, explicit platform
rejection and frame allocation versus argument stores. Twelve additional
synthetic Unicorn/Rust programs cover word lanes, masked immediates, a
memory-word source, LEAVE and REP RET.

The addition/dependency subset passes **43,838 cases / 14,699 functions**
and seven Rust units: 56.60s compilation, 11.60s replay, 70.02s monitored
total, minimum free RAM 21.93 GiB. One final
`DDNATIVE_REQUIRE_VECTORS=1 cargo test -p ddnative` passes **32 Rust tests**,
including **128,772 lifted cases / 43,294 functions**. Compilation takes
4m13s, lifted replay 58.15s, monitored total 318.06s, minimum free RAM
14.29 GiB. Existing legacy tests retain their fixture-dependent behavior;
required lifted replay cannot skip. Four Cargo jobs, incremental off, no
build/vector overlap. All six-worker vector passes stayed above 23.58 GiB
free; the 6 GiB stop threshold was never reached.

| Measure | Before | After |
|---|---:|---:|
| Selected lifted functions | 42,781 | 43,294 |
| Lifted cases | 127,303 | 128,772 |
| Inventory-verified functions | 42,785 | 43,298 |
| Verified share of functions | 75.6% | 76.5% |
| Verified summed code bytes | 13,569,344 | 14,081,075 |
| Verified share of summed code-size estimates | 87.5% | 90.8% |
| Missing-entry scan rejections | 595 | 86 |
| Liftable candidates without fixtures | 501 | 497 |

The 16 supplementary entries increase the current denominator from 56,577
to 56,593 non-thunks (15,516,093 summed code-size bytes). No addition overlaps
an already verified hand port. Coverage is sampled original-instruction
comparison with external mocks, not a game-boot or live-game comparison.

**Handoff:** the matching accepted pair, complete/baseline backups, candidate
and rejection lists, per-strategy logs, local-frame trace and resource metrics
are under `E:/ddda-re/notes/native-L12`. Head can adopt the pair and regenerate,
then rebuild the shared inventory normally; the private database alone was
rebuilt here (`scripts/inventory.py build`, then `summary`, with the usual
shared inventory destination on head's main after regeneration). Source/tests/docs
only; no generated code, runtime code, shared
native/inventory writes, native gate, Ghidra restart or push.
