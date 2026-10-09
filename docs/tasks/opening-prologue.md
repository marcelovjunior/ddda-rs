# opening-prologue: The playable prologue (stage 380)

- **Owner:** codex lane
- **Branch:** `task/opening-prologue`
- **Board area:** `story/prologue`
- **Status:** open

Part of the opening slice. Read `docs/OPENING_SLICE.md` first,
especially its rule: build the system, not the step.

## Goal

Replace the prologue fast-forward with the real prologue: stage 380, the pre-made hero and party, its fights, the `fsm\fix\*` scripts (SceNo 200 -> 900), event st380 ev00 and the prologue end (0x00444b20 -> stage 800). Trace the hero's setup (equipment, vocation and party) and the prologue's enemies and their spawns from the data.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs use `--unattended`.

## Scope

- `crates/ddstory/**`
- `crates/ddhost/**`
- `crates/ddview/src/opening.rs` (claim)
- `docs/QUESTS.md` (prologue section)
- this brief's Results

Shared viewer files need a board claim and coordination with the
agent that holds them.

## Done when

`--opening` starts at the new game and plays the prologue through to stage 800, with SceNo and GSF matching the traced sequence at each step. Enemy families that are missing are listed, or built if they're small. Fill in Results and report to `head`.

## Results

(filled in by the agent)
