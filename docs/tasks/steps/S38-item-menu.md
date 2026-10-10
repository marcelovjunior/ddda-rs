# S38: The item menu (pause menu: Items)

- **Branch:** `step/s38-item-menu`
- **Files:** `crates/ddview` (a menu module; `items.rs` holds the bags),
  GUI/message readers if missing; tests
- **Kind:** research + one feature (viewer)
- **After:** S28 (done: the opening's new-game bags)

## Context

The Arisen's bag now exists in the opening (S28: `items::QuickItems`
built from the new-game bag and quest rewards, quick use on U / Numpad0).
There is no way to see it: the original opens its in-game menu (Start /
Esc ... check the PC bindings) with an Items screen listing each member's
bag, item names/icons/counts, and lets the player use a restorative or
hand an item to a pawn (`uGUIItemGive`, 0x015823A4).

## Do

1. Find the in-game menu's GUI classes and installed layouts (the main
   menu and its Items screen), the item name/description messages and
   icon textures for the items the opening's bag can hold.
2. Draw the Items screen from those resources (reuse `hud.rs`'s rGUI
   node drawing; extend it for missing node types), listing the bag of
   the local seat's Arisen and, as the original does, its pawns.
3. Input: open/close, cursor, Use (through the same path quick use takes,
   so the host applies it); Give if it fits the step's size, else leave
   it as a documented next step.
4. The game keeps running underneath or pauses as the source says (the
   co-op host must not pause for one seat: note the decision).
5. A viewer test for list contents from a bag, and a screenshot.

## Done when

In `--opening` the menu shows the Arisen's real bag with installed names
and icons, using a curative heals through the host, and the Result names
the GUI classes, layouts and messages.

## Result checkpoint (codex-71, 2026-10-10; NOT ready)

Source WIP only; **not compiled or tested and no viewer capture yet**. Head
board #6504 prioritizes S51 host-owned bags first; this branch preserves
the menu work for adaptation to S51's authoritative inventory API.

- Native screen: `uGUIItemLE::vf58` 0x00668270 loads installed
  `id/DDN/co_item/co_item`; `uGUIPause::vf58` 0x006B7ED0 loads
  `id/DDN/pause/pause0_001`. This checkpoint keeps the existing viewer
  pause parent with an Items entry; the native pause-top VM is unported.
- `0x006682B0` binds itemName/itemInfo and
  `id/message/pause/pause_item/pause_item_eng`; ordinary item names and
  descriptions come from installed itemName/itemInfo English messages;
  Use is pause_item index 2. Late-game message aliases remain open.
- `0x0066A190` creates twelve icons in a four-column, three-row grid from
  native nodes 43/44/45; `0x00605DA0` reads four ushort atlas corners per
  item from installed exe table 0x0151AF48. No atlas table or pixels copied.
- Parent-object links verified from 0x01077EC0: object +8 is sibling,
  +C is child. Static drawing applies parent transforms to book objects;
  full GUI animation/filter/overlay state and native sort are unported.
- Owner-specific menu state, active bag snapshots, expected-live-slot
  validation, and selected-slot use share the existing quick-use recovery
  path. Head #6460 approved host ticking with only owner input suppressed.
  Guest inventory requests, Give, native keyboard remapping/repeat, and
  opening pawn inventory admission remain open.
- Checkpoint defect reported #6464/#6474: QuickItems lives outside Host.save.
  No reward is synthesized to make captures work. S51 is the prerequisite.

Private research only: `E:/ddda-re/notes/steps/S38` (decompilation,
installed resource probes, geometry checks). GUI and exe probes found
1901 non-empty installed icon rectangles and connected object trees for
the six selected animations. These are source observations, not Rust test
or fidelity acceptance. Next action: S51, then adapt and run full ddview
tests/release plus a bounded real-bag capture through the shared GPU queue.


### S51 adaptation prepared (source only)

The dedicated menu projection now keeps `Owner` with the selected slot rather
than translating bag choice back into an actor seat. Controller indices convert
explicitly to `PlayerId`; projected snapshots remain per-owner. Its list test
uses a headless ECON/runtime reward fixture, with no QuickItems manufacturing
path. Opening hooks still need S51 integration and adaptation; no Cargo or
capture acceptance is claimed for this checkpoint.


### Host authority integration checkpoint (still UNTESTED viewer)

Parent reviewed S51 inventory source and its full core run (137 passed). This
isolated S38 branch integrates that frozen source. Opening snapshots now clone
the requesting Arisen's ECON bag and active session pawn bags; selection sends
`Owner`, index and exact expected slot to `Host::item_request`. Keyboard quick
use calls the same API and respects each menu owner's input lock. Native pawn
admission additionally tests record+0x354; that flag is not yet represented.

Head #6740 will regenerate ECON v2 route checkpoints after S51 lands, then the
S38 capture uses a real post-reward checkpoint. Old ECON v1 checkpoints are not
backfilled with manufactured rewards. Full viewer tests/release and the capture
are still required; no screenshot or original comparison is claimed.
