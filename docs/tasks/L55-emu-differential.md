# L55: check the emulator fallback against the recorded test cases

- **Owner:** Codex
- **Branch:** `task/L55-emu-differential` (worktree `E:\ddda-rs-wt\L55`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

The boot's bring-up now runs through the emulator fallback (L51,
`crates/ddruntime/src/emu.rs`, feature `emu`): untranslated functions
execute as original machine code in Unicorn, with registers, EFLAGS, x87
(FNSAVE image), MXCSR, XMM and FS synced to and from the runtime's `Cpu`.
With it the game reaches the title screen. Every bring-up result depends
on that state sync being exact, and so far one unit test checks it.

We already have exact references: the recorded cases (inputs and expected
outputs, from the original program) of about 59,000 verified functions.

## Task

1. A test (ddruntime, feature `emu` plus `game`) that takes recorded cases
   from `LIFT_DATA/vectors/lift.json` and, for each:
   - sets up the case's guest memory and registers as the replay does
     (`crates/ddnative/tests/lifted.rs` shows how);
   - runs the function through `emu::run` instead of the lifted body. The
     exe's code pages must be mapped: load the image as `Session` does.
     Mocked calls (`calls` in the case) must be bound as call-outs that
     return the recorded results;
   - compares registers, flags, x87 state, MXCSR, XMM and memory writes
     with the recorded outputs, exactly as the replay does.
2. Run it over a sample that stresses the sync: functions using x87 (the
   `x87_*` records), SSE (MXCSR, XMM), flags across calls, FS accesses, and
   a random sample of the rest (say 2,000 functions).
3. Fix every mismatch in `emu.rs` (one cause at a time, with a test), or
   document why the case cannot run in the emulator (e.g. a mock protocol
   it cannot express).
4. Results: counts per class, the fixes, and the remaining exceptions.

## Notes

- Building with `emu` needs CMake on PATH:
  `C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin`.
- Use plain compiler vocabulary in code and messages (see the head's
  earlier note): this is a test of an emulator against recorded outputs.
- Heavy builds go through heavy.py as codex. The 32-shard build fits 6
  jobs in a slot now.

## Results

(Codex)
