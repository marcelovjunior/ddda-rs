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

Implemented event stream and ordinary SE playback from installed resources.
Streams use STRQ request/prepare IDs and loose Vorbis files. Ordinary terminal
SREQ play commands resolve their referenced SPAC program/variation and decode
ADPCM/PCM. Effects overlap independently; streams replace the previous source
for their unit, including multiple cues queued in one deferred batch.

Conditional voices use the sourced uSoundSe gate (0x00c78570). Both standalone
events and opening cutscenes read the Arisen and first pawn's voice context
from --save using the conversion at 0x00780fa0. A no-save opening supplies no
conditional context. The separate edit-condition group remains unknown.
Standalone events use the saved actor's sex-specific scheduler unless --sex
was explicitly supplied. Offline pause pauses the sources.

Verification: 32 viewer tests pass, one installed HUD test ignored; five of
these cover audio routing, voice context and deferred playback. Eighteen
reader unit tests have passed across the targeted runs, along with installed
ev10 stream/ADPCM decoding and all 92 motion maps/one surface map. A previous
bounded event-22 run resolved both streams and exited without audio-device
errors. A bounded saved-voice run from frame 510 reaches both authored voice
alternatives at frame 514 but resolves only the selected f1 package (40,320
samples), saves a screenshot and exits without decoder/device errors.

Not complete: motion-triggered footsteps/attacks, field BGM, original mixer
priority/groups, runtime program overrides, gain/pitch/delay/spatial sends,
random requests and multi-request chains. Audible playback and original-game
comparison have not been verified. Motion readers, weapon bank naming,
direct/attribute routing and Terrain::ray_with_material are ready for the
next actor-motion hookup. Details and evidence are in docs/SOUND.md.
