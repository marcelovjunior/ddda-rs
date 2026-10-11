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

(Codex fills this in.)
