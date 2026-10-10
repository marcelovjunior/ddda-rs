# S58: The equipment menu: equip and unequip from the bag

- **Branch:** `step/s58-equipment-menu`
- **Files:** `crates/ddeconomy` (equip/unequip on host-owned bags),
  `crates/ddhost` (command + save), `crates/ddnet` (guest command, only if
  needed), new `crates/ddview/src/equipment_menu.rs`; tests
- **Kind:** feature (a must-have of the current goal: "inventory")

## Context

Bags are host-owned and saved (S51); the item menu (S38) uses and discards.
Equipment changes only at the vocation's first choice and the
heart-taking swap (S28). The original's pause menu has an Equipment page
(uGUI classes for it in mtxfs/classes.rs; Items is S38's pattern).

## Do

1. Trace the original's Equipment page: its uGUI class, layout resources,
   slot list (12 slots, docs/VOCATIONS.md level-3 note), which items each
   slot accepts (item data category/flags), vocation and weight checks, and
   what equipping changes (stats, weight class 0x... from PARAMS.md, the
   dressed model).
2. Host command `Equip { seat, slot, item }` / `Unequip` with the traced
   rules, validated on the host bag, saved, and per profile for co-op.
3. The viewer page from the installed GUI resources, opened from the pause
   menu like Items; the Arisen's model updates (existing dressed-model path).
4. Tests: the rules (accept/reject per slot and vocation), stats/weight
   change, save round trip, a guest command applied by the host.

## Done when

From the pause menu the player can equip a weapon or armour from the bag;
the stats, weight and model change as in the original; tested headless,
one bounded capture of the page.
