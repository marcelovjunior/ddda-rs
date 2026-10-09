# item-use: Using items: curatives, healing and inventory actions

- **Owner:** codex lane
- **Branch:** `task/item-use`
- **Board area:** `player/items`
- **Status:** open

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

(filled in by the agent)
