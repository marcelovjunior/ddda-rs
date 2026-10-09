# npc-dialogue-links: NPC placement, ledger and dialogue links

- **Owner:** codex lane
- **Branch:** `task/npc-dialogue-links`
- **Board area:** `world/npc`
- **Status:** open

## Goal

Readers for the NPC ledger, MSS and MSL formats, joined with the existing layout (`mtlot`) and text (`mtgmd`) readers. Then trace how the NPC manager selects schedules, separately from generic dialogue playback.

## Evidence

Start from `docs/SURVEY.md`, section "6. NPC placement, schedules and dialogue": the resource classes,
counts, sample structures, exe loader addresses and reference coverage
are there. The tools are listed in `docs/tasks/README.md`.

## Scope

- `crates/ddnpc/**`
- `docs/NPCS.md`
- `crates/ddtool/src/<name>.rs`, if you add a CLI module (shared hookups as in `docs/CODEX.md`)
- this brief's Results

## Done when

All three new formats parse across the install, and one named NPC (named from the game's text) is traced end to end: placement, schedule entry, and dialogue lines. Coordinate with claude-7 (events-quests), who owns quest and event logic. Distinguish traced from guessed, and fill in Results. Then report
to `head`.

## Results

(filled in by the agent)
