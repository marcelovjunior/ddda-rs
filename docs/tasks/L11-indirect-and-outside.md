# L11: non-switch indirect jumps and "execution outside the image"

Owner: Codex (same session). Branch `task/L11-indirect-and-outside` from the
head's `main` (b4c7eb4 or later), worktree `E:\ddda-rs-wt\L11`.

## Why

After L09: 41,516 verified (73.4% of functions, 85.8% of code bytes). What
is left in the lifter's reach:

- **971 lift rejections**, about 717 of them indirect jumps that are not
  switch tables (`jmp eax`, `jmp dword ptr [ecx + 0x14]`, ...). Most are
  tail calls through a function pointer or vtable. In ext mode a switch jump
  to an unknown target is already emitted as a tail call through
  `x86::call_ext`, mirroring the recorder's mock; a plain indirect `jmp`
  that leaves the body can be handled the same way (the recorder already
  mocks any branch that leaves the body).
- **1,373 recorder failures** without a vector, mostly "execution outside
  the image" (about 1,118). Find out what actually runs off: an indirect
  jump that is not a branch the recorder recognises, a `push X; ret` idiom,
  a callee whose mock is not triggered, or genuine random-pointer calls.

## Task

1. Support non-switch indirect jumps in ext mode as tail calls (exact:
   the target is read through `Mem` as the CPU would; known in-body
   targets, if any, continue in the function). Keep closed functions
   strict.
2. Trace a representative sample of the outside-image failures
   (`research/native/trace_one.py`, recorder logs), classify the causes,
   fix the recorder where it is wrong (never relax a guard that protects
   validity), and add input strategies only where they are justified.
3. Re-scan, record, adopt and report as in L09 (counts per cause and per
   fix, before/after totals).

## Rules

As in L09 (`docs/tasks/L09-structured-inputs.md`, "Rules" via L08). The
head seeds `LIFT_DATA=E:/ddda-re/notes/native-L11`.

## Results

Implemented ext-mode dword register/memory jumps as runtime tail calls.
The operand is read through `Mem`; any discovered in-body instruction start
remains a local destination, including when it was reached independently by
a direct branch. All such destinations are block leaders and x87-analysis
successors. No pointer-derived code is guessed. Closed-mode decoding remains
strict, with a separate body-cache key; both cache variants are released.
Ambiguous absolute switch forms still reject rather than bypass recognition.

**Recorder diagnosis and fixes.** Private instruction/ESP traces identify:

- Interleaved nonvolatile saves counted as arguments: `push edi` at
  `0x004065EA` follows a register load, so the old prologue heuristic had
  already ended. Its first mock popped four extra bytes. `0x0040D826`
  similarly contributes two saved registers/eight extra bytes. Recognize
  unmodified EBX/EBP/ESI/EDI saves with matching own pops on paths before the
  first call, including saves after an initial conditional branch
  (`0x0041B364`). This is still a mock ABI heuristic, not a callee contract.
- `static_pop` swept linearly through a tail jump at `0x00D009E6` and used
  the neighbouring function's `ret 8` at `0x00D00A11`. It now follows direct
  reachable control flow/tails, accepting a single consistent `ret N`,
  bounded to 512 instructions; unknown/indirect/inconsistent exits use zero.
- Indirect calls followed by immediate caller cleanup were popped twice
  (`0x0040846C` and `0x004085F1`). Subtract that explicit cleanup from the
  inferred callee pop. Those larger functions still have other failures.

The chosen mock pop is recorded and replayed as before. No return-address,
stack, page, instruction/time, flags, floating-point or x87 guard was relaxed;
no original instruction or input already consumed is changed. No new input
strategy was needed. Accepted baseline fixtures retain their own recorded
mock behavior, even where the recorder would now choose a different mock.
`trace_recorder.py` provides bounded JSON instruction/ESP/ownership and mock
transfer diagnostics without a Rust build; `trace_one.py` remains the tool
for comparing a usable fixture's Rust block trace.

An initial 12-function trace sample reproduced outside-image returns. Ten
pass the same seed after the fixes; the remaining two still reject. A second
sample selected 60 addresses evenly across the old outside-failure list,
with `objects-v1`, seed index 1, and a three-second bound per trial. It ran
the old recorder, then each fix cumulatively on identical original code:

| Recorder | Passing | Outside-image | Other guarded rejection |
|---|---:|---:|---:|
| Before | 0 | 57 | 3 |
| Saved-register recognition | 27 | 30 | 3 |
| Plus reachable tail cleanup | 44 | 13 | 3 |
| Plus explicit caller cleanup | 44 | 13 | 3 |

The tail-cleanup stage unlocks 18 samples and exposes one previously passing
sample's remaining guarded failure (net +17). Explicit caller cleanup has
zero additional full-function successes in this sample; its stack behavior
is covered by synthetic regression. These are controlled sample counts, not
an extrapolation assigning every remaining failure to a cause. Of the 57
original outside-image failures, 53 immediately followed an own `ret` and
four followed a mocked tail return; none in this sample was an unrecognized
outward branch or `push X; ret` idiom. After the fixes the corresponding
counts are 12 and one. Their underlying stack/ABI causes remain unresolved.

**Scan.** The current expanded inventory yields 1,378 missing liftable
candidates before the change, plus 971 rejections and four quarantines.
This includes all 1,373 prior L09 failures plus five supplementary entries
now present in the scan selection (`0x00E21800`, `0x011574D0`, `0x01158610`,
`0x012BAED0`, `0x012BAFC0`). After the change: **1,754 candidates / 595
rejections**, with no old candidate lost. Indirect-jump rejections drop
**738 -> 336**; 25 newly decoded bodies instead reject for x87 inputs and
one for INT3, giving **376 net new liftable functions**. The 336 remaining
indirect rejections are 333 unrecognized switch remappings and three bound/
table disagreements. Original decoded bodies are released after every scan
attempt; the prepared accepted selection/closed boundary is preserved.

All input passes use three deterministic seeds, 20 seconds per function,
six workers recycled after eight functions, and GC after every seed. Gains
are disjoint and depend on this pass order:

| Strategy | Tried | Added functions | Added cases |
|---|---:|---:|---:|
| Random | 1,754 | 1,088 | 3,188 |
| Zero | 666 | 178 | 534 |
| Small words | 488 | 2 | 5 |
| Zero objects | 486 | 0 | 0 |
| Mixed objects | 486 | 1 | 2 |
| **Total** | | **1,269** | **3,729** |

Of those additions, **371** come from newly supported indirect-jump bodies
and **898** from previously liftable candidates through recorder fixes and
retries. Five newly liftable indirect-jump functions still lack fixtures.

| Measure | Before | After |
|---|---:|---:|
| Selected lifted functions | 41,512 | 42,781 |
| Lifted cases | 123,574 | 127,303 |
| Inventory-verified functions | 41,516 | 42,785 |
| Verified share of current 56,577 non-thunks | 73.4% | 75.6% |
| Verified summed code bytes | 13,306,640 | 13,569,344 |
| Verified share of summed code-size estimates | 85.8% | 87.5% |
| Lift rejections in missing-entry scan | 971 | 595 |
| Missing candidates with no fixture after all passes | 1,378 | 485 |
| Historical no-vector cache | 1,370 | 477 |

The five supplementary entries raise the current inventory denominator from
56,572 to **56,577** non-thunks (15,514,319 summed code-size bytes). None of
the additions overlaps an already verified hand port, so the inventory gains
the same 1,269 functions. All 41,512 baseline function records and 123,574 baseline cases are retained
unchanged. The no-vector cache remains a historical subset, not the full
485-entry census. Among the final remainder, primary rejection reasons are
outside-image execution 286, runaway 83, NaN/Inf 77, and other guarded
failures 39. These are final-strategy outcomes; the before/after candidate
universes differ by 376 newly liftable entries.

`sBbsRpgMain::vf06` at `0x0041DD60` now has fixtures, as do the traced
`0x004065E0`, `0x0040ECA0` and `0x0041B350`. Initializer `0x0137E490` still
lacks a usable fixture. This is sampled per-function comparison with
external mock boundaries, not a working frame or game boot.

**Validation:** all 46 native Python tests (six new L11 tests, one updated
closed/ext expectation) and seven supplementary-inventory tests pass:
**53 Python tests**. Synthetic cases cover register/memory tail targets,
local indirect destinations, closed rejection, interleaved/conditional
saves versus arguments, immediate caller cleanup, direct tails versus
neighbouring returns, inconsistent ret cleanup, and invalid-return rejection.
The addition/dependency subset passes **46,098 cases / 15,455 functions**
plus seven Rust units: 51.77s compilation, 12.99s replay, 66.02s monitored
command total, 22.17 GiB minimum observed free memory.

One final `DDNATIVE_REQUIRE_VECTORS=1 cargo test -p ddnative` passes all
**32 Rust tests**, including **127,303 lifted cases / 42,781 functions**.
The legacy hand-port tests retain their existing fixture-dependent behavior;
required lifted replay uses private L11 data and cannot skip. Compilation
is **4m20s**, lifted replay **57.42s**, with **324.06s** monitored total and **16.26 GiB minimum free
RAM**. Four Cargo jobs, incremental off; no build/vector overlap. Vector
passes stayed above **23.12 GiB** free with six workers. The 6 GiB stop
threshold was never crossed. No live-game comparison was performed.

**Handoff:** matching `patterns/lift.json` / `vectors/lift.json`, baseline
and complete backups, before/after candidates/rejections, per-strategy
JSONL/adoption lists, 60-function ablation traces, remaining-candidate report,
private inventory and resource logs live under `E:/ddda-re/notes/native-L11`.
Head can adopt the accepted pair and regenerate on main, then rebuild the
shared inventory through its normal process. Source/tests/docs only are
committed; generated full-build files remain uncommitted. No shared native
or inventory writes, native gate, Ghidra restart, runtime code, or push.

**Open:** 485 recorder failures, 595 lift rejections, and the four existing
quarantines (`0x00C1D790`, `0x00D9EF20`, `0x00FB9A10`, `0x00F276C0`) remain.
Next action: review/adopt the pair, then inspect the 333 unrecognized switch
remappings and representative residual mock-stack failures without relaxing
the recorder's guards. No quarantine was reintroduced.
