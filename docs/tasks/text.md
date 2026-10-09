# text: game text (dialogue, item and enemy names, UI strings)

- **Owner:** codex lane
- **Branch:** `task/text`
- **Board area:** `formats/text`
- **Status:** review

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

Implemented `mtgmd` (borrowed UTF-8 messages, ordered labels and zero-based
lookup), `ddtool msg-dump` in its own module, and `docs/TEXT.md` with binary
layout, language counts, file index, executable evidence and boundaries.

Verified 2026-10-09: `cargo test -p mtgmd -- --nocapture` passes 3 synthetic
tests and 1 install test. All 16,338 archive instances / 8,488 paths / 7,954
distinct payloads parse with zero failures; distinct payloads contain
305,099 messages and 93,846 labels. Value checks include item index 0
`Airtight Flask` and label `e0100_goblin` -> index 0 -> `Goblins`. CLI smoke
test prints all 112 enemy messages with their 111 labels.

Evidence: GMD loader `0x00dde1c0`, label-array accessor `0x00dde560`, exact
first-match label lookup `0x00dde580`, message lookup `0x00dde5f0` (DDDA.exe
2017-12-13 Steam). Read-only data matches the Riftstone format notes;
no game assets or reference code are committed. Other candidates surveyed:
six `rGUIFont` instances (three paths, GFD), one `rTalkMotion` (tmd v16;
loader `0x00ce8fd0` reads motion records).

Implemented and data/exe-tested; not compared in-game. Unknowns: engine
archive precedence, resource selection from higher-level scenario/FSM IDs,
markup rendering, and the invalid-index fallback pointer's contents.
Enemy label prefixes are not a reliable universal model-ID mapping.
Next action: trace one NPC interaction's text selection and rendering.

Shared manifest/lock additions and minimal main.rs hookup are explicitly
authorized by head on board #65 / codex-1 #67; head resolves merge hooks.
