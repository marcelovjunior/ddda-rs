# sound-resources: Sound banks, requests and streams

- **Owner:** codex lane
- **Branch:** `task/sound-resources`
- **Board area:** `formats/sound`
- **Status:** review

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

Implemented `mtsound`: bounded SPAC12/SREQ19/STRQ27 readers, explicit
request/package and request/source links, split RIFF reconstruction,
safe relative stream paths, and Ogg/Vorbis container validation. No playback.

Verified 25,025 archive occurrences: SPAC 1,490 paths/11,866 copies/70,303
wave slots; SREQ 1,601/9,308/320,547 requests; STRQ 2,650/3,851/141,419.
All 1,490 package references resolve. Every nonempty wave's reconstructed
RIFF chunks validate. Bridge.sngw validates 108 page CRCs and identifies
six channels at 48 kHz. Five synthetic tests plus one installation test pass.

Exe evidence: loaders e21af0/f73f10/dc0ef0, package relocation e21600,
request relocation f735b0, stream-prefix dbffe0, optional loop inspection
e21490. All addresses are for DDDA.exe 2017-12-13 Steam. See SOUND.md.

76,468 distinct stream source paths are reported; only 285 have loose
SNGW files. All 76,183 unresolved paths use request+0x0c mode1 (which skips
immediate resource loading), but mode1 also references existing loose
files. Three installed rom/sound/*.pck banks are leads; their name/hash
lookup and language selection remain unresolved. No assertion of missing
game audio or successful playback is made. Mixing/cue metadata remains raw.

Next: trace the mode1 deferred source route and PCK name/hash lookup, then
validate one cue with authorized original-game playback.
