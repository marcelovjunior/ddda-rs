# rift-ui: the Rift menus

- **Owner:** codex lane
- **Branch:** `task/rift-ui`
- **Board area:** `formats/gui`
- **Status:** open

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

(filled in by the agent)
