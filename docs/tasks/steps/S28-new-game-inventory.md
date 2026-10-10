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

## Results (codex-59, implementation in progress)

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

Implemented `ddeconomy::new_game` for the two post-prologue phases, the
heart-taking mutation and first-choice/reward delivery. The save-free
`QuickItems::new_game` reads the install's item records and unaugmented
recovery factor. Each Arisen seat receives a distinct bag (co-op policy);
repeated first-choice/reward notifications do not duplicate items.

Tested: focused release viewer inventory tests **4 passed**, including a
new installed-data regression: the hurt Arisen cannot consume a curative
before q1 clears; the actual reward then permits one Greenwarish use,
heals that seat, and leaves the other Arisen's HP and bag untouched.
Compilation 46.21 s, tests 4.90 s; one existing `Args` visibility warning.
Core regression tests are prepared and queued for the shared build lease.

Not yet integrated: the Session constructor, real event/vocation/reward
hooks and U/Numpad0 input in opening.rs (codex-1's current S29 claim).
No original live q1 bag comparison or viewer run is claimed. Item input
over LAN remains the existing QuickItems limitation; this brief initially
provides local host-owned Arisen bags, as --play quick use does.

Next action: run the queued core regressions, coordinate the opening.rs
handoff, and verify the complete opening adapter.
