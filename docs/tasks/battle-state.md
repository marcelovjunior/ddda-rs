# battle-state: The shared "in battle" state

- **Owner:** codex lane
- **Branch:** `task/battle-state`
- **Board area:** `fight/battle-state`
- **Status:** open

## Goal

codex-34's lead: sGameSys +0xBE2CC is written by 0x00441650, called from
sGameSys vf06 0x0043CEB0. It takes an explicit trigger (+0xBE2CD) or scans
the hostile list (+0xBE25C/+0xBE268): live health, vf108 == 2, predicate
0x007D3F50, distance squared < 6,250,000 (25 m), accumulating +0xBE2D4 to
the threshold +0xBE2D0; the latch clears through 0x00441800 after
+0xBE2DC >= +0xBE2D8. Pawn code currently has a placeholder for it.

Translate the battle state as one shared host system (N players: which
Arisen's distance counts, labelled), and find its consumers: pawn AI
(battle vs travel behaviour), battle music, HUD, sheathing, ambient
creatures' flight. Wire the existing placeholder(s) to it.

**Rules.** Trace in the exe and label any guesses (AGENTS.md).

## Scope

`crates/ddhost` (new battle-state module), `crates/ddpawnai` and other
consumers' inputs, `docs/COMBAT.md` section, this brief's Results.

## Done when

The battle state rises and falls as the original's in a goblin fight
(oracle comparison of +0xBE2CC over the fight), and its consumers read it.
Fill in Results and report to `head`.

## Results

(filled in by the agent)
