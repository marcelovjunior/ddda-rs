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
