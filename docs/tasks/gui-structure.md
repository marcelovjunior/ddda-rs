# gui-structure: GUI resources (HUD, menus, loading screens)

- **Owner:** codex lane
- **Branch:** `task/gui-structure`
- **Board area:** `formats/gui`
- **Status:** open

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

(filled in by the agent)
