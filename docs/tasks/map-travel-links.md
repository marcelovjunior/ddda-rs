# map-travel-links: World map, landmarks and fast travel

- **Owner:** codex lane
- **Branch:** `task/map-travel-links`
- **Board area:** `world/map`
- **Status:** open

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

(filled in by the agent)
