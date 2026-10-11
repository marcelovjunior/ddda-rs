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

Codex implemented the task (recursive-descent bodies in `lift.decode_body`:
reachable instructions from the entry, bounded by the next inventory entry,
shared tails decoded in each owner, noreturn CRT calls (`_abort`, `_exit`,
`__exit`, `__amsg_exit`) end a path, switch discovery within the band or
with a proven bound; `harness.run_sparse(body=...)` uses the body's
addresses for ownership and tracks call frames so shared code is not taken
for the root's return; 30 Python tests pass). Its after-scan has no
"disassembly" or "falls" rejections left. Its session was stopped by Claude
Code (low memory) during the vector run; the head finished that run.

The memory problem was the recorder, not the lifter: some functions loop
over a random count of random pointers and map hundreds of random pages per
seed (minutes per function, no vector), and long-lived workers grew to
3-4 GB. Fixes in main and here: Unicorn's translation buffer capped at
32 MB per instance, `gc.collect()` after each function, a worker replaced
after 8 functions (`chunksize=1`), and a 20-second budget per function
(`LIFT_FN_SECONDS`).

Result on the batch: 28,665 functions with vectors (from 28,244), 947 of
them re-recorded because their call boundary changed; 1,117 candidates have
no usable vector (mostly the random-count loops; they need structured
inputs, e.g. `LIFT_ZERO_INPUTS`).
