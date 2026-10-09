# enemy-data: enemy statistics readers

- **Owner:** codex lane
- **Branch:** `task/enemy-data`
- **Board area:** `formats/enemy`
- **Status:** open

## Goal

Read every file that defines an enemy's numbers (health, attack, defence,
resistances, scale, experience) into typed Rust, and document what each
field means. This is the data side of M4 (Fight) and of enemy AI.

## Evidence and starting points

- Types (all in `mtarc::RESOURCE_CLASSES`):
  - `rPropParam`: binary, magic `PRPZ`. Attack, defence, resistances,
    scale, EXP.
  - `rRegionStatus` (`.rst`): a creature's health, `mHPMax`.
  - `rStatusParam`: XFS.
  - `rShlParamList`: XFS, shell (projectile) parameters.
  - `rAdjustParam` (`ajp`): format in `docs/FORMATS.md`. Enemies have
    their own.
- Riftstone's notes describe several of these (MIT, private copy):
  `E:\ddda-re\refsrc\riftstone_formats.md`, around the lines mentioning
  `rPropParam`, `rRegionStatus` and the per-byte resistance table. Read
  them and cite them. Verify every claim against the files yourself.
- Enemy ids look like `em\e01\e0100`. Find where each type lives with
  `vfs.paths_of_type(..)`.
- Exe: the DTI class names (`dti-labels.csv`) of these resources and
  their loaders. A field's meaning is *traced* only when you saw the code
  that uses it.

## Scope

- `crates/mtprp/**` (new crate for the binary `PRPZ` format)
- `crates/ddenemy/**` (new crate: typed loaders for all of the above, per enemy id)
- `docs/ENEMIES.md` (new)

## Deliverables

1. Readers with tests that parse **every** instance of each type in the
   install (skipped when the install is absent), with counts reported.
2. `docs/ENEMIES.md`:
   - the formats;
   - field meanings marked traced or guessed;
   - a table of a few common enemies (goblin, wolf, saurian, harpy,
     cyclops): id, HP, key resistances, EXP, scale.
3. Do not name enemies from memory. Leave names blank, or take them from
   the game's own text once the `text` brief lands (ask on the board).

## Done when

All instances parse with 0 failures, tests pass, and ENEMIES.md
separates traced from guessed fields. Fill in Results and report to
`head`.

## Results

(filled in by the agent)
