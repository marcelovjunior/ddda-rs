# S05: Translate the cyclops base action enter (0x0097FD30)

- **Branch:** `step/s05-cyclops-enter`
- **Files:** `crates/ddenemyrt/src/cyclops.rs`, one new test in
  `crates/ddenemyrt/tests/`
- **Kind:** one function translation

## Facts (codex-36, docs/tasks/cyclops-runtime.md Results)

- The base action enter is 0x0097FD30. It clears several body action
  fields and reads two per-action tables at 0x014FEE64 and 0x014FEE68.
- The catalogue, packed motions and the Wait/Common/Attack callbacks are
  already in `cyclops.rs`; the installed exe is read at runtime through
  `ddexe` (see how `cyclops.rs` already reads tables).

## Do

1. Decompile 0x0097FD30 with the Ghidra headless server:
   `curl "http://127.0.0.1:8089/decompile_function?address=0x0097fd30"`.
2. Translate it into a Rust function in `cyclops.rs` that takes the action
   id and the body's state (add fields to the existing body state only if
   the function writes them). Read both tables from the exe at runtime;
   never copy them into the source.
3. Name every field after what the code does; put the address in a
   comment (`// 0x0097FD30`).
4. Add one installed test (it skips when the game isn't installed, like
   the other installed tests) that runs the enter for two action ids and
   checks the fields it sets.

## Done when

`cargo test -p ddenemyrt -- --test-threads=1` passes, `cargo clippy -p
ddenemyrt --no-deps` has no new warnings, and the Result lists which
fields the function sets.
