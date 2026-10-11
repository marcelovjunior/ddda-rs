# L14: lift the "platform" layer too

Owner: Codex (same session), after L13. Branch `task/L14-platform-layer`
from the head's `main`, worktree `E:\ddda-rs-wt\L14`.

## Why

The inventory classifies 12,707 functions as `platform` and the lifter
skips them: deleting destructors, DTI allocators, construct-if-memory
wrappers, base destructor chains, the MT allocator entry points, and the
whole C runtime block at 0x012E0000 and above. That split was made when
ports were hand-written; it no longer fits:

- The goal is every function of DDDA.exe translated 1:1 and verified.
  These are ordinary x86 functions (the CRT is MSVC 2010 code linked into
  the exe), not operating-system code.
- The runtime (`crates/ddruntime`, docs/RUNTIME.md) is now executing the
  game's static initializers on the loaded image: 5,650 of 5,662 complete.
  What stops the rest is CRT code that is not translated (e.g.
  `__snprintf_s` 0x012ED991, `_initstdio` needing `_ioinit` state). With the
  CRT lifted, the runtime only needs services for the true OS imports
  (`image::bind_imports`: KERNEL32, USER32, d3d9, ... 273 imports), and can
  run the CRT entry 0x012EFA14 itself instead of a hand-made start-up.
- RUNTIME.md already says the ">= 0x012E0000 is platform" rule is
  unsuitable as a runtime binding policy.

## Task

1. Include the platform-classified functions in the lifter's selection
   (the five platform-model addresses keep their models for the tests;
   decide with evidence whether their lifted bodies should also exist for
   the runtime), scan, record vectors, adopt as before. Inline-assembly CRT
   routines (SSE memcpy/memset variants, `_alloca_probe`, `_chkstk`, SEH
   prolog/epilog helpers that manipulate fs:[0] or the stack) may need
   specific handling: report what blocks them rather than special-casing
   semantics.
2. `inventory.py` should stop counting a lifted, verified function as
   `platform`: the status becomes `verified`; keep the platform class
   available as a separate attribute for reports.
3. Report how many platform functions lift and verify, per platform rule,
   and the new totals.

## Rules

As in L13 (`docs/tasks/L13-residual-census.md`, "Rules"). The head seeds
`LIFT_DATA=E:/ddda-re/notes/native-L14`.

## Results

Implemented platform-inclusive selection, retained classification, and private per-rule reporting.

Both the ordinary scan and the missing-entry scan include historical platform
classes, retaining the five explicit test-model exclusions. The incremental
scan preserves the existing closed-call boundary and releases every decoded
body. Existing accepted records/cases are retained verbatim; new bodies use
external-call mode rather than changing old mocked-call contracts.

`families.py platform` now honors private paths and writes per-address rule
metadata. `inventory.py build` stores `functions.platform_rule` independently
of status; verified declarations/tests take precedence. Range classification
wins over shapes and excludes supplementary entries. Existing lists without
rule metadata remain readable with an explicit unknown-shape label.
`platform_report.py` emits per-rule totals and evidenced residual reasons.

**Model decision:** no new original body is admitted at the five model
addresses. Direct probes find 0x00CF73F0, 0x00DE0B80, 0x00DE0B90 and
0x012EFE20 syntactically liftable, while 0x012EFEA0 rejects an unproven
indirect switch table. The pool body calls through the IAT at 0x00CF73F6;
the MT wrappers read allocator state at 0x018D0AA0. Original bodies should
ultimately exist for image-backed runtime execution, but the current recorder
intercepts these entries. Its fixtures would describe the models, not the
original instructions. A distinct original-entry recording contract and a
runtime binding policy remain required. All five existing models and caller
fixtures are unchanged; no unverified runtime-only bodies are generated.

**Scope of verification:** fixtures verify each admitted body's instructions
and its recorded callee interactions. They do not prove closed CRT startup or
allocator behavior. `___tmainCRTStartup` 0x012EF8A7 calls `__SEH_prolog4`
0x012F71E0 at 0x012EF8AE; that helper and `__SEH_epilog4` 0x012F7225 reject
FS segment access. Random startup trials take noreturn paths at 0x012EF950
or 0x012EF987. No SEH semantics, import services or runtime state were invented.
The CRT entry 0x012EFA14 and basic `__alloca_probe` 0x012F2280 were already
verified before L14; the aligned probe wrappers are separate new candidates.

**Selection and recording:** the missing-entry scan retains 43,567 accepted
entries and finds 13,141 candidates plus 161 lift rejections. Previously the
non-platform remainder had 467 candidates and 83 lift rejections; the increase
is the newly eligible population, not lost accepted code. Three deterministic
seeds per strategy and a 20-second function budget produce:

| Pass | Tried | Provisional additions | Cases |
|---|---:|---:|---:|
| Random | 13,141 | 12,479 | 37,280 |
| Zero | 662 | 117 | 351 |
| Small words | 545 | 8 | 16 |
| Zero objects | 537 | 0 | 0 |
| Mixed objects | 537 | 4 | 7 |
| Replay quarantine | 6 | -6 | -14 |
| **Net** | | **12,602** | **37,640** |

No non-platform body gains a fixture in this batch. Every addition has a
historical platform rule. The original accepted records and cases remain
unchanged; the private full/subset audit checks record equality and the case
prefix. No emitter semantics or recorder validity guard was weakened.

**Replay findings:** initial subset replay identifies six genuine oracle/state
mismatches, retained under `patterns/lift-mismatch-L14.json` and
`vectors/lift-mismatch-L14.json`, with `diagnose-six.log` and
`quarantine_evidence.json`. `__ctrlfp` 0x012FC159 loads control at 0x012FC17B:
host 0x0F7F versus Unicorn 0xAFFF. `__modf_default` 0x012FCEBC exposes status
bit 0x20 through EAX at a mocked call. Bodies 0x01314930 and 0x013149B0 differ
in returned EAX by status bit 0x02; 0x013179A0 and 0x01317A00 differ by 0x20,
while their reported float values agree. No bit normalization or selection
of only passing seeds is used. All six bodies are quarantined pending an
x87 oracle/state contract. The four prior quarantines are unchanged.
Recording and adoption now both honor quarantine even with older candidate
and successful trial files; a regression test prevents accidental re-admission.

The remaining deleting destructor 0x00DB8CD0 calls 0x00DB8BF0 at 0x00DB8CDA.
That callee tail-jumps through `[0x0139D164]` at 0x00DB8C9B. Bounded cleanup
analysis returns unknown/zero for this indirect tail. The recorded zero-input
trace confirms the mock removes only the return address, leaving the pushed
argument; the destructor's RET at 0x00DB8D06 then transfers to zero. This is
an unresolved import-tail cleanup contract, not missing destructor lifting.
`trace-dtor.log` preserves the instruction/ESP evidence.

**Per historical rule (all entry points, including thunks):**

| Rule | Inventoried | Liftable* | Verified before | Verified after | Added |
|---|---:|---:|---:|---:|---:|
| Deleting destructors | 3,926 | 3,926 | 0 | 3,925 | 3,925 |
| DTI new-instance allocators | 2,863 | 2,863 | 5 | 2,863 | 2,858 |
| Construct-if-memory wrappers | 352 | 352 | 0 | 352 | 352 |
| Base destructor chains | 23 | 23 | 0 | 23 | 23 |
| MT allocator entries | 5 | 3 | 3 | 3 | 0 |
| CRT/library range | 5,635 | 5,551 | 28 | 5,472 | 5,444 |
| **Total** | **12,804** | **12,718** | **36** | **12,638** | **12,602** |

*Selected/candidate/quarantined bodies, excluding the four historical-class
model entries. The fifth model, 0x00CF73F0, has no historical platform rule.
The CRT row includes 47 thunks (two previously verified, 45 added); its
non-thunk figures are 5,588 inventoried, 5,504 liftable and 26 -> 5,425 verified.
The attribute covers already cited/verified members as well as the old
12,707 non-thunk `platform` status, so those are different populations.

`platform-report.json` accounts for every historical-class residual:
82 lift rejections, 74 recorder failures, six replay mismatches and four
models. No unexplained or unadopted successful entry remains. All 82 lift
rejections belong to the CRT range: x87-return 35, segment override 10,
STMXCSR 8, x87 unsupported 7, RCR 6, PMAXSW 3, PSLLDQ/register/FNCLEX two
each, and RSQRTPS/PAUSE/indirect/REPNE/RCPSS/LDMXCSR/x87-overflow one each.
No instruction or SEH behavior is special-cased to force admission.

`__snprintf_s` 0x012ED991 and `__ioinit` 0x012F7C13 have replayable cases;
aligned alloca wrappers 0x012F4E90 and 0x012F4EA6 are admitted. Startup
0x012EF8A7 remains without a case after all five strategies. No loaded-image
startup or live-game acceptance claim is made.

| Whole inventory measure | Before | After |
|---|---:|---:|
| Selected entry points | 43,567 | 56,169 |
| Lifted cases | 129,587 | 167,227 |
| Verified non-thunks | 43,327 | 55,884 |
| Verified thunks | 244 | 289 |
| Verified non-thunk share | 76.6% | 98.7% |
| Verified summed non-thunk bytes | 14,103,864 | 14,847,363 |
| Verified share of summed code sizes | 90.9% | 95.7% |

Denominator unchanged: 56,593 non-thunks / 15,516,093 summed bytes; 56,882
rows including thunks. All verified rows total 56,173 (four verified hand
ports are outside the lifted selection). Whole-inventory residual: 533
recorder failures, 161 lift rejections, ten quarantines and five models,
exactly 709 unverified rows.

**Validation:** 69 Python tests pass (60 native, nine inventory), including
platform selection/model exclusion, status/rule independence, supplementary
range exemption, quarantine adoption, and report evidence checks. After the
six quarantines, subset replay passes 80,009 cases / 26,788 functions and
seven Rust units. Final subset compile 27.87s, replay 21.67s, total 52.02s,
minimum free RAM 19.44 GiB. Full replay result pending.

**Handoff/regeneration:** adopt the matching private
`E:/ddda-re/notes/native-L14/patterns/lift.json` and `vectors/lift.json`, plus
the quarantine list/evidence and mismatch records. Regenerate Rust with
`lift.py gen`; generated files are not in this commit. The supplementary TSV
is snapshotted privately and unchanged. On main, regenerate classification
with `families.py platform` using the shared output directory, then rebuild
inventory normally (`inventory.py build`); no discovery rescan is needed.
For the isolated report use private `LIFT_DATA`, `INVENTORY_DB` and
`INVENTORY_EXTRA`, retain `patterns/lift-baseline.json` and
`inventory-baseline.sqlite`, and run `platform_report.py` after successful
replay and inventory rebuild. All data, diagnostics, rejected cases, logs and
resource metrics remain private. No shared writes, native gate, Ghidra
restart, runtime source changes or push.
