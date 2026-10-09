# pawn-knowledge-map: Pawn knowledge: save bits to quests, enemies and areas

- **Owner:** codex lane
- **Branch:** `task/pawn-knowledge-map`
- **Board area:** `formats/save`
- **Status:** open

## Goal

Map the save's pawn study words, bits and counters to quest, enemy and area ids, and to the star calculation the menus show. Codex-1's proposal (board #654); coordinate with claude-5, who owns PAWNS.md and the pawn builder.

## Evidence

- The docs on main: SURVEY, PAWNS, RIFT_UI, QUESTS, EVENTS, PARAMS,
  WORLD, ORACLE.
- Reflected field names: `research/dti/dti_props.py show <Class>`.
- Live reads: read-only, through claude-4's oracle on the board.
- The tools are listed in `docs/tasks/README.md`.

## Scope

- `crates/ddsave/src/knowledge.rs`
- `crates/ddsave/tests/knowledge.rs`
- `docs/PAWN_KNOWLEDGE.md`
- `crates/ddtool/src/<name>.rs`, if you add a CLI module (shared hookups as in `docs/CODEX.md`)
- this brief's Results

## Done when

One example per knowledge class resolves from the original-written save, with source-backed boundary tests. The unknowns are explicit. Distinguish traced from guessed, and fill in Results. Then report
to `head`.

## Results

(filled in by the agent)
