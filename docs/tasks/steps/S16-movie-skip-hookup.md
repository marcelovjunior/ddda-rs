# S16: Movie skip input and the hookup after the prologue

- **Branch:** `step/s16-movie` (after S08)
- **Files:** `crates/ddview/src/movie.rs`, `intro.rs`, `opening.rs`
- **Kind:** one feature

## Context

docs/tasks/opening-movie.md: movies decode and play (`--movie st100ev00`);
the intro plays it before the editor. Open: the original's skip input (S08
finds it), subtitles outside the picture, and the hookup to q0000's event
st100 ev00 in `--opening` (after the prologue, stage 800).

## Do

Use S08's result for the skip input (keyboard and pad), and play the
movie at the point of the opening sequence where the original plays it,
driven by the event/quest data rather than a fixed order. If subtitles are
separate data, note where they are and leave them for a later step.

## Done when

An `--opening` run plays the movie at the original's point, the skip input
works as traced, ddview tests pass, and the Result lists the trigger.

## Result

codex-46, 2026-10-09: ready for lane review. Implementation checkpoint
`74ead18`, verified completion/cleanup checkpoint `0eeff08`; current main
was merged into this step with `59f5081`.

- `movie::event_paths` reads the installed event table and each event's
  main scheduler, resolving `rMovieOnDisk` resources into install paths.
  The existing scripted prologue route now stops on the quest's awaited
  `PlayEvent` request for that discovered movie. For this install it is
  event **st100 ev00**, requested by **q0000 at stage 800, SceNo 1000**.
  The filename and presentation order are not used to generate this request.
- Playback completion or skip acknowledges that awaited event. Story
  stepping then observes the authored **stage 801** editor transition;
  editor confirmation resumes the existing dragon stand-in route to
  **stage 200, SceNo 1100, q0001**. Neither q0001 nor editor admission
  happens while the movie remains awaited. Two local seats survive these
  transitions. Duplicate completions and editor confirmation at the
  wrong stage are rejected.
- Skip uses **Escape press edge** or **any connected pad's Start press
  edge**, from S08's `0x004367c0` / `0x0040d200` tracing. Enter does not
  skip; first playback is accepted. Polling begins after playback
  preparation. Bevy connected devices replace the original four slots;
  the original input-suppression plumbing and event fade timings remain
  unimplemented. The three-second unattended completion is explicitly
  automation, separate from the input binding.
- Related scope edits: `main.rs` passes whether preparation should await
  the intro; `pause.rs` leaves input to the active movie/editor so Escape
  does not open the gameplay pause UI there. Decode failure explicitly
  blocks the event rather than acknowledging an unplayed movie.

Validation:

- `cargo test --release -p ddview --target-dir E:/ddda-rs-wt/m5-cassardis/target`:
  **55 passed, 0 failed, 2 ignored**, 8.22 s. This includes the installed
  movie/editor/q0001 two-seat regression, press-edge regression, and real
  playback-system input/cleanup regression for keyboard and a second pad.
- `cargo build --release -p ddview --target-dir E:/ddda-rs-wt/m5-cassardis/target`:
  **passed**, 41.95 s; existing `private_interfaces` and unused
  `forget_actor` warnings remain. No core crate implementation changed.
- Hidden `--opening --players 2 --unattended --shot-delay 14` run:
  **exit 0**, 31.74 s including preparation. Logs show movie metadata,
  automation skip at 2.3 movie seconds, movie EventFinished to stage 801,
  then editor confirmation and Cassardis. `opening-world.png` visibly
  reports stage 200, SceNo 1100 and q0001. This used the first tested
  artifact; final guards were subsequently covered by the full suite.
- Final artifact `--opening --players 2 --unattended --shot-delay 1`:
  **exit 0**, 14.83 s. `opening-movie.png` shows the initial black picture;
  the log confirms st100ev00 playback admission. Private captures and logs
  reside in `E:/ddda-re/notes/steps/S16/`, with 60 s watchdogs.
- Final artifact `--movie st100ev00 --unattended --shot-delay 20`:
  **exit 0**, 26.10 s. `movie-frame20.png` visibly contains the movie's
  epigraph, confirming decoded video presentation beyond its initial
  black frames; this standalone run does not exercise the intro's
  automation skip.

Separate subtitles exist. Inspection of the installed
`rom/event/s100_00.arc` main scheduler found 28 tracks: `GUISubtitlesExt`
uses `id/telop/event_telop2`, `MessageResourcePath` is
`id/message/event/st200ev99_jpn`, and its `Subtitles` child has
`Language`, `IndexName` (labels 0/1/0) and `Visible` tracks. These are
outside the WMV picture. Subtitle playback and localized path selection
are left for the next step; the Japanese resource is the authored
reference, not a hardcoded viewer language choice.

Acceptance limits: the prologue and dragon route are still the existing
scripted stand-ins. This verifies the movie's admission/completion and
the current editor handoff, not a playable prologue, original fade
fidelity, audible soundtrack comparison, physical-pad interaction, or a
live original first-playback comparison. Suggested next step: connect
the separate subtitle timeline and localized message resource while
retaining this event lifetime.
