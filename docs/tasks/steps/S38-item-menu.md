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

## Results (codex-71, 2026-10-10; capture still required)

Implemented and tested source checkpoint `f34223b`, after S51 host inventory
integration and reviewed main `3ecd13e`. No whole-brief Ready claim until the
required screenshot is inspected. Head alone merges.

- Native screen: `uGUIItemLE::vf58` 0x00668270 loads installed
  `id/DDN/co_item/co_item`; `uGUIPause::vf58` 0x006B7ED0 loads
  `id/DDN/pause/pause0_001`. This step keeps the viewer pause parent with an
  Items entry; the native pause-top VM remains unported.
- `0x006682B0` binds itemName/itemInfo and
  `id/message/pause/pause_item/pause_item_eng`. Ordinary names/descriptions and
  Use (pause_item index 2) are read from installed English messages. Late-game
  aliases remain unresolved.
- `0x0066A190` creates twelve icons in a four-column, three-row grid from
  native nodes 43/44/45. `0x00605DA0` reads four ushort atlas corners per item
  at installed exe table 0x0151AF48. Category order comes from installed
  0x017F15E8 as used by 0x0066EEC0. No atlas table or pixels copied.
- Parent object links use 0x01077EC0 -> 0x01068C60/40: +8 is sibling, +C child.
  Native parent transforms and authored font, texture, size, colour and line
  endings drive the static projection. Full GUI animation/filter/overlay
  state, native sort and reserved quest-tab execution remain unported.
- Per-player menu state snapshots the requesting Arisen's real ECON bag and
  active session pawn bags. Selection retains `Owner`, index and exact slot;
  Use calls `Host::item_request`, the same authority as quick use. Native
  0x00669990's additional record+0x354 pawn admission flag remains unresolved.
- Co-op policy approved #6460: host ticks while only the menu owner's device
  input is blocked; demo-side actions and orbit for that owner are also
  suppressed. One full-screen overlay is presented at a time; other owners'
  menu state stays independent. Bindings/repeat are explicit viewer policy.
  Give, guest inventory transport, dynamic member-name headers and the native
  pause/filter VM remain next work.

Validation:

- Full `ddview --release` suite, session 49728: **88 passed, 2 existing ignored,
  0 failures**, actual exit 0. Fresh source compile 20.71 s; tests 12.33 s.
  All four source-present menu regressions ran: real runtime reward rows and
  owner/slot identity; native parent geometry and rejected bad links; installed
  icon/bitmap-font drawing without GPU; selected-slot use through host with
  foreign/stale rejection, isolated bag consumption and continued host ticks.
- Default release build, session 47922: actual exit 0, 37.65 s.
- S51 dependency: full core **137 passed**, full viewer **84 passed/2 existing
  ignored**, release build and parent independent installed item **3 passed**;
  detailed evidence remains in S51 Results. S38 changes presentation/hooks only.
- Test recovery actors and authored q1 grants are explicit installed-data
  fixtures, not completed-route claims. Native resource probes also found 1901
  non-empty icon rectangles and connected trees for six selected animations;
  no original-game visual comparison is claimed.

Private logs/source probes/pin: `E:/ddda-re/notes/steps/S38` (never committed).
Default pin `ddview-s38-item-menu-f34223b.exe`, SHA-256
`4EB1D2D788697E7EBFB071604CFE601C83923F4E51007EAD468AF34FA3A9DB6E`.

Next action, capture request #6927: head #6740 regenerates real ECON v2 route
checkpoints after S51 lands. Use a real post-q1 reward checkpoint with
`DDVIEW_ITEM_MENU_CAPTURE=1` and the existing `--shot`/`--shot-delay`, under the
normal GPU lease, `--unattended` and a retained-handle watchdog <=180 s. Inspect
that the actual menu contains native names/icons/counts from the restored bag.
Old checkpoints are not backfilled with manufactured items or rewards.

Parent capture (2026-10-10): actual session20203 exit0, owned viewer30744
numeric exit0 after58.443933s. Normal GPU lease released only after the
owned viewer was absent; original DDDA23408 remained running. Frozen pin
above resumed head's real ECONv2 `RoadWalk.ddhs` (SHA-256
`F19945A9F9A075282A3B4A375696C7259A6453AE4007394E4C3A647C8705A5A9`).
Actual logs identify seat0, Arisen(0), category0 and two real slots; the
capture driver did not grant items or rewards. The screenshot was saved and
inspected at `E:/ddda-re/notes/steps/S38/real-v2-roadwalk-f34223b/items.png`.
Installed bitmap text shows the selected Scrag of Beast name/description,
two icons/counts and the selection highlight. Screenshot existence and real
bag projection are verified; original visual fidelity is not accepted:
the Use control overlaps the description, native category/member headers
remain absent, and the static animation projection exposes coloured states
whose native control/overlay transitions remain unported. The rendered icon
association still needs an original-game comparison. Performance numbers
from this screenshot run are not timing evidence.

Two earlier runs are rejected: a nonexistent `--quit-at` flag exited101
before bootstrap; then the older pin rejected the newer `ReynardRepeat`
resume name and exited1. Source preflight selected its supported `RoadWalk`
step/checkpoint; `--shot` naturally exits one virtual second after its shot,
with an independent retained-handle90s watchdog. Neither failure is counted
as acceptance.

Parent follow-up under head7028: failed Use stores `Selection.error`, but
the painter currently discards that text. Existing native Use handler
0x0066C750 falls through0x006EADA0 on refusal; no installed failure-message
mapping/layout was established. A guessed native message was not added.
Next action: resolve native control/overlay state for the description and
Use submenu, then recapture the same real bag; map refusal feedback from
source. Source/host-use regression acceptance remains separate from this
open visual gate.
