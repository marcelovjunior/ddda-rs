# opening-movie: Movie playback (st100ev00.wmv)

- **Owner:** codex lane
- **Branch:** `task/opening-movie`
- **Board area:** `story/movie`
- **Status:** open

Part of the opening slice. Read `docs/OPENING_SLICE.md` first,
especially its rule: build the system, not the step.

## Goal

Play the game's WMV movies from the install, starting with `movie\event\st100ev00.wmv` (the beach and the dragon) at q0000. Decode them at run time (Windows Media Foundation through the `windows` crate is the likely route; no transcoded copies on disk), with audio, a skip key if the original has one (trace it), and the subtitle source if there is one.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs use `--unattended`.

## Scope

- `crates/ddview/src/movie.rs`
- `crates/ddstory/**` (request only)
- `docs/EVENTS.md` (movie section)
- this brief's Results

Shared viewer files need a board claim and coordination with the
agent that holds them.

## Done when

The movie plays fullscreen at its native size and rate, with sound, in `--opening` at the traced point, and returns control to the quest script. Fill in Results and report to `head`.

## Results

(filled in by the agent)
