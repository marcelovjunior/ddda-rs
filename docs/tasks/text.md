# text: game text (dialogue, item and enemy names, UI strings)

- **Owner:** codex lane
- **Branch:** `task/text`
- **Board area:** `formats/text`
- **Status:** open

## Goal

Read every text resource in the install: dialogue, names, menu strings,
in every language shipped. NPCs, quests, items, UI and the other briefs
all need names from the game's own text rather than from memory.

## Evidence and starting points

- `rGUIMessage` is in `mtarc::RESOURCE_CLASSES`. In other MT Framework
  games this is the `.gmd` message format. Confirm its layout from the
  files. Check whether any other type also carries text (search
  RESOURCE_CLASSES for `Message`, `Text`, `Talk`, `Font`).
- Find where they live: `vfs.paths_of_type(mtarc::type_hash("rGUIMessage"))`.
  Note the path patterns and the per-language variants.
- Exe: the `rGUIMessage` DTI class and its loader in `dti-labels.csv`.
  How a message is looked up (by index? by label hash?) matters to every
  caller.
- Specs: check `E:\ddda-re\refsrc` for GMD notes (MIT). Verify against
  the files.

## Scope

- `crates/mtgmd/**` (new)
- `crates/ddtool/src/main.rs`: you may add one `msg-dump` subcommand
- `docs/TEXT.md` (new)

## Deliverables

1. `mtgmd`: parses a message file into labels (if present) and strings,
   with the right encoding. Tests: every instance in the install parses
   with 0 failures (skipped when the install is absent), plus a
   value-checked test on one known string.
2. `ddtool msg-dump <nativePC> <path>`.
3. `docs/TEXT.md`:
   - the format;
   - the languages and file naming;
   - how the exe looks messages up;
   - an index of which files hold what (item names, enemy names, NPC
     dialogue, quest text, UI), found by inspection.

## Done when

All text resources parse, tests pass, and TEXT.md has the file index.
Fill in Results and report to `head`. Tell `all` on the board when the
reader is usable: the enemy-data and asset-survey agents want it.

## Results

(filled in by the agent)
