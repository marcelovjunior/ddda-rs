# S06: Translate the cyclops placement initializer (0x00978820)

- **Branch:** `step/s06-cyclops-init` (after S05 is merged)
- **Files:** `crates/ddenemyrt/src/cyclops.rs`, one new test
- **Kind:** one function translation

## Facts

- 0x00978820 is the placement-specific health/leg initializer (codex-36).
- The q0003 cyclops is stage 100 cell (63,52), group 20, index 0, flag
  30, special HP 0.2 (docs/tasks/cyclops-runtime.md Results).

## Do

1. Decompile 0x00978820 as in S05.
2. Translate it into `cyclops.rs`: inputs are the placement values it
   reads, outputs the health/leg values it writes. Read any tables from
   the exe at runtime.
3. Add one installed test using the q0003 cyclops placement (group 20,
   index 0) that checks the resulting HP and leg values.

## Done when

Tests and clippy as in S05 pass, and the Result gives the initial HP and
leg values for the q0003 cyclops.
