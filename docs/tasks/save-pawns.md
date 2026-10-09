# save-pawns: typed pawn knowledge and equipment from the save

- **Owner:** codex lane
- **Branch:** `task/save-pawns`
- **Board area:** `formats/save`
- **Status:** review

## Goal

Extend the read-only `ddsave` crate with typed pawn knowledge (quest,
enemy and area knowledge), equipment, skills and inclinations, and add a
`ddtool save-pawns` dump. This is proposal 4 in `docs/PAWNS.md`, which
maps the XML fields in §8.

## Scope

- `crates/ddsave/**`. Do not change the existing APIs without telling
  `claude-5`, who works on `pawn-builder` in `ddpawn` and may add fields
  here: claim carefully and coordinate on the board.
- `crates/ddtool/src/save_pawns.rs`
- `docs/PAWNS.md` §8: a small section only; coordinate with `claude-5`.

## Done when

- Every pawn record in a private save copy decodes into typed fields with
  bounds checks, and the reader handles missing fields.
- Synthetic tests and a private-save test that skips without a copy.
- No personal data is printed or committed. Fill in Results and report.

## Results

Schema survey (private copy, no names or account values printed): 23
`cSAVE_DATA_CMC` records, including both player slots, close friends,
Kaiou records and New Game+ system records. The reflected XML stores
322 study words, 322 local study words, 72 encounter-frame floats,
72 kill counters and 116 unique counters; twelve equipment slots and
twelve six-slot weapon-skill arrays.

Built `ddsave::pawns::PawnFields` as an additive API: optional fixed arrays,
full item payloads, named weapon-skill banks and typed inclination members.
Present fields are checked for scalar width/type, array declared/actual
lengths, duplicate names, nested class and finite floats. Missing remains
`None`; signed sentinel values are retained. Existing `pawn.rs` is unchanged.

`ddtool save-pawns` prints structural counts only (23 pawn/equipment/study
records and 276 weapon-skill arrays in the private copy). No identity data
is decoded or printed by the new module. No save writes.

Validation: four synthetic tests plus the private-save test pass; all seven
existing crate unit tests pass; `cargo check -p ddtool` and the CLI against
the private copy pass. Tests cover all 23 CMC records, including both
player slots and system records.

Head #594 moved notes to `docs/SAVE_PAWNS.md` to avoid claude-5's PAWNS.md
edits; #615 narrowed the shared claim. Only the additive lib export and
minimal ddtool/Cargo hooks touch shared files.

Unresolved: study word/bit to quest/enemy/area IDs and star ratings, skill
bitsets, item flags and inclination display ranking. This is typed storage
validation against original-written data, not behavior/UI comparison.
Next: trace knowledge evaluators and add identifier mappings with tests.
