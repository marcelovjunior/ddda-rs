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

## Result (codex-38)

Implemented `cyclops::enter_action`, translating 0x0097fd30 and its base
0x007f1890/collision helper 0x0081f790. Both per-action fields are read
from the installed exe. Disassembly corrects the decompiler's misleading
array types: both fields have **16-byte stride**, and 0x014fee64 is a byte,
while 0x014fee68 is a word. Invalid action ids select row zero.

State written: action completion +50, conditional enemy link +54,
cleared scratch +58/+60/+70 and float +64, entered byte +6c, checked
cyclops link +74; owner action flags +2370/+2374, conditional scratch
+273c and priority +2e34 (pending +2e38 bits 0x40/0x400 remap), conditional
sentinel +2ba0, vectors +2530/+2540 including zero W, byte +5c7c, and
motion/frame fallback +623c. The collision switch at owner +2e74 updates
its mode, six cached suppression bits and present object +61 bits only
under the original bound/mode-change/override guards. Unresolved scratch
semantics are explicitly named as such, not assigned gameplay meanings.

The shared vector at 0x018cfe80 is runtime BSS; the function accepts its
three components from the caller, avoiding an unproven file-image default.
This is a headless primitive, not a live action manager or connected fight.
Installed regression uses actions 0 and 81, both owner gates, priority
remapping, missing collision objects, suppression override and invalid-id
fallback. No game process or install was modified.

Validation: `cargo test -p ddenemyrt -- --test-threads=1` passed (74 passed, 1 existing ignored); installed enter test passed in 0.01 s. `cargo clippy -p ddenemyrt --no-deps` exited 0 with only the two pre-existing combat.rs type-complexity warnings (lines 243/245). Both used the shared item-use-review-target. Next: S06 placement initializer,
then connect the action driver with actual owner identity and runtime
reset-vector inputs; no original-game execution comparison claimed.

