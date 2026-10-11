# L04: lifter: floats returned on the x87 stack

Owner: Codex (same session). Branch `task/L04-x87-returns` from the head's
`main` (the head names the base commit), worktree `E:\ddda-rs-wt\L04`.

## Why

`x87-return` is now the largest rejection (about 1,210 functions). MSVC's
32-bit calling convention returns `float`/`double` in `st(0)`, so a caller
that does `call GetSomething; fstp dword ptr [x]` reads a value the callee
pushed. `lift.x87_depth_check` rejects any x87 read or pop at depth 0,
because a mocked call (ext mode) pushes nothing.

## Task

Two cases, both exact:

1. **Mocked callee returning a float (ext mode).** When the static depth
   walk shows that st0 is read after a mocked call with nothing pushed by
   the function itself, the mock must push a value. Suggested design (not
   binding): `run_sparse`'s mock, instead of jumping straight to the return
   address, jumps to a small fixed trampoline on its own fresh page,
   `fld dword ptr [SLOT]; jmp dword ptr [SLOT + 4]`, with the random finite
   float in SLOT and the return address in SLOT + 4 (data, not code: Unicorn
   caches translated code per page; see the st0-capture pitfall). The event
   records the pushed value (`"st_push": bits`), and the generated test's
   replay pushes it onto `cpu.st` (single-precision bits, as the lifter's x87
   model holds them). Decide which calls push: the lifter knows statically
   where a value is consumed; carry that to the recorder (e.g. per call site
   address in the lifted entry) so both sides agree exactly.
2. **Closed (lifted) callee returning a float.** The depth check is
   conservative here: a closed callee runs for real on both sides and pushes
   its result, so the caller is fine. Compute each lifted function's net x87
   effect at its `ret`s (the same walk; reject inconsistent ones) and use it
   at closed call sites instead of 0.

Keep `x87_depth_check` strict everywhere else. Count functions newly lifted
per case, record vectors for them, and pass the lifted test on your data.

## Rules

As in L01/L03 ("Rules" in `docs/tasks/L01-lifter-switch-tables.md`): own
`LIFT_DATA` (the head seeds it), never write `E:/ddda-re/notes/native`, no
`native_gate.sh`, source and docs only in commits (no generated code), git
identity janne / marcelovjunior55@gmail.com, no AI trailer, never push.
Document the convention in docs/NATIVE_PIPELINE.md ("Calls to untranslated
code (ext)" limits paragraph).

## Results

(Codex fills this in.)
