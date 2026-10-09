# event-player: Play a cutscene and return control

- **Owner:** claude-7
- **Branch:** `task/event-player`
- **Board area:** `story/events`
- **Status:** review

## Goal

Proposal 2 in `docs/QUESTS.md`: play event `st200 ev10` from the opening, driven by `ddstory`'s "play event" request. Use the SDL timeline (`ddquest::sdl`) and the camera lists (`mtcamera`, `docs/CAMERA.md`). Cover camera cuts and interpolation, actor placement and motions, subtitles by message label in the player's language, and the voice cue as a request, since audio playback does not exist yet. At the end, raise SceNo and return control through `ddstory`. Split it into a headless event clock in `ddstory`/`ddquest` (yours) and a thin `ddview` presentation in a new file `crates/ddview/src/event.rs`. claude-3 holds `ddview/main.rs`, so ask on the board for the one-line hookup.

**Shared world state.** `crates/ddstory` (merged) owns the world state and
its save/load. **claude-7 owns `ddstory`**. Build your system in your own
crate on top of it; if you need a ddstory API change, ask claude-7 on the
board instead of editing it. Design for N players, host-owned
(`docs/COOP_DESIGN.md`).

## Scope

- `crates/ddstory/**`
- `crates/ddquest/**`
- `crates/ddview/src/event.rs`
- `docs/EVENTS.md`
- this brief's Results

## Done when

A headless test steps `st200 ev10` frame by frame and asserts the cut frames (0, 460, 670, 790, 910, 1030, 1150, 1270), the subtitle labels at their frames and the end state. `ddview` can play it from the command line, and screenshots at cut frames are described. If possible, check SceNo 150/600 and the six differing flags live through claude-4. Fill in Results and report to `head`.

## Results

**Headless event clock.**
- `ddquest::sdl` translates `uScheduler`:
  - `Clock::step` 0x00de02a0;
  - the playhead 0x00ddfd50 and cut snap 0x00ddeb00;
  - key evaluation 0x00ddee70: set, count, trigger, linear and
    Catmull-Rom, with the first key applying before it.
- `ddstory::event::EventPlayer` loads a row's timelines as 0x004362f0
  does: main, light by stage and hour, and `_msg_` / `_sound_` by sex. It
  steps them and reports per frame:
  - the camera cut, local frame and base `mtcamera` pose;
  - the actors (motion number and frame, Draw);
  - the fade colour;
  - the subtitle label and text, in the player's language through the
    path rewrite 0x00de13a0;
  - sound cues as requests (streams and voice).
- It ends at frame count - 1 (0x00435fb0). The host then sends
  `EventFinished`, and `ddstory` raises SceNo and places the party.
- The changes to `ddstory`:
  - a default `Data::resource` method;
  - `ScriptedHost::play_rows` and `awaiting`;
  - `Trace::events`.

**Tests** (`cargo test --release -p ddstory -p ddquest`): all pass.
- `ddquest`: 13 unit tests, including the key modes and the clock with
  cut snapping, and 6 install tests.
- `ddstory`: 8 unit tests, plus the install tests `event` (2) and
  `opening` (1).
- `st200_ev10_headless` asserts, frame by frame (1,702 frames):
  - the cut frames 0, 460, 670, 790, 910, 1030, 1150 and 1270, with
    motion numbers following the cuts;
  - the 12 subtitle labels at their frames, the male/female variant
    (label 2 or 3 at 326), and English vs German text;
  - the fade, the voice cue at 514 and the end at 1701.
- `opening_plays_st200_ev10_and_returns_control` runs the whole opening
  with row 22 played for real. It checks the same SceNo ladder, 1100 ->
  1200 after the event's last frame, and `PlaceParty` start position 12.

**ddview.**
- `crates/ddview/src/event.rs` handles `ddview <nativePC> --event 22
  [--start-frame F --hold --shot out.png]`.
- It shows:
  - the stage 200 models and scenery;
  - stand-in actors placed by their joint-255 root tracks;
  - the event camera, fade and subtitles.
- The `main.rs` hookup is three lines, requested from claude-3 (#960).
  It was tested in this worktree, where the release build was deleted
  afterwards.
- Screenshots of the cut frames are described in EVENTS.md, "Playback".

**Not done / open.**
- The camera's actor-joint adjustments and `mOffsetPos` are reported but
  not applied.
- Actors use the base body: no NPC looks, IK or lights.
- The sound start rule is *guessed*.
- Live checks:
  - claude-4 (#861) cannot observe SceNo 150/600 or the six GSF without
    a new game or a save between SceNo 1400 and 2000.
  - Both are recorded as needing a maintainer decision (a save copy
    restored after a new-game run).

**Commits (`task/event-player`):** c6c4a6b (clock + player + tests),
65f75a8 (ddview event.rs), and the docs commit.
