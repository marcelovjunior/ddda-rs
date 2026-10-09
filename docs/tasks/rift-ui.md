# rift-ui: the Rift menus

- **Owner:** codex-15 (Codex lane)
- **Branch:** `task/rift-ui`
- **Board area:** `formats/gui`
- **Status:** review

## Goal

Document the Rift's menus as the original builds them, ready for a UI
implementation:
- the screens and node trees (`pawn2` GUI files, through `mtgui`);
- the strings (`pawn_search_<lang>` and related, through `mtgmd`);
- which menu item maps to which search category: the list builder is
  `0x0047a550`, and the category enum is in `docs/PAWNS.md`;
- what each row shows (level, vocation, cost, flags).

## Scope

- `docs/RIFT_UI.md` (new)
- `crates/mtgui/**`: readers only, if a format gap needs filling

## Done when

Each Rift screen's tree, strings and data bindings are documented with
exe addresses, or the gaps are named. Fill in Results and report.

## Results

Added `docs/RIFT_UI.md`: five screen/resource trees (six archive copies,
573 unique-path nodes / 637 occurrence nodes), executable loaders, runtime
message families, PC menu category mappings and row bindings. All 49
message paths / 105 occurrences in the surveyed seven families parse;
this includes the older non-DDN skill-name family for comparison, not as a
claimed loader target. Existing `mtgui` requires no change.

The key new finding is GUI-side offline filtering: `0x006d4bb0` constructs
exclusion bytes via `0x006d4c50` (vocation, any matching weapon-skill category,
gender); `0x006d5090` skips them in a seven-row window. This fills the
explicit GUI-filter gap in PAWNS.md. Disassembly confirms ECX category
arguments in `0x006d0350` / `0x006d0b50` and the distinction between node
array indices and instance IDs in the row setter (`0x01077560`).

Validation: seven existing `mtgui` synthetic tests pass; targeted read-only
VFS/reader survey checked every listed resource occurrence. No runtime code
was translated and no new tests added for this documentation-only change.
No original-game render/input comparison; no process writes requested.

Open gaps are enumerated per screen and in the report: detailed state
transitions, filter writers, remaining category callers, ranking fields,
detail/result/review bindings and rendering. Next: a tested, per-viewer
headless list projection and comparison with an existing offline Rift
capture or a maintainer-operated session. No changes to shared milestones,
roadmap, PAWNS.md, or the game install.
