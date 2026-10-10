# S28: The new-game inventory in the opening

- **Branch:** `step/s28-new-game-bag`
- **Files:** `crates/ddview/src/items.rs` (a new-game constructor),
  `crates/ddview/src/opening.rs` (quick use), `crates/ddeconomy` /
  `crates/ddprogress` if the source gives the items there; tests
- **Kind:** research + one feature

## Context

Quick item use (`items::QuickItems`, U / Numpad0) works only in `--play`
with a save: it reads the save's bags. The opening is a new game, so its
Arisen has no bag. The vocation's first choice (`ddprogress::change::
starting_items`, 0x00673740) gives equipment (the weapons, and 0x227 /
0x228 for everyone), not restoratives. What the Arisen carries at q0001
(after the prologue and the dragon) is not traced.

## Do

1. Trace the new game's starting bag: what `0x00445AB0` (new game) and the
   prologue/q0000/q0001 scripts give (item adds through 0x0045E680 /
   0x0045ECE0, as story_pawns.rs found for Rook), and check it live in the
   original at q0001 if needed (read-only, oracle rules).
2. Give the opening's Arisen seats that bag (and the vocation's first-choice
   items as the game adds them), as a `QuickItems` built without a save.
3. Hook quick use into the opening the way `play::step` does.

## Done when

In an `--opening` run the Arisen's bag holds the traced items, U uses a
restorative when hurt (test), and the Result lists the items with their
sources.
