# L10: functions missing from the inventory (initializers, virtual targets)

Owner: Codex (same session). Branch `task/L10-inventory-gaps` from the
head's `main` (a507b2a or later), worktree `E:\ddda-rs-wt\L10`. This is
R02's first half (docs/RUNTIME.md, "Staged implementation").

## Why

R01 found that the CRT initializer loop (`0x012EDABC..0x012EDAC9`) walks
104,456 slots with 5,656 distinct non-null targets, and only five of them
are in the inventory (`E:/ddda-re/notes/inventory/inventory.sqlite`, built
from a Ghidra export). Ghidra never defined most static initializers as
functions; virtual-function targets that are only reached through vtables
may be missing too. Our percentages and the lifter both start from that
inventory, so everything those functions do (game state set up before
WinMain, e.g. registration at `0x0136DDF0`) is invisible today.

## Task

1. Discover function entries the inventory lacks, deterministically and
   from the installed image: the C and C++ initializer tables, vtables
   (RTTI-described tables in `.rdata` whose slots point into executable
   sections), and direct call targets inside lifted bodies that are not
   inventory entries. Classify each with evidence; leave out anything you
   cannot show is code.
2. Record them as a supplementary table the tooling reads alongside the
   Ghidra export (e.g. `E:/ddda-re/notes/inventory/extra_functions.tsv` plus
   a `scripts/inventory.py build` step), with the discovery source per
   entry. Sizes are hints only: L07's recursive descent finds bodies.
   Keep the canonical Ghidra project untouched (its MCP server is down).
3. Run the lifter scan over the new entries, record vectors for those
   that lift, pass the lifted test on your data. Report counts per source
   (initializers, vtables, call targets), how many lift and verify, and the
   new totals the inventory reports.
4. Note in docs/INVENTORY.md and docs/NATIVE_PIPELINE.md where the extra
   entries come from and how to regenerate them.

## Rules

As in L08 (`docs/tasks/L08-lifter-integer-sse.md`, "Rules"); the head seeds
`LIFT_DATA=E:/ddda-re/notes/native-L10`. The recorder's memory fixes are
in main (docs/NATIVE_PIPELINE.md, "Unicorn's code-hook size").

## Results

(Codex fills this in.)
