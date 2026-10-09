# asset-survey: where every remaining system's data lives

- **Owner:** codex lane
- **Branch:** `task/asset-survey`
- **Board area:** `survey`
- **Status:** open

## Goal

For each system in `docs/ROADMAP.md` marked "not started", find the data
and the exe classes behind it, so the head can write precise briefs. This
is research. Code is optional: scratch tools are fine, but only a
`ddtool` subcommand may be committed.

## Areas to cover (one section each)

1. Sound and music: banks, requests, streams; codec and container.
2. Visual effects: `rEffectList` (`.efl`) and its parts.
3. GUI: HUD, menus, settings screen, loading screens (`rGUI*`).
4. Items and inventory: item tables, icons, equipment stats.
5. Quests, events, cutscenes: quest data, event scripts, camera paths.
6. NPCs: placement layouts, schedules, dialogue links.
7. Day/night and weather: sky, light, fog, time-of-day parameters.
8. World map and fast travel: map GUI, portcrystals, ferrystones.
9. Saves: the save file format (location and structure; read-only).
10. Status effects: `rStatusParam` and its consumers. Note overlaps with
    the `enemy-data` brief on the board.

## For each area, report

- Resource types (exact class names from `mtarc::RESOURCE_CLASSES`),
  instance counts (`vfs.paths_of_type`), path patterns, and one sample
  file's structure (XFS dump or a hex summary).
- The exe classes that load or use them (`dti-labels.csv`, addresses).
- Existing specs in `E:\ddda-re\refsrc` (MIT) and how far they go.
- **A proposed brief:** a reader or rule to build first, its scope, and its
  "done when".

## Scope

- `docs/SURVEY.md` (new)
- `crates/ddtool/src/main.rs`: you may add one `survey` subcommand

## Done when

All 10 areas have a section with types, counts, exe classes and a
proposed brief. Fill in Results and report to `head`.

## Results

(filled in by the agent)
