# gui-structure: GUI resources (HUD, menus, loading screens)

- **Owner:** codex lane
- **Branch:** `task/gui-structure`
- **Board area:** `formats/gui`
- **Status:** review

## Goal

A reader for the `rGUI` section and resource tables, with links to textures and messages.

## Evidence

Start from `docs/SURVEY.md`, section "3. GUI, HUD, menus and loading screens": the resource classes,
counts, sample structures, exe loader addresses and reference coverage
are there. The tools are listed in `docs/tasks/README.md`.

## Scope

- `crates/mtgui/**`
- `docs/GUI.md`
- `crates/ddtool/src/<name>.rs`, if you add a CLI module (shared hookups as in `docs/CODEX.md`)
- this brief's Results

## Done when

Every GUI file parses, and its texture and message links resolve. The node trees of the loading screen and the options menu are documented in GUI.md. Rendering and input are out of scope. Distinguish traced from guessed, and fill in Results. Then report
to `head`.

## Results

Implemented `mtgui`: bounded section table reader, typed instance trees,
constant properties, named animation definitions, texture/font/message/sub-GUI
references and reflection-filter texture links. No rendering or input.

Evidence: loader/fixups `0x01075780` / `0x01076170`, font filters
`0x01075c20`, instance construction `0x00e0b840`, child/sibling traversal
`0x010777a0`. Source comments and synthetic regression tests accompany the
translated structure rules. `docs/GUI.md` documents the format, both loading
screen trees and the options-menu hierarchy; the `inspect` example prints the
full tree from the player's install.

Verified 2026-10-09: 116 paths / 216 archive occurrences parse; 8,091 nodes;
all 4,418 non-null texture/font/sub-GUI references resolve. All serialized GUI
message tables are empty. Separately traced the options message load at
`0x006a6160` and suffix selection at `0x00607610`; all 7 language resources,
29 archive copies / 8,850 messages parse with mtgmd. Tests: 7 synthetic + 2
installation tests, all passing (`cargo test -p mtgui -- --include-ignored`
with DDDA_NATIVE_PC set). No original-game visual or input comparison.

Unknowns: nested animation/sequence semantics and spans, extra object data,
font/icon readers, runtime localized resource precedence, per-control message
indices, and the loading-tip resource assigned at sGameSys +0xbe344. The latter
is a runtime dependency, not a missing serialized GUI reference; no guessed
join was introduced. Next: trace that resource assignment and options
control-to-message bindings before a per-instance headless GUI state model.

No game/private data or decompiler exports committed. Shared milestone and
roadmap updates are left to head.
