# weather-clock-bindings: Weather clock and scheduler-to-sky bindings

- **Owner:** codex-11 (Codex lane, head assignment #417; delegation #429)
- **Branch:** `task/weather-clock-bindings`
- **Board area:** `formats/weather-clock`
- **Status:** review (ordinary sync compared; special modes/final light still open)

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

Implemented `mtweather::clock` with explicit local/saved clock, scale, scenario,
battle and pause inputs. Source: 0x004c0e30/0x004c2b60/0x00437210; initialization
0x004bb200. Six regression tests cover modes, gates, HP caps, truncation,
forward/reverse wrap, and unsupported inputs. Special reverse progression and
multi-day steps are rejected; no guessed runtime default or global clock.

Resolved st200 normal scheduler chain 0x004c3140 -> st200_sky tracks 1/2/3
and 23/24 -> scr/sky/st_all_sky_F. Installed test verifies both resource links,
the primary-light class/scale inputs, and fine-weather fog/WEP samples at
06:00/12:00/23:00. Primary light comes through uSky/0x00c688a0; WEP is colour
correction. Final sun RGB and zone/event override reproduction remain open.

`cargo test -p mtweather`: 11 synthetic tests pass (six new, five existing).
`cargo test -p mtweather --test clock -- --include-ignored` with DDDA_NATIVE_PC:
seven pass including read-only installed chain/samples. Clippy with -D warnings
passes. No rendering/simulation/core code or private data added.

Additional source finding: 0x00dbe6e0 publishes mActualFps as a >1-second window
average; 0x00dbfa90 computes mDeltaTime from QPC accumulators, gates, border,
limit and global speed. Only +0x7c is 30/max(actualfps,1). 0x0041dd60 selects
normal border/limit 2/2 or flag-dependent 100/16. This explains why exact
dt=30/fps is not a per-frame invariant. Head corrected RECORDINGS independently
after board #488/#491; full source explanation posted #511.

Measured comparison: raw session005707 CSV has 2165 rows; abs(dt-30/fps)
min0.000328643/median0.072867217/max1.1798625. No weather/saved clock fields are
recorded, so clock-advance error cannot be measured from that file.

Review follow-up: passive observations returned by claude-4 #532, private
notes/oracle/readonly_batch_0120.json key codex11. Independently checked all
240 stage100 idle samples: weather/saved time equality has maximum error0ms;
hour/minute/second have zero mismatches; all239 increments exactly match the
same-frame saved scale*(delta/basefps)*30000 with f32 arithmetic and truncation,
maximum integer error0ms. Recorded local Pause=1 does not stop sync copying.
This closes ordinary synchronized-clock comparison only. Special modes/wrap
and final-primary-light RGB remain unmeasured; final-light gate remains blocked
pending suitable passive captures. No setters, input, calls, teleportation,
injection or process writes were requested. Removed unverified st200 location
name; numerical resource stage and stage100 observation are kept distinct.

Detailed evidence, resource samples and limitations: docs/WEATHER_CLOCK.md.
Next action: passively observe naturally available mode/wrap transitions and
sample uSky colour; review this branch's covered source rules
independently without treating the outstanding comparison gate as complete.
