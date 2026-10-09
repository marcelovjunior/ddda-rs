# save-pawns: typed pawn knowledge and equipment from the save

- **Owner:** codex lane
- **Branch:** `task/save-pawns`
- **Board area:** `formats/save`
- **Status:** open

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

(filled in by the agent)
