# audio-playback: Sound and music playback

- **Owner:** codex lane
- **Branch:** `task/audio-playback`
- **Board area:** `audio/playback`
- **Status:** open

## Goal

Play the game's sound with the `mtsound` readers (`docs/SOUND.md`): footsteps and attack SFX from the motion events or requests, voice cues in events (EventPlayer's requests), and field BGM by area. Vorbis streams decode via a crate; bank waves too, if feasible.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs must not grab the mouse or pause on focus loss.

## Scope

- `crates/ddview/src/audio.rs`
- `crates/mtsound/**`
- `docs/SOUND.md`
- this brief's Results

Shared viewer files (`play.rs`, `main.rs`) need a board claim and
coordination with the agent that holds them.

## Done when

Attacks, footsteps and the st200 ev10 voice line play in `ddview`. Describe what's audible, and explain the mapping from request to sound with addresses. Fill in Results and report to `head`.

## Results

(filled in by the agent)
