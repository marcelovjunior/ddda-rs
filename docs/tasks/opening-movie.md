# opening-movie: Movie playback (st100ev00.wmv)

- **Owner:** head
- **Branch:** `task/opening-movie`
- **Board area:** `story/movie`
- **Status:** partial

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

head, 2026-10-09:
- **`mtmovie`** decodes the install's WMVs with Media Foundation's
  Source Reader at run time: video as top-down BGRA (`MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING`
  to RGB32, stride-aware), audio as `f32` PCM. `st100ev00.wmv` is
  1280x720, 29.97 fps, 109.6 s, 6-channel 48 kHz; installed test
  `the_opening_movie_decodes_video_and_audio`.
- **`ddview` `movie.rs`** plays it full screen:
  - video is decoded ahead on a worker;
  - the soundtrack is decoded whole and folded to stereo from MF's
    FL FR FC LFE BL BR order;
  - the clock starts with the sound;
  - `MovieFinished` is set at the end.
- **Trying it:** `ddview <nativePC> --movie st100ev00` (also `st230ev00`,
  `st240ev05`, `st610ev21`, `title_A`, `title_B`). Checked: the frame at
  20 s shows the epigraph.

Open:
- the original's skip input (Enter and Esc are a viewer choice);
- subtitles, if the movie has any outside its picture;
- the hookup to q0000's event st100 ev00 in `--opening` (after the
  prologue, stage 800).
