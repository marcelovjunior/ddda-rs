# enemy-placement: Enemy placement and spawn rules per area

- **Owner:** codex lane
- **Branch:** `task/enemy-placement`
- **Board area:** `world/enemies`
- **Status:** open

## Goal

Find what spawns where, and when: enemy-group layouts per stage cell, the layout-flag gating (claude-7 found the layout flags at sGameSys+0xadee8 and an open question about which group field names its flag), day and night variants, respawn after days, and story-progress variants. Readers plus an exe trace of the spawner.

## Evidence

- The docs on main: SURVEY, PAWNS, RIFT_UI, QUESTS, EVENTS, PARAMS,
  WORLD, ORACLE.
- Reflected field names: `research/dti/dti_props.py show <Class>`.
- Live reads: read-only, through claude-4's oracle on the board.
- The tools are listed in `docs/tasks/README.md`.

## Scope

- `crates/ddspawn/**`
- `docs/SPAWNS.md`
- `crates/ddtool/src/<name>.rs`, if you add a CLI module (shared hookups as in `docs/CODEX.md`)
- this brief's Results

## Done when

Every enemy-group layout parses. One area near Cassardis is fully explained (which groups, which conditions, respawn timing) with exe addresses, and compared, if possible, with a read-only oracle snapshot of the live enemies there. Distinguish traced from guessed, and fill in Results. Then report
to `head`.

## Results

(filled in by the agent)
