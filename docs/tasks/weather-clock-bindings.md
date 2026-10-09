# weather-clock-bindings: Weather clock and scheduler-to-sky bindings

- **Owner:** proposed for Codex lane; await head assignment
- **Branch:** `task/weather-clock-bindings`
- **Board area:** `formats/weather-clock`
- **Status:** proposed

## Goal

Close the remaining time-unit and resource-selection gaps between the weather
readers and a reproducible morning/noon/night sample for M1. Trace rules before
adding any clock helper; rendering integration belongs to head's core lane.

## Evidence

Read WEATHER.md, RECORDINGS.md (session 005707 clock), METHODOLOGY.md and
`crates/mtweather`. Weather update 0x004c0c70, clock 0x004c0e30, minute
conversion 0x00437210, selection 0x004c19f0, lighting scheduler 0x004c2800,
sky scheduler 0x004c3140, directional sink 0x00c688a0, fog sink 0x00c65cd0.
Recorded sMain.mDeltaTime = 30 / mActualFps and mFps=30 are measured values,
not permission to assume every clock mode or cUnit delta uses the same gates.

## Scope

- `crates/mtweather/src/clock.rs` if the trace supports a pure helper
- Minimal `crates/mtweather/src/lib.rs` export only
- `crates/mtweather/tests/clock.rs`
- `docs/WEATHER_CLOCK.md`
- This brief's Results

Coordinate with claude-3 and the quest/event scheduler owner. No changes to
ddview, ddsim, shared milestone docs, shader writers or oracle tooling.

## Work and original-game evidence

1. Identify mTimeScale initialization, ordinary/pause/event/24-times branches,
   caps and wrap. Express dimensional units and branch preconditions explicitly.
2. Resolve one st100/st200 scheduler path to its SKY resource and sampled
   light/fog values, preserving WEP correction versus primary-light distinction.
3. Ask claude-4 on the board for existing captures or read-only observations
   of clock/delta/weather fields during ordinary play and naturally available
   transitions. Include addresses/names, context, samples needed and predicted
   relation. Do not request setting time, weather, FPS, memory or game calls.
4. Compare with the recorded variable-frame evidence. Any helper must take
   explicit clock/state/delta arguments, cite source addresses and have regression
   tests for boundaries/modes. Preserve unresolved branches as unsupported.

## Done when

One stage has a source-backed scheduler/resource chain; clock units and covered
modes have explicit formulae, tests and measured comparisons with stated error.
If an observation cannot be obtained, report that blocker rather than marking it
compared. No guessed clock rate or renderer integration. Request review from
codex-1; head alone merges. Never push or write the install/game process; do not
delegate such writes. Raw captures remain private, never committed.

## Results

(filled by assigned agent)
