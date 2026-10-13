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

## Corrected c17 profile and CD8760 port

Head's `profile_c17draw.json/.txt` contains 1,756 rounds over 60.024 s.
Main: 1,064 busy samples / 31.1 CPU seconds. Worker busy samples are
346/973/182 and CPU seconds 24.7/22.3/21.8; the markedly different busy
fractions are sampling/classification observations, not CPU utilization.

Retained stacks expose two important limits of nearest-*lifted* ownership:

- E10FC0's 122 main/job samples include **91 inside the readable E0EC50**
  port (main25, workers9/41/16). Only 31 remain wrapper/helper samples.
  Reported to head and L131/L132 owner in board #11592.
- 112D970's 73 main/job samples include substantial emulator/x87 state
  overhead. `run_c17draw.log` confirms unbound duration callees BB40/60/
  80/A0/C0/E0, called at 112DA14. These six entries are the next worker
  target, not an unmeasured rewrite of the animation wrapper (#11631).
- 11787C0 has **two** nearest samples (main1, worker1074 one). Its expected
  benefit is correspondingly tiny in this scene; old inclusive19.1% was
  never a removable-share estimate. The exploratory 5D0F50 source/recipe
  was stashed when c17 arrived and is neither registered nor committed.

`resolve_identifier` ports **CD8760**, the main thread's largest
non-scheduler nearest-lifted function: **40/1,064 = 3.76% busy samples**.
It preserves EAX/ESI input convention, signed/range guards, the count and
indexed-entry virtual calls, changing vtables/counts, partial identifiers,
INC carry and exact stack/flags at every boundary. The unreachable
FFFFFFFF equality arm remains excluded by the original negative guard.

- **216 original Unicorn + extracted lifted cases pass**, with **3,640
  callee entry/return boundaries**. Every feasible conditional edge is
  covered (only CD876C -> CD878A is unreachable). Includes null entries,
  type and id mismatches in both ordering directions, first/last matches,
  count growth/truncation, vtable replacement, 0/1/8/32-entry scans,
  high-bit identifiers, randomized registers/XMM and x87 sentinels.
- All three isolated function suites pass. ddports registry test and
  strict own-crate clippy pass. No full shard/game/GPU run.
- Five alternating 5,000-call opt1/opt3 atomic-page medians on the second
  pass (ns): rejected id **54.5 -> 43.2**, one miss **413.2 -> 330.7**,
  one hit **364.5 -> 291.8**, eight-entry hit **1,792.1 -> 1,688.6**,
  32-entry miss **6,618.1 -> 6,300.7**, 32-entry hit **6,758.7 -> 6,540.7**.
  First pass eight-entry results regressed (1,254.9 -> 1,725.3), and
  32-entry hit gained more (6,445.7 -> 5,010.9). Shared-machine noise and
  common synthetic callback replay cost mean no reliable large-loop gain.
- **Expected share:** the latest 3..21% synthetic reduction applied to
  the 3.76% sampled main busy share would suggest roughly **0.1..0.8
  percentage points**, conditional on a matching call mix. This is a
  planning estimate only; the first-pass regression means **no positive
  gameplay saving is yet established**. Do not use total frame time or
  worker CPU utilization as the denominator for this estimate.

Private vectors/logs: `native-L130/identifiers.json` and
`C:/ddda-targets/L130/identifiers-{record,differential,bench,bench2,clippy,ddports}.log`.
Record with `--identifiers`, benchmark with `--bench --identifiers`.
Next: exact ports for six emulated animation-duration decoders; head owns
combined native gate and same-scene gameplay comparison.

## Worker hotspot: six emulated animation duration readers

Whole ports for **112BB40, 112BB60, 112BB80, 112BBA0, 112BBC0 and
112BBE0** are registered through `mtcore::hot::PORTS` / ddports. These
entries lack Ghidra functions and existing generated bodies; source is the
original instruction bytes, decoded with Capstone. The animation format
table at 1441CF0 selects them. They load a u16, one of two u8 fields, or a
packed high nibble, overwrite the argument slot, and execute FILD m32int.
Partial CX/CL writes, untouched registers/flags and the x87 stack are retained.
No executable bytes or table contents are committed.

The recorder runs the original instructions and separately asks the existing
lifter for **six isolated Rust bodies in private scratch**. It does not
change the inventory, patterns, canonical project or generated shards.

- **4,224 cases pass** for ports and isolated lifted bodies: all six
  entries, boundary/random packed fields, every rounding mode, rotated TOP,
  depth 0/3/7/8 and randomized GPR/XMM/flags. Exact written bytes, GPR/XMM,
  defined arithmetic flags/DF, MXCSR, x87 CW/SW/tags and all eight extended
  registers are checked. SHR >1's architecturally undefined AF/OF are
  explicitly excluded; other functions retain their existing flag checks.
- **3,168 valid-stack cases agree between Unicorn and host FILD.** The
  **1,056 full-stack cases expose a Unicorn defect**: it overwrites ST0
  rather than raising the masked stack fault and returning indefinite.
  Both observations remain in the private vectors; the original FILD
  instruction executed on hardware supplies those cases' x87 oracle.
  FSAVE's reserved words are checked against the existing hardware contract.
- All four isolated suites pass (6,632 total whole-function cases,
  6,434 recorded call/return boundaries in the three calling functions).
  Focused mtcore hot tests **4 pass / 4 benchmark ignores**, ddports **1
  pass**, strict own-crate clippy passes. Earlier complete core suite
  remains green. No shard or game run, no FPS/image claim.

Before/after benchmark uses **the actual ddruntime Unicorn fallback** on
GuestMemory, with only the six original leaf code bodies and synthetic
data mapped. No game session or window. Runtime/base/port opt3, five
alternating rounds, 20 warmups + 1,000 measured calls per body/round;
same input restoration outside timing and full oracle gate before timing.
The warmed pool is reused. Fresh synthetic CPUs explicitly release their
x87 identities outside timing: otherwise the benchmark itself accumulates
stale spill entries (initial exploratory timings were invalid and discarded).

| Entry | Runtime fallback ns | Port ns | Speedup |
|---|---:|---:|---:|
| 112BB40 | 3,635.0 | 89.7 | 40.5x |
| 112BB60 | 4,319.9 | 88.1 | 49.0x |
| 112BB80 | 4,325.8 | 106.4 | 40.7x |
| 112BBA0 | 3,665.1 | 91.5 | 40.1x |
| 112BBC0 | 3,726.6 | 105.8 | 35.2x |
| 112BBE0 | 3,681.7 | 105.1 | 35.0x |

Isolated opt1 lifted/opt3 port medians are similar, **89..118 ns**; the
benefit is removal of emulation, not a claim of faster native arithmetic.
Microbenchmark memory is small; production fallback revalidation sees more
pages. No claim about the amount of process memory released by these ports.

**Expected share removed:** of 112D970's 73 nearest samples, **61 actually
contain younger ddruntime::emu frames**: main12, workers12/36/1. Removing
97..98% of that sampled cost suggests approximately **1.1 percentage
points of main busy work**, **3.4/3.6/0.5 points** of worker busy work.
This is conditional on the captured call mix and this benchmark; the
one-sample worker estimate is especially weak. It is not a whole-frame
or FPS prediction. Other emulated entries remain in c17's log: these ports
remove the six animation paths, not all gameplay emulation.

Reproduce: record `--durations`, ordinary `check_main_thread.py` for all
original/lifted comparisons, `--bench --durations` for isolated native
timing, and `--bench --emu` for actual runtime fallback versus port.
The last option adds `ddruntime` with **default-features=false, emu only**
to the private scratch crate; it never builds shards. Private vectors and
lift output are `native-L130/durations.json` and `.rs`; final logs are
`C:/ddda-targets/L130/durations-{final-differential,final-bench,final-runtime-bench,core,clippy,ddports}.log`.

Head's additional executor request (#11667): coordinated read-only with
claude-2/codex-7. Codex-7 isolated the failure to the process-global sink
being installed by a concurrent test (#11689/#11691), not an apply-port
merge regression. Agreed on a test-only mutex and RAII sink reset; codex-7
owns that fix. L130 makes no mtdraw changes.

Next action: head integrates the duration family, checks the six fallback
log entries disappear, and captures the same scene/profile; codex-7 owns
the combined gate and milestone file. Further ports should follow the
new remaining exclusive costs, accounting for readable callees.

## Session continuation: recording-state save (2026-10-13)

Rebased onto `task/fps-line` at `0d5ccfb8`; earlier three local commits are
now `6a71c3a2`, `fc799fd1`, `e3e976ca`. The user assigned the remaining c17
main-thread candidates and all six active job threads (1070..1084).
Recording-side placement proposed to claude-2/head in board #11737/#11772:
mtcore whole-machine ABI ports; no executor/sink changes.

**FEDB80** saves the variable-length recording descriptor and sort cursor,
then starts a nested state. Registered through `mtcore::hot::PORTS`, already
consumed by ddports. **48 original Unicorn and isolated lifted cases / 96
call-return boundaries pass**, including four depths, six descriptor counts
(0..4096), random untouched state and callbacks changing the depth/argument.
Checks cover complete written bytes, GPR/XMM, defined arithmetic flags/DF,
MXCSR and x87 state. IMUL's undefined SF/ZF/AF/PF are excluded only at its
callee-entry boundary. Memcpy is an explicit copying synthetic callback;
its ABI side effects are recorded/replayed, not claimed as an original
memcpy equivalence test. The isolated reference redirects that platform
call to the same checked callback, leaving the lifted body unchanged.

- Full core/ddports: **13 tests pass / 5 benchmark ignores**; isolated
  original/lifted suites: **5 pass / 5 ignores**, 6,680 total cases.
- Five alternating opt1-lifted/opt3-port rounds, atomic paged memory,
  1,000 measured calls each, bulk synthetic callback replay: median ns
  **553.1 -> 564.6** (count0), **619.5 -> 641.4** (count8),
  **854.0 -> 849.8** (count32). **No reliable speedup**; no FPS claim.
  First timings used byte-wise callback replay and mostly measured that
  harness overhead. Both versions gate the full final machine state.
- Logs: `C:/ddda-targets/L130/recording-{differential,core,bench2}.log`.
  Private corpus: `native-L130/recording.json`; regenerate with
  `record_main_thread.py --recording`, replay with `check_main_thread.py`,
  measure with `check_main_thread.py --bench --recording`, all via heavy.py.

The milestone file remains owned by codex-7; send the above evidence for
its merge-ready entry. Next: FEDC00 restore/record oracle and measurement,
then FED200 and the remaining assigned whole-function candidates. No game,
GPU or shard build/run in this continuation.

### FEDC00 restore and command recording

Whole ABI port registered through mtcore/ddports. **96 original Unicorn +
isolated lifted cases / 480 call-return boundaries pass**, including all
four conditional edges, depths 0..3, descriptor counts 0..4096, varying
pending draws, allocation-free/exact-fit/refill paths and allocator zero.
The zero-result case maps page zero in synthetic memory to check the
original unchecked writes; this is not an assertion that allocation
failure is safe in the game. The original memcpy and allocator remain
external calls, with explicit synthetic effects checked at both boundaries.

Five alternating opt1-lifted/opt3-port medians (ns/call): empty descriptor
with spare capacity **775.8 -> 495.1**, exact capacity **716.2 -> 639.9**,
refill **700.8 -> 699.7**, zero allocator result **816.1 -> 697.9**;
32-entry descriptor **823.5 -> 853.0**. Mixed results, shared-machine
timing, synthetic callees: no guaranteed workload/FPS improvement.
All seven isolated suites pass, registry test passes, strict own-crate
clippy passes. Logs: `restore-lanes-differential.log`, `restore-bench.log`,
`recording-{registry,clippy}.log` under `C:/ddda-targets/L130`.

**Activation correction (c20):** all six duration readers still appear in
the runtime fallback log despite being in ddports. Registry only inserts
generated entry addresses; these six have none. Head assigned **L136 to
codex-11** to bind missing overrides and test `call_ext`. Prior leaf
comparisons and fallback microbenchmarks remain valid; actual runtime
activation and any resulting FPS/memory saving were never established.
The c20 FPS change must not be attributed to these six ports.

Next: commit the separately verified FED200 lane initializer, then finish
the E10FC0 dependency-key wrapper oracle. Milestone update sent to its owner.

### FED200 recording-lane initialization

Whole port registered through mtcore/ddports. **60 original Unicorn and
isolated lifted cases / 560 call-return boundaries pass**: one/two/four
lanes, descriptor counts 0/1/8/32/4096, capacity 0/1/17/FFFFFFFF and all
three frame-bank indices. Full machine/memory state is checked with the
same explicit memcpy/memset callback contract. The initial zero-lane
branch is unreachable because the preceding DIV faults; the port retains
the existing lifted divide-error behavior. All other conditional edges pass.

Five alternating opt1-lifted/opt3-port medians (ns/call): one lane, count0
**523.2 -> 523.2**; one lane, count1 **527.2 -> 516.4**; two lanes, count0
**1,009.3 -> 1,020.5**; four lanes, count0 **1,924.5 -> 1,954.9**.
No reliable speedup. Includes synthetic callee work; no gameplay claim.
Registry and strict own-crate clippy pass. Private `lanes.json` is recorded
with `--lanes`; isolated benchmark uses `--bench --lanes`. Logs:
`restore-lanes-differential.log`, `lanes-bench2.log`,
`recording-{registry,clippy}.log` in the L130 target directory.

Next: E10FC0 original/lifted replay. L136 independently fixes runtime
activation of entries absent from the generated list (`d1c586ee`).

### E10FC0 dependency-key interning

Whole wrapper port keeps all six dependency calls and the final interning
call. The CRC lookup table stays in guest memory. **768 original Unicorn
and isolated lifted cases / 3,840 call-return boundaries pass**: all 16
header-presence masks, individual/all/no dependencies, optional CRC and
zero/multiple callback outputs. The byte-CRC arm is unreachable because
the original LEA multiplies by four; every feasible conditional edge is
covered. The callbacks are explicit synthetic ABI effects, not a new
equivalence claim for E0EC50/E0E5E0 themselves.

Five alternating opt1-lifted/opt3-port medians (ns/call): no dependencies,
CRC off **290.5 -> 252.4**, CRC on **371.8 -> 206.9**; all dependencies,
CRC off **707.8 -> 554.9**, CRC on **1,083.3 -> 846.2**; all headers and
dependencies with CRC **1,196.7 -> 1,043.8**. Synthetic callback work is
included. The c17 nearest-lifted share includes the existing E0EC50 port:
these timings do not establish how much gameplay work is removed.

All eight original/lifted suites pass (the broad run was suspended during
head's game comparison). Strict mtcore clippy and ddports registry pass.
Private corpus `keys.json`; record `--keys`, replay `--keys`, benchmark
`--bench --keys`. Logs: `keys-{differential,bench}.log` and
`keys-small-{clippy,registry}.log` in the L130 target directory.
The checker now supports focused replay when a family switch is supplied;
no switch still runs every suite. Next: five observed emulator entries.

### Five additional emulator replacements

Head/L136's run-log audit (board #11879/#11883) found nine remaining
addresses after the duration family. Ported **5828E0, 582900, 582920,
5ABDC0 and 5829A0** and registered all five through mtcore/ddports.
The first three forward stack/register arguments and installed-image
table pointers to 582A10. The predicate retains EAX=10001 when bit16 is
set. The transition leaf preserves its zero/latched/invalid state paths,
four-slot clearing and partial AL result. No table contents are copied.

**640 original Unicorn + isolated lifted cases / 768 call-return
boundaries pass**. Covers every conditional edge in the two branching
leaves, randomized GPR/XMM/flags, all rounding modes, rotated x87 TOP,
untouched x87 payloads, owner/state overlap and a clear overlapping saved
EDI (which must be restored before the writes). Each forwarding entry is
tested separately. Isolated lifter bodies are generated in private scratch
for these missing-inventory entries; inventory and shards remain unchanged.

Five alternating rounds, 20 warmups + 1,000 measured calls, actual runtime
Unicorn fallback on GuestMemory versus the port. Forwarding calls use the
same explicitly recorded synthetic 582A10 effects through a Registry binding
on the fallback side. Fresh Cpu x87 identities are released to avoid a
benchmark-induced spill leak. Full-state gates run before measurements.
Hardware FNSAVE in the runtime bridge writes FFFF into its three reserved
words; those are checked against the hardware contract, while architectural
x87 control/status/tags and all register payloads retain the original oracle.

| Entry / path | Fallback ns | Port ns | Ratio |
|---|---:|---:|---:|
| 5828E0 forwarding | 15,272.9 | 148.2 | 103.1x |
| 582900 forwarding | 14,174.8 | 148.5 | 95.5x |
| 582920 forwarding | 19,413.1 | 189.9 | 102.2x |
| 5ABDC0 clear / set | 6,499.1 / 7,535.2 | 48.1 / 49.6 | 135.1 / 151.9x |
| 5829A0 latched / latch | 11,016.6 / 9,032.3 | 85.6 / 91.6 | 128.7 / 98.6x |
| 5829A0 clear / changed | 10,695.8 / 9,790.6 | 140.7 / 89.3 | 76.0 / 109.6x |
| 5829A0 saved-EDI overlap | 10,504.2 | 93.2 | 112.7x |

Isolated opt1-lifted versus opt3 ports are much closer: forwarding
**138..149 / 144..180 ns**, predicate **47.7 / 42.1 ns**, invalid transition
**90.9 / 70.4 ns**. The improvement is emulator removal. Counts in the run
logs are first-seen diagnostics, all before their gameplay marker: they
do not establish invocation frequency or the removable gameplay share.
**No FPS/loading-time/memory improvement is claimed. Runtime activation
requires L136 and head's integrated run.**

Strict mtcore clippy and ddports registry pass. Corpus/body: private
`small.json/.rs`. Record `--small`, replay `--small`, benchmark
`--bench --small` or actual fallback `--bench --emu --small`, via heavy.py.
Logs: `small-{differential,bench,runtime-bench2}.log` and
`keys-small-{clippy,registry}.log` under `C:/ddda-targets/L130`.

This continuation adds **nine ports / 1,612 original cases / 5,744
callee boundaries**. Remaining assigned c17 candidates include 10A0310,
10A3C00, FEE320, FEDD60, DD42C0, DAED10, 112D970 and 431F10, plus the
worker helpers FEFB90/FF1920. These require larger whole-function fixtures;
none is claimed implemented in this batch. DBDC90 is the job scheduler,
so its nearest-lifted share includes waits and dispatch, not removable
body work. Four first-seen emulated entries remain: 4A0C00, 815C60,
C1D790 and the CRT interior entry 1300534 (platform-owner review needed).
Next action: head integrates with L136, checks the eleven newly covered
fallback addresses disappear, then selects the largest remaining exclusive
cost from the refreshed profile. Milestone evidence handed to codex-7/head.
