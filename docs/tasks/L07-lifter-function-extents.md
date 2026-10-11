# L07: lifter: function bodies by recursive descent

Owner: Codex (same session). Branch `task/L07-function-extents` from the
head's `main` (the head names the base commit), worktree
`E:\ddda-rs-wt\L07`. After L06.

## Why

About 1,850 functions are rejected as "disassembly" (1,025) or "falls"
(820). A sample of 400 shows the cause is almost always the function's
*extent*, not its instructions: the lifter disassembles the contiguous
range [address, address + size) with `size` from the Ghidra export, and
that size is often wrong:

- the code continues past the recorded end (`0x0045C720`: 104 bytes per
  the inventory, but its epilogue `mov ecx, esi; pop esi; pop ebx; ...;
  ret` follows; 99 of 400 sampled "falls" end on a `pop`);
- linear decoding runs into another function's middle or into data, so
  capstone desynchronises ("undecodable mid-code", `0x00405410`, 206 of 400);
- a few have data (jump tables, padding) inside the range.

## Task

1. Find each function's body by **recursive descent** from its entry:
   follow fall-through and direct jumps, add switch-table targets (L01's
   recognizer), stop at `ret`, at noreturn calls, and at a jump to another
   function's entry in the inventory (a tail call, as today). The body is
   a set of instructions, not necessarily contiguous; code shared by two
   functions (MSVC tail merging) may appear in both.
2. Use that body everywhere the contiguous range is used now: `lift()`
   (leaders, "inside the function" checks), `harness.run_sparse` (`end`
   for mocking, the own-`ret` check: pass the body's address set or
   ranges), `lift.py vectors` (what it passes), and the inventory sizes
   only as a hint. Keep it deterministic (same body for the same input).
3. Report before/after rejection counts, record vectors for the new
   functions, and pass the lifted test on your data. Note any function
   whose body overlaps another's.

## Rules

As in L06 (`docs/tasks/L06-lifter-crate-split.md`, "Rules"). If L06 has
changed where generated code lives, follow its new layout.

## Results

(Codex fills this in.)
