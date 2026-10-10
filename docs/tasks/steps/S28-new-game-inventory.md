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

## Results (codex-59)

The opening does **not** begin q0001 with restoratives. The first quest
rewards them when it actually clears after ev10. Preloading the prologue's
potions into Cassardis would contradict the original's bag reset.

- `0x00445ab0` calls `0x004460d0`: the temporary prologue hero receives
  equipment 601..608 and item 43 x5, 44 x5, 217 x2, 83 x3, 48 x3 and
  64 x1. These do not carry into the new Arisen's bag.
- Prologue-end `0x00444840`, disassembly `0x00444b02..0x00444b0d`, calls
  `sItemManager::vf10` (`0x0045da90`) through vtable +0x28. That clears
  all five bags (`0x00458010`) before the `0x00444b20` stage-800 handoff.
- Successful first character creation (`0x0052b2d0`, first-game test
  `0x0052b740`: SceNo 1000 and `sGameSys+0xb8844 & 0x20000`) calls
  `0x0077fd80`: item 301 (Iron Sword) and 1184 (Set of Fisherman's Garb),
  one each, added/equipped through `0x0045ece0` / `0x00459100`.
- Before event row 21 (st200 ev01), `0x0050c290` case 0x15 unequips the
  Arisen (`0x00459650`), removes one 301 and one 1184 (`0x00461be0`),
  and adds/equips 1185 (Set of Fisherman's Tatters). Other collected
  items survive this mutation. Thus q0001 starts with 1185 x1.
- The actual free first vocation choice (`0x00673740`) clears equipment,
  then adds/equips the existing `ddprogress::change::starting_items`:
  Fighter 321/475, Strider 405/504, Mage 426, and 551/552 for all three.
  The unequipped 1185 remains in the bag.
- Installed `etc\questreward\questreward` (`rQuestReward`, game_main.arc)
  q0 has no items. q1 grants 64 x1 (Lantern), 1 x5 (Scrag of Beast), and
  43 x5 (Greenwarish). Its actual QCT clear is
  `etc\questCtrl\q0001_b00`, table 0 row 2 result type 16; reward dispatch
  is `0x00790539` through `0x0045e680`. Names were read from
  `id\message\item\itemName_eng`; reward pairs are read at runtime,
  never copied into implementation constants.

Implemented `ddeconomy::new_game` for the empty pre-editor bag and the two
post-editor phases, the
heart-taking mutation and first-choice/reward delivery. The save-free
`QuickItems::new_game` reads the install's item records and unaugmented
recovery factor. Each Arisen seat receives a distinct bag (co-op policy);
repeated first-choice/reward notifications do not duplicate items.

The opening Session constructs those bags at its actual preparation
boundary, gives the temporary equipment on editor confirmation, applies
the heart-taking swap on the real event-row-21 request, gives first-choice
items on menu close, and delivers the runtime `Request::Reward` pairs
after the real host frame. The existing headless intro bypass also applies
the completed heart-taking boundary; it does not leave prologue gear in
q1. U / Numpad0 call the same recovery adapter on the authoritative local
host after pause, cutscene, talk and menu gates.

Tested: **14 ddeconomy tests passed** (8 unit, 1 existing install, 3
item-use, 2 new bags), compile 11.02 s; new installed regression 4.36 s.
Full release viewer tests **62 passed, 2 existing ignored**, compile
18.52 s, tests 10.86 s. The new item test presses U and Numpad0: a hurt
Arisen cannot consume a curative before q1 clears; the actual reward then
permits one Greenwarish use, heals that seat and leaves the other bag and
HP untouched. A held key spends no additional item. The final intro phase
change additionally passed its focused installed test (compile 18.25 s,
test 5.12 s), checking the real Session's empty pre-editor bag and both
post-dragon Arisen bags. The shared M5 target initially reused another
worktree's ddeconomy artifact; only that package was cleaned (5.6 MiB),
then the actual current sources compiled and passed.
Normal release `ddview` build passed in 31.30 s, with the existing `Args`
visibility and unused `forget_actor` warnings.

No original live q1 bag comparison or rendered opening route is claimed.
Item input over LAN and persistence remain the existing QuickItems
limitations; this brief provides independent local host Arisen bags, as
--play quick use does. The heart-taking bag mutation does not model the
removed equipment's global buy-back side effect (`0x00461be0` ->
`0x00461f30`). Storage/platform bonus grants are separate systems; they
are not invented as items held by the new Arisen.

Next action: compare a fresh original q1 bag after ev10, then move item
commands and inventory ownership into the host/network save protocol.
