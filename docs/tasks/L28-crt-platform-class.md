# L28: the CRT functions still marked `platform`

Owner: claude-3, branch `task/L28-crt` (cut from `task/L17-seh-workers`,
whose recorder it needs), worktree `E:\ddda-rs-wt\L17BC`, data set
`E:/ddda-re/notes/native-L17` (the one the boot, L21, generates from).

## Why

The boot (L21) passes `__heap_init`, `__mtinit`, `__RTC_Initialize` and
`__ioinit`, then stops at `__setargv` -> `__setmbcp` 0x012FB049, which has
no lifted body. 116 inventory rows still have status `platform`, rule
`CRT/library range`: `___updatetlocinfo`, `__control87`,
`___set_fpsr_sse2`, `__fpmath`, `___tzset`, the 64-bit division helpers,
stdio/lowio (`__fsopen`, `_fread_s`, `_fclose`, `__read`, `__lseek`,
`__close`, `__commit`, `__alloc_osfhnd`, `__getstream`, `flsall`,
`__fcloseall`), `FID_conflict:_memcpy` 0x012EDE40 and about 60 `FUN_` in
0x012E0EA0..0x0132A390. Per RUNTIME.md they are lifted and verified like
everything else, never written as services.

## Task

1. Census the class: which are still platform-only, and why the earlier
   CRT lifting (L14/L15) skipped them.
2. Bring them into the normal flow (scan, vectors, gen, required-vector
   replay) with a general rule, not by address.
3. Lift rejections go to claude-6 (L22, lifter rejections).
4. Tell claude-2 (L21) as each group lands.

## Rules

As L17: heavy jobs only through `E:/ddda-re/tools/heavy.py`,
`CARGO_BUILD_JOBS=4`, `CARGO_INCREMENTAL=0`, at most six vector workers,
existing records and cases unchanged unless a documented boundary change
requires it, no address special cases, no pushes.

## Results

(draft; numbers after the pause lifts)

**Census (native-L17, after L17's recording).** The 116 rows are not
excluded from any scan: `structured_inputs.py prepare` takes every row whose
status is not `verified`. The status is only stale because the inventory
was not rebuilt. They split into:

| Group | Rows | Why no fixture |
|---|---:|---|
| Lift rejections | 68 | lifter: x87-return/unsupported, STMXCSR/LDMXCSR/FNCLEX, RCR (all six 64-bit division helpers), the memcpy 0x012EDE40 table, PMAXSW/PSLLDQ/RSQRTPS/PAUSE/REPNE. Handed to claude-6; most are fixed on `task/L22-lift-rejections` (STMXCSR/LDMXCSR remain: guest MXCSR needs sticky SSE flags) |
| Recorder failures calling the SEH helpers | 28 | the recorder mocked `__SEH_prolog4`/`__SEH_epilog4`, so the frame never moved and the function's `ret` popped its scope table (e.g. `__setmbcp`: "returned to 0x0161EBE0" under every strategy) |
| Other recorder failures | 16 | noreturn and unwind paths (`_abort`, `__exit`, `___crtExitProcess`, `__math_exit`, `__global_unwind2`, `_EH4_GlobalUnwind2`, `__CallSettingFrame@12`), eight `FUN_` and `vf14` |
| Fixtures from L17's passes | 4 | `vf20` 0x0130A800, `vf31` 0x0130AAC0, 0x013201C0, 0x01320700 |

Across all 418 remaining candidates, 33 call an SEH helper.

The 16 other recorder failures, with every reason seen across the L17
passes (all strategies):

| Address | Function | Reason |
|---|---|---|
| 0x012ED9DD | `___crtExitProcess` | noreturn call (ExitProcess path) |
| 0x012EDC4B | `__exit` | noreturn target |
| 0x012EE1A1 | `vf14` | noreturn target |
| 0x012F9E95 | `_abort` | noreturn target |
| 0x013006FE | FUN_013006FE | unmasked x87 exceptions (FLDCW input) |
| 0x0130070B | `__math_exit` | unmasked x87 exceptions (FLDCW input) |
| 0x012FF0F0 | `__CallSettingFrame@12` | execution outside the image (calls a funclet address from its argument) |
| 0x01303CD2 | `_EH4_GlobalUnwind2` | execution outside the image |
| 0x01306FD0 | `__global_unwind2` | execution outside the image; overwritten return |
| 0x012E1DA1 | FUN_012E1DA1 | execution outside the image |
| 0x012F9D82 | FUN_012F9D82 | execution outside the image; runaway |
| 0x012F0D4C, 0x012F37AD, 0x012F45C4, 0x0132A1EC | FUN_ | NaN/Inf float input |
| 0x01314A90 | FUN_01314A90 | NaN/Inf float input; runaway |

The four noreturn bodies cannot have a returning fixture under the current
contract (the recorder rejects noreturn paths by design); the unwind group
needs exception-dispatch state (a runtime task, L15/L17).

**Rule: promotion of verified call-free callees.**
`structured_inputs.py promote` re-lifts a selected ext entry as a closed
callee when its arms make no call at all and some unverified candidate
calls it directly. Its own cases are re-recorded in closed mode with its
previous input distribution. The recorder (`run_sparse`'s `lifted` set) and
the replay (dispatch table) both run closed targets for real, so a case that
mocked a promoted entry would change meaning. Hence:

- An ordinary call-free entry that some recorded case mocks stays ext: a
  mock with arbitrary returns is a faithful stand-in for it.
- A caller-frame helper (derived `frame_exit`, L17) is promoted even when
  mocked, because no mock can move the caller's frame. The callers whose
  cases mock it are re-lifted against the new closed set and re-recorded
  with their previous distribution. The helper is promoted only if every
  one of them records again; otherwise it stays ext (iterate to a fixpoint),
  so no verified function is lost.

Promoted helpers store `frame_exit`, and `run_sparse(frame_contracts=)` ends
their nested frame at the return PC with exactly that ESP. The prolog
returns below the call's ESP, and the ordinary `ESP >= call ESP` test would
leave the caller marked nested, which breaks mocking and the own-return
check. A synthetic regression test shows the caller rejected without the
contract and recorded with it.

Two corrections during the run. (1) The first draft also required "no
recorded case mocks it". That excluded the SEH helpers themselves: 21 cases
of 8 verified functions (0x012EDAF5, 0x012F914F, 0x012F933B, 0x012F95B6,
0x012F96FD, 0x012FAD40, 0x01302F75, 0x01304FEE) mock both helpers. That run
promoted 11 unmocked call-free entries, 0x01306F7F among them. (2) Dropping
that gate entirely made 378 widely mocked targets (102,442 recorded calls)
eligible and started re-recording most of the program. I stopped it before
it wrote anything and restricted the mocked case to caller-frame helpers.

**Verification process.** Under the batched-verification rule
(AGENT-RULES.md, 2026-10-11) this branch records on its own data set and
runs the Python tests. The full gen + required-vector replay is the head's
integration train.

### Batch 1 (recorded on native-L17; replay is the head's train)

**Promotion.** 13 entries were promoted to closed callees. 11 were unmocked
call-free entries: 0x004021A0, 0x00D93610, 0x0129F000, 0x0129FB20,
0x012F51DC, 0x012F9EC8, 0x012FE663, 0x01306F7F (`__EH_prolog3` body),
0x01306FB5, 0x013070DC, 0x01314A30. The other 2 are the caller-frame
helpers `__SEH_prolog4` 0x012F71E0 and `__SEH_epilog4` 0x012F7225. All 8
verified callers whose 21 cases mocked the helpers re-recorded under the new
boundary, so no helper stayed ext: 0x012EDAF5 (`_doexit`), 0x012F914F,
0x012F933B, 0x012F95B6, 0x012F96FD, 0x012FAD40 (`___updatetmbcinfo`),
0x01302F75, 0x01304FEE.

**Recording** (fresh prepare after promotion: 418 candidates; L17's pass
protocol; then a `--retry 'memset of|memcpy of'` pass for the
constant-size rule):

| Pass | Added functions |
|---|---:|
| random (3 seeds) | 30 |
| objects-mockzero-v1 (3 seeds) | 1 |
| other 3-seed passes, the 12-seed passes | 0 |
| memset/memcpy retry, all strategies | 9 |
| **Total** | **40** |

| Measure | L17 final | Batch 1 |
|---|---:|---:|
| Selected lifted entries | 58,989 | 59,029 |
| Cases | 175,600 | 175,719 |
| Liftable candidates without a fixture | 418 | 378 |

Audit (`L28/audit_l28.py`, per function against L17's final pair):
exactly 21 records changed, the 13 promoted and 8 re-recorded; all other
58,968 functions' records and cases are byte-identical; 40 are new.

What records now. 26 of the 116 `platform` rows: `__setmbcp`,
`___updatetlocinfo`, `___tzset`, `__isindst`, `__fsopen`, `_fread_s`,
`_fclose`, `__fseeki64`, `_fseek`, `_fprintf`, `__read`, `__lseek`,
`__close`, `__commit`, `__alloc_osfhnd`, `__getstream`, `flsall`,
`__fcloseall`, `__sopen_helper`, `__exit`, `_Type_info_dtor`,
`___DestructExceptionObject`, `CallCatchBlock`, `___BuildCatchObject`,
`___BuildCatchObjectHelper`, `___FrameUnwindToState`. Of claude-7's
title-path list: `sSave::ctor` 0x004980F0, 0x00499830, 0x00499880,
0x00DCE2E0, `sShader2::ctor` 0x00E121D0 (constant-size memsets), `_exit`,
`__lseeki64` and `__write`.

**Unverified.** The batch has not been replayed in Rust on this branch
(batched-verification rule). The head's train runs gen + the gate on it.

**Remaining 378 candidates**, primary reason across all strategies:
execution outside the image 190, NaN/Inf float input 76, runaway 73,
noreturn call 15, two NaN operands 5, undefined flags 4, overwritten return
5, CPU exception 2, memset (non-constant size) 2, unmasked x87 2, misaligned
stack 1, noreturn target 1, page limit 1, x87 stack fault 1. Owners:
NaN/two-NaN go to claude-6's oracle work (L22 retires both guards, so they
need a retry after that merges). Runaway: claude-6's constant-loop budget
covers 8; stack-slot counters and value-tracked starts are open.
Outside-image is the recorder's mock-ABI class (L11-L13), next for this
branch. Noreturn call needs a terminal-fixture contract (below).

**Terminal fixtures (proposal, not implemented).** 15 candidates always
reach a noreturn call. Examples: 0x00D0CB00, a fatal-error reporter
(format, callback, `_exit(1)`; callers include D3D init 0x00DB1480), and
`___crtExitProcess`/`_abort`. A case could end at the noreturn call with the
state recorded there; replay would stop at that call_ext with a sentinel
and compare state. This needs the generated test template (lift.py) as well
as the recorder.

**Adoption (head, for a train).** Branch `task/L28-crt` (on top of
`task/L17-seh-workers`, merged as a097dfb): 522d784, 2018ea3, ede0c63,
de8880d, 0421471, plus this Results commit. Private data: native-L17's
`patterns/lift.json`, `vectors/lift.json`, `patterns/lift_novector.json`,
`patterns/lift_novector_keys.json`, `structured_quarantine.json`,
`quarantine_evidence.json`; evidence in `structured-*.jsonl`,
`structured-*-adopted.json`, `structured-promoted.json`, `L28/`. Merge
note: `research/native/lift.py` has a four-line `frame_contracts` plumbing
in `vectors()`, next to claude-6's L22 lines (claude-6 informed).

### Batch 2 (recorded on native-L17; replay is the head's train)

Recorder changes (general rules, each with synthetic tests):

1. **Mock cleanup.** `static_pop` searches up to the lifter's body limit
   (20,000 instructions) instead of 512. 0x0040AB20 ends in `ret 4` after
   640 instructions, and the old zero fallback left its argument on the
   stack. `IMPORT_CLEANUP` gives 45 imports their documented x86 cleanup
   (Microsoft Learn signatures; steam_api is cdecl), replacing L15's
   three-entry list. Traces behind this: 0x004083A0 returned 4 bytes low
   after `push eax; call 0x0040AB20`; 0x0130A5C0's QueryPerformanceCounter
   mock popped its local frame along with its argument; 0x01106C80's IMM32
   thunks fell back to zero cleanup.
2. **Constant model sizes.** A memset/memcpy size pushed as an immediate
   and not overwritten counts against a separate 4 MB per-run budget.
   Example: `sSave::ctor` clears 0x80000 bytes.
3. **Noreturn roots.** The noreturn-target guard exempts the root entry
   (`__exit` returns syntactically).
4. **Terminal fixtures (terminal-v1).** Head-approved. They touch lift.py's
   generated test template; see NATIVE_PIPELINE.md. claude-6 merged them
   into L22 and adds MXCSR to the Terminal payload there.
5. `record --retry REGEX` and `record/adopt --terminal`.

| Pass | Added functions |
|---|---:|
| retry outside-image/overwritten-return/misaligned: random | 46 |
| same, objects-mockzero-v1 / mockzero-v1 / zero | 2 / 1 / 1 |
| terminal: random (zero, mockzero-v1: 0) | 5 |
| **Total** | **55** |

| Measure | Batch 1 | Batch 2 |
|---|---:|---:|
| Selected lifted entries | 59,029 | 59,084 |
| Cases | 175,719 | 175,883 |
| Liftable candidates without a fixture | 378 | 323 |

Audit (`L28/audit_b2.py`, against the head's snapshot native-L28b1): no
record changed, 55 are new.

By claude-7's L38 queue: 9 title path (the IME setup 0x01106C80, 0x01106BB0,
the clock helper 0x0130A5C0, the Steam callbacks 0x004083A0,
`sSoundExt::vf06` 0x004AD440, 0x00DFA0C0, 0x00E04A90, and the terminal cases
0x00D0CB00 and `__amsg_exit`); 18 gameplay (`uPlayer::ctor` 0x00B55900,
`uNpc::ctor` 0x00BAB3A0, `uCmc::ctor` 0x00B53220, `uPlayerBase::vf23`,
`uCharacterBase::vf80`, `uModel::vf21`, `uGUICompound::vf05`,
`sPrimitive::vf16`, `_abort` (terminal) and 9 FUN_); 28 elsewhere.

**Unverified:** all 55, plus the template change. Terminal cases replay
only with this branch's lift.py template (or L22's merge of it). Data note:
claude-6's native-L22c also starts from L28b1. The two pairs add disjoint
functions as far as I know (L22 adds lift-rejection fixes, this branch adds
recorder-failure fixes), so adopting both means appending each one's new
records and cases to L28b1.

**Remaining 323 candidates.** NaN/Inf and two-NaN inputs wait for L22's
oracle merge, then a `--retry`. Runaway: the constant-loop work is
claude-6's; stack-slot counters and value-tracked starts are open. The rest
of the outside-image class needs per-trace diagnosis (indirect calls
through random data, internal entries).

**Adoption (head):** branch `task/L28-crt` commits after 0c9e87b
(afe22df, 3bf9559, the train-1 merge, this Results commit); private data:
native-L17's `patterns/lift.json`, `vectors/lift.json`,
`patterns/lift_novector.json`, `patterns/lift_novector_keys.json`,
`structured_quarantine.json`, `quarantine_evidence.json`.

### State at hand-off (2026-10-12, wrap-up requested by the head)

**Batch 3, recording in progress (run 4).** These commits came after
batch 2: f43e7ff/7000f4b/307287b/fcaadc4 plus the alloca-probe work:

- `moves_frame` classifies ESP moved by a value (`__alloca_probe`) as an
  unmockable frame helper, closed with `frame_exit: 'return-pc'` (nested
  frame ends at the first return-PC arrival).
- `local_alloc_sites` recognizes entry allocations through branches.
- promote's re-recording falls back across the standard input distributions.
- An indirect call's merged deferred caller cleanup is subtracted from its
  mock pop.

Run 4 (`L17BC/l28c.sh`, log `L28/run4.log`) archived the batch 2 trial logs
to `L28/trials-b2/`. It then promoted 0x00DB9C30 (call-free, unmocked). It
kept `__alloca_probe` ext: under that run's code only
`__alloca_probe_16`/`_8` failed to re-record, and the distribution fallback
committed afterwards fixes that (checked by a diagnostic: the other 21
mocking callers re-record). It is recording the full protocol; at the time
of writing native-L17 holds 59,089 entries / 175,898 cases. The local-alloc
and merged-cleanup changes landed while run 4's workers were running, so its
later passes mix recorder versions. That is harmless for validity: a mock
pop only ever leads to rejection, never a false pass. It does make
per-rule attribution for run 4 unreliable. The head adopts native-L17 after
run 4 ends; audit it with `L28/audit_b2.py`, pointed at native-L28b2 as the
base.

**Not yet recorded (run 5, ready).** It promotes `__alloca_probe` (29
remaining candidates call an alloca probe) with the fallback. It also
re-records everything under the merged-cleanup rule, which by trace makes
the sShader2 loader 0x00E0E470 record (3/3 seeds, 8 mocked calls).
Commands:

```
# after run 4 has finished (no other job of mine writing native-L17):
E:/ddda-re/tools/pyenv/Scripts/python.exe E:/ddda-re/tools/heavy.py <who> -- \
  "C:/Program Files/Git/bin/bash.exe" E:/ddda-re/notes/native-L17/L17BC/l28d.sh \
  > E:/ddda-re/notes/native-L17/L28/run5.log 2>&1
```

`l28d.sh` archives the trial logs to `L28/trials-run4/`, then runs
`structured_inputs.py promote`, `prepare`, and record+adopt for: the 9
strategies at 3 seeds / 20 s; the 3 zero-return strategies at 12 seeds /
60 s; random/zero/mockzero-v1 with `--terminal`. It sets `LIFT_DATA`,
`INVENTORY_DB` and `INVENTORY_EXTRA` to native-L17's copies and 6 workers.
Then hand the pair to the train; no local Rust replay (batched rule).

**Remaining candidate classes** (323 before run 4; about 318 now):
- Outside-image (about 160): mock-ABI residue that needs per-trace work.
  Seen in samples: mid-function supplementary entries (0x004E4145),
  indirect targets through random data, callee cleanups static_pop can't
  prove.
- NaN/Inf and two-NaN inputs (about 80): retry with
  `record --retry 'NaN'` once L22's hardware oracle is on main.
- Runaway (about 73): constant-trip loops with calls or stack-slot
  counters. claude-6 covers 8; stack-slot counters and value-tracked starts
  are open.
- Noreturn calls on non-root paths, and a residue of terminal-ineligible
  noreturn paths.
- Small classes: undefined flags, unmasked x87, page limit, CPU exceptions.

The lift rejections (STMXCSR/LDMXCSR, x87 forms, RCR and others) are
claude-6's (L22).

### L28b takeover: run 4 audit and run 5 (2026-10-12)

**Batch ready (codex, task/L28b-crt): +25 functions / +70 cases in run 6,
59,115 entries total; alloca promotion and its 23 caller refreshes audited.
33 light tests pass; Rust replay belongs to the integration train.**

Run 4 printed `run4 done` before this work touched native-L17. Its final
pair contains **59,090 selected entries / 175,899 cases**, leaving **317**
prepared candidates without fixtures. The inherited `L28/audit_b2.py`,
pointed at the head's native-L28b2 snapshot, reports **6 additions / 16 new
cases**, no removals and no unexpected edits; the only existing entry
changed is the promoted 0x00DB9C30.

The six additions are 0x007F2120 (random), 0x007DF790
(objects-mockzero-v1), and 0x004AB440, 0x00E0E470, 0x00E0E5A0,
0x00E111A0 (small-mockzero-v1). In particular **the shader loader
0x00E0E470 already recorded in run 4**. Run 5 preserves accepted entries;
promotion refreshes the helper and its previously mocking callers, and
the strategy passes retry the remaining candidates. It does not refresh
every accepted record.

Run 5 uses the supplied `L17BC/l28d.sh` with only its working-directory
line changed to this L28b worktree. It runs through `heavy.py codex`, with
native-L17's LIFT_DATA, INVENTORY_DB and INVENTORY_EXTRA and six workers.
Recorder source stays fixed throughout the run. No private data snapshot
was made. `research/native/audit_records.py` records hashes and addresses
only, then checks the complete selected pair: existing record hashes and
case multisets must match, except newly closed helpers and callers whose
old cases mocked them. Those callers must receive new cases, and no case
may still mock a newly closed helper. Missing entries also fail the audit.

Run 5 finished at 02:32:03: **59,090 entries / 175,899 cases**, zero
additions and zero promotions. `L28/run5-audit.json` reports **all 59,090
existing entries unchanged**, including their case multisets. No removals,
unexpected edits or stale mock cases. All prescribed strategy passes ran.

**Why the fallback still failed.** The existing recorder always initializes
ESP to a multiple of 16. The alignment wrappers 0x012F4E90 and 0x012F4EA6
compute alignment from `ESP + 4 - EAX`. Even zero EAX therefore allocates
four bytes, and their exit ESP equals entry ESP; the ordinary stack guard
rejects it. Merely switching among the existing input distributions cannot
exercise their zero-allocation path.

The new opt-in `zero-call-v1` uses the zero-input distribution with entry
ESP four bytes below an aligned caller stack (ESP modulo 16 = 12).
It adds no address special case or stack-contract exemption, preserves all
existing distributions and checks, and is included in promotion fallback.
Synthetic 8-/16-byte alignment tests reproduce the old rejection and the
new valid return; the promotion test proves this distribution is tried for
an affected caller. The existing rejection test still forbids promotion
when a caller cannot re-record. **33 light tests pass** (16 structured-input,
13 frame-contract, 4 audit).

Run 6 (`L28/run6.sh`, `L28/run6.log`) starts with an assertion against each
original alignment wrapper, then promotes, prepares, and records/adopts
random, zero, zero-call-v1 and small-mockzero-v1 (three seeds, 20 seconds).
It moves old trial logs aside; no selected-data snapshots are taken. Since
run 5 preserved the entire pair, the run-5 hash manifest is also the exact
run-6 baseline. Both original wrappers rejected under the old zero strategy
and returned with ESP increased by four under zero-call-v1, **3/3 seeds
each**. Promotion then closed **0x012F2280**, re-recorded all **23** existing
mocking callers and kept every selected entry (59,090 / 175,901 cases at
that point). Run 6 finished at 02:49:42:

| Measure | Before run 5 | After run 6 |
|---|---:|---:|
| Selected entries | 59,090 | 59,115 |
| Recorded cases | 175,899 | 175,969 |
| Liftable candidates without a fixture | 317 | 292 |
| Lift rejections (this branch, before L22 integration) | 157 | 157 |

Random added 17 functions / 44 cases; zero added 8 / 24. Promotion added
two cases while refreshing the existing helper/callers. The new aligned
strategy enabled promotion; it added no functions directly in its own
pass. Seven new entries occur in TITLE_PATH.md: **0x004403A0, 0x00448500,
0x0044E6F0, 0x004709D0, 0x004A3260, 0x00F9FBA0, 0x00FEE130**.

**Preservation proved:** `L28/run6-audit.json` reports **59,066 unchanged
old records and case multisets**. Exactly 24 existing entries changed:
the promoted 0x012F2280 and its 23 previously mocking callers. No entries
were removed, no unrelated entries changed, and no cases still mock the
promoted helper. The report lists every added and refreshed address.

**Adoption / verification:** native-L17 is frozen after run 6 for the head
to snapshot and adopt. Use its `patterns/lift.json`, `vectors/lift.json`,
`patterns/lift_novector.json`, `patterns/lift_novector_keys.json`, plus the
unchanged quarantine metadata. This is a boundary-changing pair, not just
25 append-only additions: when combining another lane's data, re-record
any imported case still mocking 0x012F2280. No Rust build or replay was
run here, per batched verification; all 25 additions and the refreshed
24 entries await the integration train. The original-wrapper assertions
and the 33 light tests passed. No shared native/inventory data was written,
and no snapshots were taken by this agent.

The audit can be repeated without modifying the pair:

```
E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/audit_records.py compare --root E:/ddda-re/notes/native-L17 --manifest E:/ddda-re/notes/native-L17/L28/run5-before-hashes.json --report E:/ddda-re/notes/native-L17/L28/run6-audit.json
```

**One next action:** integration replay of this pair; then resolve the
image-table/stack-slot bounded loops below with the lifter lane. NaN and
instruction rejections still wait for L22 integration. The outside-image
terminal-call cleanup finding remains a diagnosis, not a shipped fix.

**Title-path follow-up evidence (not fixes).** Of the 317 run-4 residuals,
62 addresses occur in TITLE_PATH.md (this is a document intersection, not
a newly proven reachability closure). Private evidence is in
`L28/run4-title-residue.json` and `L28/title-loop-evidence.json`.

- Archive registration 0x00419100 reads ten descriptors via 0x0153B6BC.
  Their image-backed counts total 13,061 records. Its six-instruction
  inner loop at 0x00419113 already requires 78,366 instructions, above
  the default 20,000 budget. Random inputs cannot remove this loop.
- `sObjCollision::ctor` initializes a stack-slot counter to 799 at
  0x0047862B and decrements it at 0x004786B5 (800 iterations).
  `sNpcManager::ctor` also has a stack-slot counter initialized at
  0x0047549A and decremented at 0x00475540. These require bounded-loop
  analysis beyond the register-counter forms in the L22 lane; sent to
  head and claude-6 on the board.
- 0x00DB8B90 pushes one argument for 0x00DB8AC0. The latter's normal
  exit 0x00DB8B65 tail-jumps to LeaveCriticalSection through 0x0139D164
  (four-byte cleanup), but its other path calls 0x004080A0 at 0x00DB8B7F
  then reaches INT3 at 0x00DB8B84. The recorder's static cleanup analysis
  returns zero because of that trap path. This is a candidate diagnosis,
  not a proved recording fix: terminal-path classification needs evidence
  before changing the conservative fallback. The next link is concrete:
  0x004080A0 calls inventory symbol `__invoke_watson` (0x012EDDD5) at
  0x004080C5, then reaches INT3 at 0x004080CB. Neither name is covered by
  the current four-symbol `noreturn_entries` set. Sent to the lifter lane.

### L28c: boot partition and imported alloca boundary (2026-10-12)

**Batch ready: boot blocker 0x00DAFED0 has eight accepted fixtures;
0x00F6A380 is refreshed. 59,534 entries / 177,196 cases, both changes
audited separately. 39 light tests pass; Rust replay awaits the train.**

Merged main into task/L28b-crt at **3d8ac913**, retaining both sides of the
milestone-note conflict. Harness and structured-input changes merged
automatically; all 33 preceding light tests passed. This batch uses only
**native-L28c**, initially **59,533 selected records / 177,188 cases**.
native-L17 stays frozen; native-train3 is untouched. INVENTORY_EXTRA points
to native-L28c/extra_functions.tsv; the shared inventory database is opened
read-only (no private database was supplied with this data set).

**Boot blocker 0x00DAFED0:** random, zero, small-v1 and objects-v1 each
rejected as `runaway` in the original recorder, with its ordinary 20,000
instruction limit. Read-only disassembly/decompilation establishes:

- First and last are entry-stack arguments +8/+12 (loads at 0x00DAFEE2
  and 0x00DAFEDD); +4 is the output iterator-pair pointer.
- The distance is converted to a midpoint and multiplied by **0x5C** at
  0x00DAFF06. Median selection calls 0x00DAFD10 at 0x00DAFF12.
- Comparisons use two unsigned words at record offsets **0x50/0x54**
  and a float at **0x58** (e.g. 0x00DAFF30..0x00DAFF4C).

Independent random pointers/counts do not describe this bounded range;
zeroing both pointers still does not supply the nonempty median-selection
precondition. The general `record-range-v1` strategy takes a private JSON
layout describing stride, first/last/result stack or register bindings,
and finite float/unsigned-key fields. No function-address dispatch is
used. It fixes all initial bytes before execution, keeps image reads and
guest writes unchanged, and records the layout for exact re-recording.
Its seeded cases include ascending, descending, equal and mixed keys,
3/5/8/17/33 records, and page-crossing records. No budget or rejection
check is relaxed.

The baseline audit also finds exactly one imported stale caller:
**0x00F6A380** still mocks already-closed **0x012F2280**. The generic
`refresh-closed` operation re-lifts/re-records only such callers, preserving
the current lifter's x87-entry and instruction-budget metadata. A failed
caller refresh writes neither selected file. This repairs an imported
boundary mismatch without re-promoting or demoting unrelated entries.

**Original-code checks:** all four old distributions rejected. The bounded
range produced **8/8 accepted trials** without increasing the instruction
budget. Every trial also passed independent checks on its recorded writes:
returned bounds are inside the input range and record-aligned; the output
records are a permutation of the inputs; and the less/equal/greater
partitions satisfy the lexicographic two-uint/float comparator. Counts
3/5/8/17/33 and equal-key cases are exercised. These are original-code
recording checks, not a claim that Rust replay has passed.

The harness owner agreed to the five input-only hooks in board #9531;
the source patch was then applied. **39 light tests pass** (19 structured
inputs, 13 frame contracts, 2 range-generator tests, 5 audit tests). The
refresh tests also check failure before pair writes and removal of stale
optional x87/budget metadata when the current lifter no longer emits it.

The first refresh job was terminated before reporting a result. Its hash
audit confirmed all 59,533 entries and 177,188 cases unchanged. The retry
repaired only 0x00F6A380; `L28c/refresh-audit.json` confirms **59,532 old
records/case multisets unchanged**, no removals, no unrelated changes,
and no remaining mocks of 0x012F2280. Counts stayed 59,533 / 177,188.

**Final pair:** **59,534 records / 177,196 cases** (+1 / +8); missing
prepared candidates **229 → 228**, with **20 lift rejections / 3
quarantines** unchanged. `L28c/partition-audit.json` compares against the
post-refresh hash manifest and proves **all 59,533 pre-existing records
and case multisets unchanged** during adoption. Its only addition is
0x00DAFED0. Together with the preceding refresh audit, exactly one original
entry changed (0x00F6A380) and exactly one was added. No snapshots were
taken; the manifests contain hashes/addresses only.

**Private evidence:** `L28c/diagnose.log`, `partition-trial.json`,
`partition-check.log`, `refresh-audit.json`, `partition-audit.json` and
`partition-layout.json` under native-L28c. The eight trial cases were
recorded through heavy.py using the exact proposed hook in an isolated
process while waiting for the shared-file owner. After agreement the
same hook was applied to source and light-tested normally. Adoption and
hashing execute no guest code; that light step ran directly with 16 GB
free after cancelling its unnecessary queued heavy wrapper.

**Reproduction for a missing candidate:** set LIFT_DATA to
`E:/ddda-re/notes/native-L28c` and INVENTORY_EXTRA to its
`extra_functions.tsv`, then run through heavy.py as codex:

```
python research/native/structured_inputs.py record --strategy record-range-v1 --layout E:/ddda-re/notes/native-L28c/L28c/partition-layout.json --address 0x00DAFED0 --seeds 8 --seconds 20
python research/native/structured_inputs.py adopt --strategy record-range-v1
```

Use the research Python interpreter specified above. The private layout is
`stride=92`, `first={stack:8}`, `last={stack:12}`, `result={stack:4}`,
`uint_offsets=[80,84]`, `float_offsets=[88]`. `--address` is an optional
candidate filter, not an address-specific implementation rule. Successful
existing functions are not re-recorded by `record`. `refresh-closed`
repairs imported stale closed-call boundaries; its audit requires explicit
`--refresh-closed 0x012F2280` authorization of that exact target.

**Adoption:** native-L28c is frozen for head's snapshot/merge. Take its
selected pair and updated `patterns/lift_novector.json` /
`patterns/lift_novector_keys.json`; quarantine metadata is unchanged. No
full Rust replay was run here. The next action is integration replay and
resuming the real boot, then prioritizing the next executed title-path
failure. Image-table/stack-slot loop budgets and the terminal-call cleanup
diagnosis above remain open; coordinate with L45 before changing extents
or terminal evidence. Both L45's `refresh`/`table_goals` and this branch's
separate `refresh-closed` command must be retained when merging.

### L28c render cleanup batch (2026-10-12, boot preview ready)

**59,534 → 59,542 selected records; 177,196 → 177,220 cases.** Eight
additions, three original-code fixtures each: **0x00DAA4B0, 0x00DAE760,
0x01128EE0, 0x01128F40, 0x01129030, 0x01129310, 0x01129370,
0x01129460**. All 59,534 old records and case multisets are hash-identical
(`native-L28c/L28c/title-cleanup-audit.json`); no removals, promotions or
stale mocked targets. Missing prepared candidates: **228 → 220**.

The recorder's indirect-call cleanup overcounted arguments when a later
stdcall/thiscall consumes an argument pushed before the indirect call.
Concrete evidence: 0x01128F4F pushes the diagnostic string, 0x01128F54
pushes the COM receiver, and 0x01128F55 calls Unlock. 0x01128F57 then
pushes HRESULT and 0x01128F5A calls 0x00DA89B0, whose return at
0x00DA8C15 is `ret 8`. Unlock consumes four bytes, leaving the string for
the later call. The analogous Lock sequence is 0x01128F19..0x01128F31.
The general forward cleanup scan now accounts for the later direct
callee's static pop, reserving its pre-pushed prefix. No address-specific
case, changed input distribution, or relaxed execution guard is involved.

**41 light tests pass:** 15 frame-contract (two new regressions), 19
structured-input, two range-generator and five audit tests. The new tests
cover the saved prefix and a subsequent callee consuming only intervening
pushes before explicit caller cleanup. Original recording ran through
heavy.py; seven additions use zero-call-v1, 0x00DAE760 uses zero-objects-v1.
Rust replay remains for the head's integration train.

Before this change all 29 remaining direct-title candidates rejected with
three seeds in each of zero-call-v1, zero-objects-v1, small-mockzero-v1 and
image-v1. Private evidence: `L28c/title-diagnosis.jsonl` and
`L28c/title-cleanup.jsonl`. The broader one-hop trial was stopped on head's
new priority order, preserving its completed rows. **native-L28c is not
frozen**, per head: 0x01129030 and 0x01129460 are ready for preview now;
0x00DA9EC0 still rejects and is being traced first, then 0x01104470.
Constructor constant-loop analysis remains open (stack counters, copied
initializers and closed-callee cost); no speculative budget change landed.

### L28c priority boot-route follow-up (2026-10-12)

**0x00DA9EC0 and 0x01104470 now record 3/3 each.** The preceding batch
already added 0x01129030 and 0x01129460, so all four requested boot-route
functions have accepted fixtures. Private pair: **59,542 → 59,544 records,
177,220 → 177,226 cases**. `L28c/boot-route-audit.json` compares against
the post-render hash manifest and proves every old record/case multiset
unchanged. Missing prepared candidates: **220 → 218**. No promotions,
removals or unrelated changes; native-L17 and native-train3 remain untouched.

For 0x00DA9EC0 the original trace reached its own return at 0x00DAA4A4,
but ESP was four bytes too high. At 0x00DA9F2E the COM call is followed
by `mov ecx,[esp+0x20]` (0x00DA9F30), before the HRESULT push and call
to 0x00DA89B0. The cleanup scan previously stopped at that neutral setup
because it had not yet seen a later push. It now crosses non-branching,
non-ESP-writing instructions before as well as after that push. Its
24-instruction bound and branch/call/stack barriers remain. A synthetic
MOV case passes; replacing it with a jump still rejects, demonstrating the
conservative barrier. **42 light tests pass** across the four test files.

0x00DA9EC0 uses zero-call-v1. 0x01104470 uses image-v1: zero-based
distributions correctly reject at the `div ecx` at 0x01104491, whose
divisor is the byte loaded from its first argument +0x25 at 0x01104489.
With the cleanup correction, all three image-backed trials return normally.
No input bytes or arithmetic were repaired during execution.

Evidence: `L28c/trace-da9ec0.log`, `trace-00da9ec0.json`,
`boot-route.jsonl`, and `boot-route-audit.json`. Recording used heavy.py;
no Rust replay. The data set stays **open, not frozen**, as explicitly
requested by head, so claude-2 can preview these additions immediately.
