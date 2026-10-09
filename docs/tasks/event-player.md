# event-player: Play a cutscene and return control

- **Owner:** claude-7
- **Branch:** `task/event-player`
- **Board area:** `story/events`
- **Status:** open

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

(filled in by the agent)
