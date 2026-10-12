# L22: the remaining lift rejections and quarantines

- **Owner:** claude-6
- **Branch:** `task/L22-lift-rejections` (worktree `E:\ddda-rs-wt\L22`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

58,628 of 59,290 functions are verified (main, `docs/INVENTORY.md`). Of
the 662 others, claude-3 (L17 B/C) handles functions that are lifted but
lack fixtures: SEH helpers, worker loops and runaways. This brief takes
the other two groups:

- **Lift rejections** (about 145): functions the lifter refuses. Causes
  include x87 returns, a jump table the recognizer cannot bound in
  `memcpy`, segment-register reads, CPUID (the CPU name/clock the registry
  service answers from a recorded machine), and a few unsupported
  instructions.
- **Quarantines** (ten): x87 status-word differences between the host and
  Unicorn.

## Task

1. **Census:** list every rejection and quarantine with its reason
   (`scripts/inventory.py`, the lifter's reject reasons, and
   `E:/ddda-re/notes/native/quarantine_evidence.json`), grouped by cause.
   Put it in Results.
2. **Fix by cause**, largest group first, in `research/native/lift.py` and
   the lifter's Rust output conventions (`crates/ddnative-base`), each
   cause with a general rule:
   - CPUID through a runtime service that answers like the host CPU (the
     registry service `crates/ddruntime/src/registry.rs` reports a
     recorded CPU name and clock, so keep the two consistent);
   - segment registers through `Cpu` (it has `fs_base`);
   - x87 returns as the lifter's x87 model defines them
     (`docs/tasks/L04`, `L05`);
   - jump tables per L01.
   For the quarantines, find which side is right on real hardware (run the
   instruction sequence natively, for example in a small C/asm probe under
   `research/`) and make the lifter, or the recorder's model, follow the
   hardware.
3. **Record and verify** with the normal pipeline (lift, vectors, gen, the
   required-vector replay; see `docs/NATIVE_PIPELINE.md`) on your own copy
   of the shared data: copy `E:/ddda-re/notes/native` to
   `E:/ddda-re/notes/native-L22` and point `LIFT_DATA`, `INVENTORY_DB` and
   `INVENTORY_EXTRA` at it. Existing records and cases must stay unchanged
   (prove it, as L14/L15 did).
4. claude-3 (`l17bc-ab`) works in the same pipeline. Keep your lifter
   changes separate from its recorder/strategy changes, and tell it when
   you change a shared file. Write adoption instructions for the head in
   Results.

## Results

(open)
