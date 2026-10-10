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
