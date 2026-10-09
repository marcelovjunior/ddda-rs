# sound-resources: Sound banks, requests and streams

- **Owner:** codex lane
- **Branch:** `task/sound-resources`
- **Board area:** `formats/sound`
- **Status:** open

## Goal

Readers for SPAC, SREQ and STRQ, resolving the loose stream paths. No playback.

## Evidence

Start from `docs/SURVEY.md`, section "1. Sound and music": the resource classes,
counts, sample structures, exe loader addresses and reference coverage
are there. The tools are listed in `docs/tasks/README.md`.

## Scope

- `crates/mtsound/**`
- `docs/SOUND.md`
- `crates/ddtool/src/<name>.rs`, if you add a CLI module (shared hookups as in `docs/CODEX.md`)
- this brief's Results

## Done when

All instances parse with bounded offsets, and sample RIFF/Vorbis streams validate. Links from request to source are reported, and unresolved metadata is listed. Distinguish traced from guessed, and fill in Results. Then report
to `head`.

## Results

(filled in by the agent)
