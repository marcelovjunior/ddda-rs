# stream-rules: World streaming: when cells and assets load and unload

- **Owner:** codex lane
- **Branch:** `task/stream-rules`
- **Board area:** `world/streaming`
- **Status:** open

## Goal

Research only, no engine code. Find the original's streaming rules: cell load and unload radii and hysteresis around the player (cSplitSbc, the split loader 0x00C5F680, uStageSplitMdl), the detail and far rings, when scenery, enemies and NPC models are spawned by distance, and how the floating origin re-centres (sWorldOffset; claude-4 found +0x2c/+0x30 meaning unknown). Read-only oracle snapshots of the loaded-cell set while the player stands still are fine; ask claude-4.

## Evidence

- The docs on main: SURVEY, PAWNS, RIFT_UI, QUESTS, EVENTS, PARAMS,
  WORLD, ORACLE.
- Reflected field names: `research/dti/dti_props.py show <Class>`.
- Live reads: read-only, through claude-4's oracle on the board.
- The tools are listed in `docs/tasks/README.md`.

## Scope

- `docs/STREAMING.md`
- `crates/ddtool/src/stream_survey.rs`
- `crates/ddtool/src/<name>.rs`, if you add a CLI module (shared hookups as in `docs/CODEX.md`)
- this brief's Results

## Done when

STREAMING.md gives the radii, hysteresis, per-class spawn distances and the origin re-centring rule, each with exe addresses and at least one read-only live snapshot. It ends with a proposed core design for ddworld/ddview (the head assigns that work). Distinguish traced from guessed, and fill in Results. Then report
to `head`.

## Results

(filled in by the agent)
