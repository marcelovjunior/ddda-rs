# chara-edit: Character creation data and its gameplay effects

- **Owner:** codex lane
- **Branch:** `task/chara-edit`
- **Board area:** `player/customization`
- **Status:** open

## Goal

The character editor: its data (body, face and morph parameters, presets, voice), how a saved Arisen or pawn appearance is stored (cSAVE_DATA_EDIT and similar; see dti_props), and its gameplay effects: height and weight to the body class (PARAMS.md, 0x00B81120), carrying capacity, stamina and the ground-speed factor uPlayer+0xDA8 (writer 0x00B6B350, scale from edit records at 0x0081C170; the recorded value is 0.8471 for the maintainer's Arisen).

## Evidence

- The docs on main: SURVEY, PAWNS, RIFT_UI, QUESTS, EVENTS, PARAMS,
  WORLD, ORACLE.
- Reflected field names: `research/dti/dti_props.py show <Class>`.
- Live reads: read-only, through claude-4's oracle on the board.
- The tools are listed in `docs/tasks/README.md`.

## Scope

- `crates/ddedit/**`
- `docs/CHARA_EDIT.md`
- `crates/ddtool/src/<name>.rs`, if you add a CLI module (shared hookups as in `docs/CODEX.md`)
- this brief's Results

## Done when

The edit records of the save's Arisen and main pawn decode. The 0.8471 factor is reproduced from the Arisen's edit data, or the missing link is named precisely. The body-class thresholds are documented with addresses. Distinguish traced from guessed, and fill in Results. Then report
to `head`.

## Results

(filled in by the agent)
