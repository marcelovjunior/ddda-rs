# S19: Subtitles of the opening movie and events

- **Branch:** `step/s19-subtitles` (after S16, merged)
- **Files:** `crates/ddview/src/movie.rs`, `event.rs` (subtitle drawing),
  `crates/ddstory` or a small reader for the scheduler tracks if needed
- **Kind:** one feature

## Context (S16 Result)

The movie event's main scheduler (`rom/event/s100_00.arc`) has 28 tracks.
`GUISubtitlesExt` uses `id/telop/event_telop2`; `MessageResourcePath` is
`id/message/event/st200ev99_jpn`; its `Subtitles` child has `Language`,
`IndexName` (labels 0/1/0) and `Visible` tracks. These are outside the WMV
picture. Event cutscenes already show subtitles (`event.rs`), so reuse how
they pick the language and message resource.

## Do

Play the movie's subtitle timeline from its scheduler tracks during movie
playback, in the language the cutscenes use (the Japanese path is the
authored reference; find how the original picks the localized resource).
Draw them the way the event subtitles are drawn.

## Done when

A `--movie st100ev00` or `--opening` capture at a subtitled moment shows
the right line, ddview tests pass, and the Result lists the track timing
rule and the resource selection.

## Result (codex-53, 2026-10-10; review)

- Code: `bbecf0d` on `step/s19-subtitles`. Movie playback now evaluates the
  installed event subtitle timeline against its sound/video elapsed clock.
  `EventPlayer` reads message paths from the main scheduler as well as the
  separate `_msg` scheduler, and supports `uGUISubtitlesExt` and Ext2.
  Event and movie UI share the existing subtitle drawing helper; the movie
  parents its line to the movie root so both skip and end remove it.
- Selection: the installed event table and each scheduler's
  `rMovieOnDisk` resource resolve the movie to its row (S16's `event_paths`),
  rather than a hardcoded event row or message filename. Runtime
  `MessageResourcePath` is localized by `localize_path` (`0x00de13a0`,
  language getter/table `0x0114fe40`/`0x016333e0`), with the authored path
  fallback from `0x00726780`. Both current viewer cutscenes and movies use
  language 1 (English). The authored opening path
  `id/message/event/st200ev99_jpn` therefore loads `_eng` from the install;
  no dialogue or font pixels are embedded in this change.
- Timing: `rom/event/s100_00.arc`, scheduler
  `event/st100/ev00/st100ev00` has 28 tracks and 3,285 frames. Its main
  `GUISubtitlesExt.Subtitles.IndexName` keys are labels 0 at frame 0,
  1 at 120, and 0 at 555. `Visible` is false/true/false/true/false at
  0/120/300/555/819. The viewer evaluates the existing sourced SDL set-key
  and nearest-even index-frame rules (`0x00ddfd50`/`0x00ddee70`) at
  `movie_elapsed_seconds * 30`, using the existing event frame rate.
  `Language` is a bit mask, verified at `0x007263a0`, not an index:
  authored 255 admits the first eight language indices. Its constructor
  initializes the mask to zero (`0x00726490`). Installed label 1 is a
  whitespace-only line; label 0 is the epigraph translation and attribution.
- Tested: `cargo test -p ddstory --release` in the shared core target:
  **39 passed**, including four installed event tests. Final event tests
  rerun in the shared viewer target: **4 passed**, 2.52 s, including
  fractional nearest-even onset, both visible windows, hidden intervals,
  backward seeking, excluded language bit 9, English runtime GMD equality,
  German localization, and the existing ev10 tests. Full
  `cargo test -p ddview --release`: **60 passed, 2 existing ignored**,
  9.75 s. `cargo build --release -p ddview`: passed, 33.42 s; only the
  existing Args visibility and unused audio method warnings. Movie skip
  regression also verifies its subtitle descendants are removed.
- Captured and inspected: private
  `E:/ddda-re/notes/steps/S19/movie-subtitle20.png` and matching `.err.log`,
  from `ddview ... --movie st100ev00 --unattended --shot ... --shot-delay 20`.
  Hidden run with a 60-second watchdog exited 0 after 27.13 s. It visibly
  shows English label 0 below the WMV's French quote. Runtime transition
  log: label 1 at 3.99 s, hidden at 9.99 s, label 0 at 18.48 s. Shared GPU
  and build claims were released on actual completion.
- Limits: this satisfies movie timeline/text hookup and reuses the event
  UI's existing presentation choices (30 px white centered text, bottom
  8%). It does not establish the original GUI font/layout/VariableSection
  fade equivalence or compare with a running original-game subtitle frame.
  The default event font displays a missing glyph for the installed
  attribution's U+00E9; the GMD text is preserved correctly. The opening's
  existing unattended three-second skip still precedes the first subtitle
  window; standalone `--movie` is the accepted capture route here.
- Next step: use the authored `id/telop/event_telop2` GUI and installed
  bitmap font for event/movie subtitles, including accented glyphs and
  its VariableSection fades, then compare a matched original frame.
