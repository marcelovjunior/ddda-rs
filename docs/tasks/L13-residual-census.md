# L13: the residual: close what can be closed, account for the rest

Owner: Codex (same session). Branch `task/L13-residual` from the head's
`main` (eab8faa or later), worktree `E:\ddda-rs-wt\L13`.

## Why

After L12: 43,298 of 56,593 inventoried functions verified (76.5%, 90.8%
of code bytes); platform 12,707 (replaced, not translated). What remains of
the game's own code:

- about 497 lifted functions without a usable fixture (outside-image 287,
  runaway 88, NaN/Inf 78, other 44);
- 86 lift rejections (x87 input 26, rsqrtps/rcpps/rcpss 33, segment
  overrides 9, indirect 7, platform-dependent cpuid/rdtsc/pushfd 4,
  sfence 3, ...);
- functions the inventory calls "untouched" (490), "translated" or
  "documented" that are in neither list: find out why each one is not in the
  lifter's selection at all (thunks, data mislabelled as code, platform
  overlap, scan filters...).

The goal is every function translated 1:1 and verified, so the end state
of this task is: everything closable is closed, and every function that is
not verified has a recorded, evidenced reason.

## Task

1. Census: one table (private JSON plus a summary in docs/INVENTORY.md)
   covering every non-platform inventory function that is not verified,
   with its category and reason.
2. Close what can be closed exactly: e.g. `sfence` (a no-op for this
   single-threaded model? only with evidence), approximate reciprocals
   (rsqrtps/rcpps/rcpss: the host CPU result can be recorded and replayed if
   the recorder runs the same instruction on the host rather than in
   Unicorn: evaluate), segment overrides (`fs:` TLS/SEH access as a platform
   event), the x87-input cases, the fixture-less functions with targeted
   strategies (outside-image: trace a sample again after L11/L12 fixes).
3. Re-scan, record, adopt and report as before.

## Rules

As in L12 (`docs/tasks/L12-remaining-lift-rejections.md`, "Rules"). The head
seeds `LIFT_DATA=E:/ddda-re/notes/native-L13`.

## Results

(Codex fills this in.)
