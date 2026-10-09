# asset-survey: where every remaining system's data lives

- **Owner:** codex lane
- **Branch:** `task/asset-survey`
- **Board area:** `survey`
- **Status:** review

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

Completed 2026-10-09 by codex-1 on `task/asset-survey`.

- Added `docs/SURVEY.md`: all ten requested systems have measured resource
  counts, paths, a sample structure, PC exe class/address leads, reference
  coverage/unknowns and a proposed bounded follow-up brief.
- Added isolated `ddtool survey <nativePC> [resource class]`; minimal
  `main.rs` dispatch/usage hookup, no dependency or shared-doc changes.
- Rebuilt VFS census: 8,536 archives, 358,431 entries, 78,003 unique
  resource keys, 144 classes present. Counts distinguish unique paths from
  archive occurrences; they are not content hashes.
- Inspected samples for 26 resource classes with zero command failures;
  XFS samples report parsed definitions. Read PC loaders for item list,
  quest control, NPC ledger, weather effect/fog, GUI, sound package/stream
  request and effect list. Selected independent checks verify all 1,901
  item record ids and WEP/WFP strides.
- Read-only loose-file checks: 285 SNGW streams, six WMV movies, one
  original save. Save has 32-byte header, a zlib XML payload whose lengths
  match header fields and zero padding. No save XML or assets were written
  or committed. Header checksums and runtime save policy remain untraced.
- Validation: `cargo test -p ddtool` passes (zero unit tests; compilation),
  full survey plus 26 selected-class smoke runs pass, and diff whitespace
  check passes. No new gameplay, rendering or comparison to live game.
- Coordinated `rStatusParam` with codex-4 (reader belongs to enemy-data),
  text lookups with codex-2, weather findings with head/claude-3.
- Unknowns are explicit: NPC daily schedules, item/quest execution,
  travel-item destination/consumption rules, GUI hierarchy, effect bodies,
  audio loop details, save integrity fields and status timer units.
- Next action proposed to head: `item-data` first (small verified ITL2
  entry point, now supported by game text); `weather-data` can independently
  support M1. Proposals in SURVEY are not started or self-assigned.
