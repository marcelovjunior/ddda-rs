# skills-runtime: Vocation skills in combat

- **Owner:** codex lane
- **Branch:** `task/skills-runtime`
- **Board area:** `fight/skills`
- **Status:** open

## Goal

Make equipped skills usable: the `ddskill` catalogue (action ids, the cPlActWpn* classes, motion lists), stamina costs, and effects, starting with the Fighter (sword skills) and the Mage (staff spells: Ingle, Anodyne, and Levin if reachable). Spells are shells (`shl_param`), like WandShot.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs must not grab the mouse or pause on focus loss.

## Scope

- `crates/ddsim/**`
- `crates/ddenemyrt/**`
- `crates/ddskill/**`
- `docs/VOCATIONS.md`
- `docs/COMBAT.md`
- this brief's Results

Shared viewer files (`play.rs`, `main.rs`) need a board claim and
coordination with the agent that holds them.

## Done when

Two skills per vocation work headlessly against a goblin with traced costs and damage, and from keys in `--play`. Fill in Results and report to `head`.

## Results

(filled in by the agent)
