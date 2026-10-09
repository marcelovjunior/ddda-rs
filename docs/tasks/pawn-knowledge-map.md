# pawn-knowledge-map: Pawn knowledge: save bits to quests, enemies and areas

- **Owner:** codex lane
- **Branch:** `task/pawn-knowledge-map`
- **Board area:** `formats/save`
- **Status:** review

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

Added `ddsave::knowledge`: installed-exe readers for 52 quest, 97 enemy,
18 area rows; 92 enemy-to-counter mappings; 113 unique-counter rules.
Pure per-pawn bit progress, source f32 stars, completion predicate, area
reverse mapping and counter gates. Reuses existing strict pawn-save reader.
Minimal ddexe dependency/module/lock extension approved by head #1401.

Five new tests pass; full ddsave suite passes 18. Private original-written
main pawn examples: quest5=6/6 (3 stars), enemy8=2/11 (1), area210=3/3 (3).
Encounter/kill and unique-counter examples also resolve. Source assembly
proves encounter equality passes, unlike a misleading decompiler expression.
Targeted Clippy passes allowing the existing field_reassign_with_default
finding in unchanged pawns.rs. Details/addresses in PAWN_KNOWLEDGE.md.

No original-menu visual comparison or automatic award/increment pipeline
implemented. Eleven enemy counter IDs have no menu row and are preserved;
bulk tables read from the install, no copied game data or identities.
Next: compare live menu tabs and trace counter increment/notification rules.
