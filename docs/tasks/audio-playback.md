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

Buildable checkpoint, not complete. Event stream cues are forwarded in the
standalone player and opening cutscenes, decoded into a cached Bevy source,
and paused with offline play. The six-channel stereo fold is provisional.
SREQ program/variation lookup and installed ev10 SPAC decoding are tested.
Release ddview builds; all 11 mtsound tests pass, including the installed
resource corpus. Two viewer audio regressions pass. A bounded event-22 run
resolves both streams, saves a screenshot and exits without reported audio-device
errors. Source addresses and remaining mapping work are in SOUND.md.

Audible playback and comparison against the original have not been verified.
Conditional Arisen voice, motion footsteps/attacks and field BGM are still
unconnected. Motion-event remap and request variants now have a typed reader
and regression, and all 92 installed motion sound maps parse. Next: bind the
request banks and surface lookup to actor motion events.

The material ray API and explicit direct/attribute bank routing are ready.
Five motion unit tests pass; material columns and bank selectors remain
separate, including the original's low-29-bit material scan. No footsteps
or weapon sounds are yet claimed audible.

Unconditional event SE cues now resolve ordinary terminal SREQ play commands
to their referenced SPAC wave and play as overlapping one-shots. Stop/fade,
random requests, multi-request chains and conditional event variants are
explicitly unsupported. Fifteen reader tests and the installed ev10 decode
pass. Original mixer behavior and audible comparison remain open.
