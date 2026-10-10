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
