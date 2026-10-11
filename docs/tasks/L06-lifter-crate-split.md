# L06: lifter: split the generated code across crates

Owner: Codex (same session). Branch `task/L06-crate-split` from the head's
`main` (the head names the base commit), worktree `E:\ddda-rs-wt\L06`.
Do this before more coverage work: every further lifter gain makes the
single generated crate bigger.

## Problem

All lifted functions are modules of one crate, `ddnative`
(`crates/ddnative/src/generated/lifted_NNN.rs`, about 123 MB of Rust for
28,244 functions). One rustc process compiling it peaks at about 18 GB on a
32 GB machine (L05 measured 17.82 GiB), and memory grows with every new
batch. Twice tonight Claude Code killed jobs because the machine ran out of
memory. A rebuild after any regeneration recompiles all of it.

## Task

1. Split the generated code into several crates (e.g. `crates/ddlift/
   lift_00` ... `lift_NN`, a fixed number of shards by address range or by
   size, so a function's shard is stable across regenerations). Shared
   machinery (`Mem`, `Cpu`, `Flags`, `x87`, `platform`, `call_ext`) stays in
   a small base crate the shards depend on; `ddnative` hand ports stay where
   they are or move to the base, whichever keeps the dependency graph acyclic.
2. Direct calls between lifted functions currently use
   `super::lifted::f_xxxxxxxx`. Across shards that needs a crate path, and
   shard dependencies must stay acyclic: either order shards so callees come
   first (calls only go "down" the shard order, e.g. by lifting round), or
   route cross-shard calls through a function table / `call_ext`-style
   dispatch installed at start-up. Choose with measurements; keep the
   generated code exact (the vectors judge).
3. The tests: one integration test (or one per shard) must still replay
   every vector, and `scripts/native_gate.sh` plus `scripts/inventory.py`
   must keep finding the `// Translated from` and `// verifies` lines
   (check how they scan; adjust them if the paths change).
4. Measure peak memory per rustc and wall time for a clean build and for a
   rebuild after regenerating one shard, with `CARGO_BUILD_JOBS=2` and 4.
   Goal: peak per process well under 8 GB, total clean build not slower.
5. Update `lift.py gen`, docs/NATIVE_PIPELINE.md ("Memory", "Generated
   code") and the gate script if needed.

## Rules

As in L05 (`docs/tasks/L05-lifter-x87-arithmetic.md`, "Rules"): own
`LIFT_DATA` (the head seeds it), never write `E:/ddda-re/notes/native`, no
`native_gate.sh` runs against the shared data, commit source/docs and the
new crate skeletons (Cargo.toml, lib.rs, build plumbing) but not the
generated shard contents unless the head says so, git identity janne /
marcelovjunior55@gmail.com, no AI trailer, never push. Memory: never more
than one full build at a time, `CARGO_BUILD_JOBS=2` until the split is in.

## Results

(Codex fills this in.)
