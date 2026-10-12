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

PENDING: promote, fresh prepare, the L17 pass protocol (3 seeds x 9
strategies, then 12 seeds x 3 zero-return strategies), counts, what records,
the noreturn/unwind remainder, adoption.
