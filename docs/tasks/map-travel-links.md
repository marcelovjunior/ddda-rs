# map-travel-links: World map, landmarks and fast travel

- **Owner:** codex lane
- **Branch:** `task/map-travel-links`
- **Board area:** `world/map`
- **Status:** review

## Goal

Typed readers for landmarks, start positions and map names, plus an exe trace of the travel-item consumer (Ferrystone, Portcrystal).

## Evidence

Start from `docs/SURVEY.md`, section "8. World map and fast travel": the resource classes,
counts, sample structures, exe loader addresses and reference coverage
are there. The tools are listed in `docs/tasks/README.md`.

## Scope

- `crates/ddmap/**`
- `docs/MAP.md`
- `crates/ddtool/src/<name>.rs`, if you add a CLI module (shared hookups as in `docs/CODEX.md`)
- this brief's Results

## Done when

All map resources parse and their text ids resolve. MAP.md explains, with addresses, how a destination is chosen and how the item is consumed. The teleport itself is later core work. Distinguish traced from guessed, and fill in Results. Then report
to `head`.

## Results

Implemented `ddmap`: typed XFS landmarks/start positions and SPN v0x30
place names, plus explicit caller-selected GMD name resolution. No simulation,
rendering or global player state.

Validation: `cargo test -p ddmap -- --nocapture` passed two synthetic tests
and one installed-corpus regression. All 3,400 archive copies parse:
3,251 landmark copies / 51,323 rows; 126 start files / 506 rows; 23 place-name
files / 208 rows. All 207 non-sentinel text IDs resolve in all seven map
language tables; 5,733 unnamed landmark occurrences retain u32::MAX. The
st000 three-offset start layout and all its truncated prefixes are checked.

`docs/MAP.md` records source addresses for layouts, map name lookup, menu
destination selection into per-actor item-use queues, ten saved Portcrystal
slots, placement/pickup and inventory consumption. Ordinary Ferrystone and
Portcrystal consume quantity one via b77130 -> 45f480 -> 45f4f0 -> 4588c0
-> 459880; item1724 Eternal Ferrystone has an explicit exception. No game
bytes, decompilations or private research files are committed.

Not implemented/verified against running original: destination availability
gates, final stage transition/arrival, full SPN HUD consumer, item-add failure
handling, multiplayer ownership/replication. Next action: trace destination
queue through stage transition and compare allowed/failing travel flows
before headless host-authoritative implementation.
