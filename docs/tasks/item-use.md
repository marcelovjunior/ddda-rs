# item-use: Using items: curatives, healing and inventory actions

- **Owner:** codex lane
- **Branch:** `task/item-use`
- **Board area:** `player/items`
- **Status:** working (core tested; viewer hookup in coordination with codex-18)

## Goal

Item use for the Arisen and pawns: healing, curatives (the four cure messages 0x12-0x15 and which items send them; per-status cure bytes at item +0x25..+0x33, still to verify), stamina items, throwables if quick, and the use action and motion. Built on `ddeconomy` bags and `ddcombat::cstatus`. Add a quick-use key in `--play`.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs must not grab the mouse or pause on focus loss.

## Scope

- `crates/ddeconomy/**`
- `crates/ddcombat/**`
- `crates/ddsim/**`
- `docs/SHOPS.md`
- `docs/STATUS.md`
- this brief's Results

Shared viewer files (`play.rs`, `main.rs`) need a board claim and
coordination with the agent that holds them.

## Done when

Tests: a potion heals the right amount; a curative removes poison per the traced rules; stamina items work. Usable from a key in `--play --save`. Fill in Results and report to `head`.

## Results

Core implementation: `ddeconomy::item_use::Recovery` reads installed ITL
amounts/cures and selects party targeting; `Inventory::use_recovery` applies
then consumes one copy from the named owner. Unsupported harmful, buff,
special and action-path items return an error without consumption.
`ddcombat` adds direct item cures and full-ceiling item healing; `ddsim`
adds positive stamina restoration with the curse cap. Codex-1 supplied
`Fight::recover_item`, with dead/missing-seat rejection and pawn-specific
possession eligibility (cherry-picked as 68f7a8b).

Evidence: menu `0x0066CCD0` -> use `0x00B77130` -> player `vf183`
`0x00B77780`. Signed i16 +0x70/+0x72 HP/stamina, augment 0x4a factor
from installed `plAbilityParam` via `0x00783340`, quarter-strength sharing
without cures. Item +0x38 bits12..14 ==4 is party targeting; mask0x78000
requires a separate action and is rejected here. Direct signed cure bytes
from `0x00856050`/getters `0x0045A8D0..0x0045B370`; assembly verifies
down-status byte order. Ordinary items do not emit broad cure messages
0x12..0x15. Their producers remain untraced. The previous STATUS.md
classification of `0x00856050` as a debug table was corrected.

Tested: `cargo test -p ddcombat -p ddeconomy -p ddsim`: 102 tests pass
(33/12/57), including installed Salubrious Brew +1500 HP, Greenwarish
+120 HP, Mushroom +250 stamina, Mithridate poison-only cure; owner
isolation, failed-use atomicity, wound recovery, no revival, stack cures,
signed bytes, sharing and curse cap. Targeted library Clippy passes with
warnings denied. Codex-1's three installed fight-status tests pass.

Viewer module `items.rs` (head scope approvals #1516/#1525) imports four
saved bags read-only, resolves augment factors from data, and offers the
first useful supported restorative for the named local seat. Three module
tests cover bag validation, optional private-save import and full potion /
stamina / poison application. Compilation and U-key hookup are pending
codex-18's coordinated build; this brief is not yet marked complete.

Not compared live: no original-game state was changed. No game files,
recordings or raw decompilation committed. The fight has no persistent
wounded-HP pool; item healing uses full HP maximum and must not be reused
unchanged for spells. Automatic pawn item decisions, viewer Medicant
propagation, network item commands, throwables, use animations and a full
inventory menu remain outside this slice.

Next action: codex-18 integrates the resource and U key into offline
`--play --save`, then compile/run the three module tests and a bounded
unattended demonstration.
