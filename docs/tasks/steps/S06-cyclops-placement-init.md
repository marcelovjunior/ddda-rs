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

## Result (codex-45)

Implemented `cyclops::PlacementValues` and `initialize_placement` in
063dc70, with table-read validation before mutation in a98f8e0. This is
the health/leg slice of 0x00978820 (0x009790ca..0x0097916d), taking the
four authored placement inputs and updating this actor's existing pools.
It requires ordinary region initialization and the common HP multiplier
from 0x00889040 to have been applied first. The q0003 placement has that
common multiplier disabled.

Owner vf82 resolves to 0x004c6af0 and returns the first pool slot's maximum
HP; the source neither sums pools nor searches for ID zero here. Special
remaining HP is maximum times the authored ratio, capped above at maximum
without a lower clamp. Helper 0x00770f00 maps hit regions 5 and 6 through
0x014ff680 and finds the matching pool identities. The mapping is read from
the installed exe, never copied into the implementation. The special
placement overwrites **current** DP/BP (+0x14/+0x20), preserving maxima,
speeds, other pools and other state.

Installed stage 100 cell (63,52), group 20, record 0 yields **initial HP
5000 / maximum 25000**, and both leg pools have **current DP 1500 and BP
1500**. One installed regression checks these calculations against the
parsed placement and region resource, all untouched fields, disabled
special flag, upper cap, negative ratio, identity-based leg lookup with
reordered pools, and an empty pool array.

Validation used E:/ddda-rs-wt/item-use-review-target exclusively:
`cargo test -p ddenemyrt -- --test-threads=1`: **75 passed, 1 existing
ignored**, exit 0. After the table-read ordering adjustment,
`cargo test -p ddenemyrt --test cyclops_placement -- --nocapture`: **1
passed**, output `q0003 initial HP=5000/25000; legs DP=1500 BP=1500`,
exit 0. `cargo clippy -p ddenemyrt --no-deps`: exit 0, only the two
pre-existing combat.rs type-complexity warnings at lines 243/245.

This does not translate the entire initializer's resource, equipment,
experience, think, status and rendering setup. In particular the earlier
+0x5ea0 bookkeeping array receives small integer entries from 0x015bfb14;
it is not treated as a child-health table. Its semantics remain open. No
live original-game comparison or host encounter hookup is claimed. No
install or process writes, new targets, or unrelated file changes were
needed. Next: S14 think driver and target selection, supplying actual
placement and canonical actor state to these primitives.
