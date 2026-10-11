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

(Codex fills this in.)
