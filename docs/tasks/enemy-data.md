# enemy-data: enemy statistics readers

- **Owner:** codex lane
- **Branch:** `task/enemy-data`
- **Board area:** `formats/enemy`
- **Status:** review

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

Implemented `mtprp` and headless `ddenemy` readers, with full XFS trees
retained alongside common-stat, status-entry and shell convenience types.
RST exposes every region/element; adjust values remain indexed. Added
`EnemyId`/`EnemyData` loading for observed path conventions, with missing
resources represented explicitly, and an `inspect` example.

Evidence: PC `rPropParam::vf10` 0x00fde3c0 verifies the PRPZ wrapper;
`rRegionStatus::vf10` 0x00cd9f70 and element reflection 0x00cda9a0 establish
RST disk fields. PRP names/types were checked against serialized UTF-8
labels. [ENEMIES.md](../ENEMIES.md) distinguishes traced layout from
label-derived guesses and unknown runtime semantics, and includes five
sample enemies named through original English message labels supplied by
the text task, not memory. Label-to-resource numeric matching remains an
explicit inference.

Verified with `cargo test -p mtprp -p ddenemy -- --nocapture`: eight
synthetic unit tests and one exhaustive local-install test. Zero failures
across **4,386 archive occurrences** (including duplicate paths): PRP
2,530/229 paths, RST 110/106, status 2/2, shell 490/22, adjust 1,254/73.
Common-stat decoding covers 2,444 occurrences; 91 em#### bundles load.
Tests skip the installation check when nativePC is absent.

Not compared with original combat. Unknown: resistance formulas and
status categories/units, species-specific and shell-subclass consumers,
region/body-part selection, effective encounter HP, resource-list aliases
and archive precedence. No simulation rules are invented; all unadapted
XFS fields remain available with their stored labels and types.

Next action: trace one enemy's resource-list selection and damage consumer
to connect raw PRP resistance values with the selected RST region/element.
