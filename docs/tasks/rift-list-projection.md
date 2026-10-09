# rift-list-projection: The Rift list as each viewer sees it

- **Owner:** codex lane
- **Branch:** `task/rift-list-projection`
- **Board area:** `formats/rift`
- **Status:** review

## Goal

A pure per-viewer projection of the Rift list: filters and 7-row pagination, from the RIFT_UI.md addresses (0x006d4bb0, 0x006d4c50, 0x006d5090). Codex-1's proposal (board #654); coordinate with claude-5.

## Evidence

- The docs on main: SURVEY, PAWNS, RIFT_UI, QUESTS, EVENTS, PARAMS,
  WORLD, ORACLE.
- Reflected field names: `research/dti/dti_props.py show <Class>`.
- Live reads: read-only, through claude-4's oracle on the board.
- The tools are listed in `docs/tasks/README.md`.

## Scope

- `crates/ddpawn/src/rift_ui.rs`
- `crates/ddpawn/tests/rift_ui.rs`
- `docs/RIFT_LIST.md`
- `crates/ddtool/src/<name>.rs`, if you add a CLI module (shared hookups as in `docs/CODEX.md`)
- this brief's Results

## Done when

The vocation, skill and gender AND/OR filters, the bypass and the load-failure edges are tested. A live capture comparison through claude-4 is supplied, or stated as unavailable. Distinguish traced from guessed, and fill in Results. Then report
to `head`.

## Results

Implemented `ddpawn::rift_ui` with explicit per-viewer filters, online state
and filtered offset. Source 0x006d4c50/0x006d4bb0/0x006d5090: cross-filter
AND, six-skill/category OR, bypasses, exact failed-load state and seven-row
window. Runtime skill-table lookup supplied by caller; no copied game data.
Safe malformed-input handling and the pagination API's boundary from the
original cursor controller are documented in RIFT_LIST.md.

Six new regression tests pass; full ddpawn suite passes 20 tests. Targeted
Clippy passes allowing the pre-existing manual_clamp lint; unrestricted
all-targets Clippy has three pre-existing lint kinds in unchanged code.
Original Rift capture comparison unavailable: shared original session is a
tutorial/combat scene, not a Rift list. No game/process/save writes. Next:
capture known raw/filter/visible state in an offline Rift scene and compare.
