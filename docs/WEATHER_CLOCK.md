# Weather clock and scheduler bindings

The local weather clock, its saved-clock input and the st200 SKY resource
chain are traced. `mtweather::clock` implements a bounded, pure weather-clock
step with explicit inputs. Ordinary saved-clock synchronization was compared
with 240 passive original-game samples, with zero millisecond discrepancy.
It does **not** implement the saved game clock,
atmospheric scattering, scheduler execution or rendering. Resource samples
below are tested against installed files; they are not live light measurements.
All executable addresses refer to the 2017-12-13 Steam build in METHODOLOGY.md.

## Clock ownership and units

`sWeatherManager` is `*0x018fa520`. Relevant members are:

| Offset | Meaning |
|---|---|
| +0x40 | SyncGameTime (only byte value 1 takes the synchronization branch) |
| +0x41 | Pause |
| +0x44 | mTimeScale, dimensionless |
| +0x4c | local unsigned milliseconds |
| +0x50/+0x54/+0x58 | hour/minute/second |
| +0xa50 | LastBattleMode |
| +0xa54 | mBossHpRate |

Reflection `0x004bca80` supplies names; constructor `0x004bb200` initializes
mTimeScale to 1, local milliseconds to 0 and Pause to true. These constructor
values are not assertions about the active scene: schedulers/reflection can
change them. The initialization of SyncGameTime was not closed in this trace;
the helper requires it explicitly instead of choosing a default.

Weather update `0x004c0c70` validates live references, runs `0x004c0e30`, then
updates weather and the light-zone listener. Define:

- `battle = (sGameSys.SceNo == 8800) || LastBattleMode`.
- `main_pause = (*sMain+0x68 == 0) && sMain.mPause(+0x6c)`.
- `event_pause = low_byte(0x0043f480()) == 1`.
- `base = mDeltaTime / mFps * 30000` if `mFps > FLT_EPSILON`, otherwise zero.
- `increment = mTimeScale * base`, evaluated in single precision in that order.

`sMain` is `*0x018d0b28`; mFps is +0x38 and mDeltaTime +0x70. mDeltaTime
is expressed in base-rate frames. Dividing by mFps gives elapsed seconds;
30000 converts each elapsed second to 30000 game milliseconds before scale.
This is a dimensional interpretation of the code, not evidence that every
recorded render-rate sample determines mDeltaTime exactly.

| SyncGameTime | battle | Source and gate |
|---|---|---|
| true | false | Copy `sGameSys+0xb8768` verbatim; ignore local scale and Pause |
| false | false | Local increment unless local Pause; main/event gates are not consulted here |
| false | true | Local increment unless main_pause or event_pause; local Pause ignored |
| true | true | Same battle gates, local increment multiplied by 24, then HP-dependent cap |

`0x0043f480` returns true when main_pause is true, or game flags
`+0xb883c & 0x20000000` or `+0xb8848 & 0xe0000000` are nonzero. The helper
accepts its resolved result instead of inventing names for these flag meanings.
`0x004c2b60` implements the same battle pause decision. A paused special clock
can still be clamped if its existing time exceeds the current HP cap.

| Boss HP fraction | Special SyncGameTime cap |
|---|---|
| > 0.8 | 04:30:00 (16,200,000 ms) |
| > 0.6 through 0.8 | 13:00:00 (46,800,000 ms) |
| > 0.4 through 0.6 | 21:00:00 (75,600,000 ms) |
| <= 0.4 | 23:59:59 (86,399,000 ms) |

The cap compares the truncated proposed sum to the cap; it does not clamp the
increment independently. Day wrap is applied **before** adding the truncated
increment: positive steps subtract 86,400,000 when current >= day-increment;
negative steps add one day when current < -increment. The code does only one
wrap, not arbitrary modulo. The helper rejects multi-day increments and
non-finite inputs. Reverse special-battle progression is explicitly unsupported
because its unsigned cap comparison needs additional original-game evidence.

The decompiler's `ROUND` is misleading: x87 control-word RC is set to 11 at
`0x004c0fa9` / `0x004c10b2`, selecting truncation toward zero. No fractional
remainder is retained. At delta=1, basefps=30, scale=-1, the f32 increment is
about -1000.000061 ms. A local clock at 1000 therefore wraps first, then adds
-1000, leaving exactly 86,400,000. This boundary is preserved and regression
tested, rather than silently normalized. The helper accepts this day endpoint.

`0x00437210` derives display seconds as `(ms/1000)%86400`, then hour/minute/
second. `0x00f30160` receives the local time for both mpSky and mpRefSky and
stores `ms%86400000` at uSky+0x3bc. WEP/WFP use integer `hour*60+minute`.

## The saved clock is a separate producer

`sGameSys::vf06` at `0x0043d0a0` writes +0xb879c as
`sMain.mDeltaTime / sMain.mFps`. `0x0043f2c0` computes the saved-clock
increment using that value times 30000 and its own scale at +0xb8778,
then truncates, stores the step at +0xb877c, updates +0xb8768, and advances
the 64-bit day count when wrapping. A nonzero scale with an exactly zero
computed step has a special minimum-step fallback of 1 before truncation.

Its pause predicate `0x0043f3d0` includes main/event flags, SceNo <=1099,
SceNo 8800, LastBattleMode, bits 0xc00000 of game flags +0xb8844, a nonnull
game +0x54 object, and stages 800 through 804. The saved-scale initialization,
flag meanings and all state transitions remain outside the translated helper.
In ordinary synchronized play it is this producer, not mTimeScale, that
determines the weather clock's rate.

## A source-backed st200 chain

`0x004c3140` finds the stage in the 65-short table at 0x01530348 and reads
the sky remap at `0x015303d6 + 22*index`. st100 is index 0 and maps to 100;
st200 is index 1 and maps to 200. On the normal, non-special scheduler path,
st200 loads `scr\st200\sdl\st200_sky` into manager+0x530, creates a
uScheduler at +0x544, binds it with `0x00dded70` and starts it. The alternate
`_sky_T` and stage-370/380 paths are not selected by this example.

SDL v19 is described by quest owner claude-7's `ddquest::sdl`; loader
`0x00db8e50`, evaluator `0x00ddee70`. The installed st200 sky scheduler
has 243 tracks and a nominal 5000-frame length. Its relevant binding path:

1. Track 1 binds class sWeatherManager, JAMCRC 0x015f9012, named WeatherManager.
2. Track 2 follows mpSky; track 3 sets mpResource to rSky
   `scr\sky\st_all_sky_F` (hash 0x5ea7a3e9), at key zero.
3. Track 23 follows mpRefSky; track 24 sets the same SKY resource.
4. Track 80 creates uSkyInfiniteLight (hash 0x0662a73e), named Light-DMain-SPE,
   on move line 25. Track 241 creates uSkyColorFog (0x16b23211), on line 4;
   track 242 sets mDistanceType to 1.

The primary light's scheduler inputs are constant keys at zero: mLightType=3,
mLightScale=0.1; Fine/Cloudy/VeryCloudy intensity scales 3/3.6/1.26 and
saturation scales 1/1/1. These are **inputs**, not the final sun RGB. The
same st200 sky configuration has mpSky.mBaseSunColor=(64,64,64), north axis 1
and height offset -1300 (st100 uses 30000). uSky's physical/atmospheric
calculation still supplies time-dependent colour and direction.

`0x00c688a0` obtains sky colour with `0x00f32fd0`, applies the scheduler's
light scale and weather intensity/saturation settings, optionally replaces
them with zone values, and writes the actual uLight RGB at +0x50. The exact
morning/noon/night primary-light RGB has **not** been independently reproduced.
WEP correction list 0 must not be substituted for it.

Separately, `0x004c2800` loads `scr\st200\sdl\st200_light`, which has
86400 nominal frames and includes night point lights and point shadows.
The sky scheduler's 5000 frames must not be mistaken for a day length.
General scheduler playhead-to-clock binding is not translated here.

Fine-weather data comes from `effect\wep\w_st200_f` and
`scr\st200\etc\st200_fog_f` through `0x004c19f0`. WFP samples follow
`0x00cead20` / `0x004c4da0`; the fog sink `0x00c65cd0` copies manager values
to uFog unless a zone overrides them. Exponent density is a separate member.
The regression test resolves the two SKY links, parses their resource and
samples these fine-weather files:

| Time | Fog RGB | WEP list 0 RGBA | WEP colour blend |
|---|---|---|---:|
| 06:00 | (0.122360006,0.18728134,0.24553332) | (254,129,17,255) | 0.30 |
| 12:00 | (0.188254,0.347661,0.607746) | (255,83,0,255) | 0.12 |
| 23:00 | (0.005,0.005,0.005) | (0,212,255,255) | 0.20 |

All three fog samples have density 0.9, start 3000, end 130000. These values
are resource samples before zone/event overrides, not captured rendered output.

## Recorded comparison and remaining gate

The source explains why mDeltaTime need not equal `30/mActualFps`:

- `0x00dbe6e0` counts updates and publishes mActualFps (+0x44) as count/elapsed
  only when its elapsed interval exceeds one second (or a forced refresh flag
  at +0x4d). It is a window average, not the inverse of this frame's duration.
- `sMain::vf06`, `0x00dbfa90`, reads QueryPerformanceCounter via `0x0130a5a0`
  and frequency via `0x0130a5c0`. It accumulates `(new_counter-old_counter)*mFps`
  in the 64-bit +0x50 accumulator, and obtains the candidate delta from the
  difference against the consumed accumulator +0x58 divided by frequency.
  mTimer (+0x20) separately holds elapsed integer milliseconds; mFrameTimer
  (+0x30) is integer-base-FPS times that timer divided by 1000.
- With mFrameWait (+0x40) false it waits toward `mFps/mMaxFps`, clamps a
  negative candidate to zero, and handles a value strictly above
  mDeltaTimeBorder by adding `candidate-1` to mDelayFrame and resetting delta
  to 1. It then clamps to mDeltaTimeLimite, advances the consumed accumulator
  and multiplies the final delta by mGlobalSpeed. With mFrameWait true it
  advances the consumed accumulator by one frequency unit and uses delta=1
  before global-speed multiplication.
- `sBbsRpgMain::vf06`, `0x0041dd60`, sets border/limit to 2/2 normally, or
  100/16 when either game flag +0xb8844 & 0x06000000 is present. These are
  distinct border and limit operations, not a single universal 2-frame cap.
- Only the separate +0x7c member is explicitly `30/max(mActualFps,1)` at the
  end of `0x00dbfa90`. The weather clock reads +0x70 instead.

These timer rules are documented, not translated into an OS timer helper.
The exact accumulator integer-conversion/overflow edge cases remain untested.
They establish that a window-average FPS cannot reproduce the per-frame delta.

RECORDINGS.md reports mFps=30 and variable render rate for session 005707.
However, a direct read of private `005707_sprint_open.csv` finds 2165 rows
with FPS 46.1881..73.14286. Comparing its `dt` column with `30/fps` gives
absolute error min 0.000328643, median 0.072867217, max 1.1798625; no row is
within 1e-5. The median product dt*fps is 28.3370068, with range
-31.0758315..90.40896. Thus the raw file does **not** support the stronger
statement that this relation holds in every sample. Head acknowledged the
discrepancy on board #491 and corrected RECORDINGS.md independently.

The capture source reads +0x70 and +0x44 sequentially from a configured
sMain address (default 0x080ebc70). The averaged FPS explains why exact equality
is not expected. Cross-frame timing, sampling phase or an address issue could
also contribute, but none is established; in particular the negative recorded
delta is not explained by the final nonnegative normal-path delta. The
CSV does not record weather milliseconds, saved milliseconds, scale, clock
mode or base FPS. It therefore cannot yield a measured error for clock
advancement. Source-derived 312 ms/frame at delta=0.3125, basefps=30,
local scale=1 is a regression test, not a measured weather-clock result.

Passive clock/weather observations requested on board #439/#469 were returned
by claude-4 on #532: private `notes/oracle/readonly_batch_0120.json`, key
`codex11`, 240 consecutive frames on 2026-10-09 at 01:16:10. The window was
focused, the player idle in stage 100, and the probe was already loaded. The
request and capture involved read-only observations, no input, setters, calls,
teleportation, injection or process writes. The resource example above is
st200; these observations must not be presented as its lighting comparison.

Independent replay of all recorded fields confirms:

- All 240 weather times equal the saved game milliseconds exactly; maximum
  difference **0 ms**. All recorded weather hour/minute/second fields match
  `hms`, with zero mismatches. SyncGameTime=1, local Pause=1, local scale=1,
  LastBattleMode=0 throughout: this directly verifies that ordinary saved-clock
  copying is not stopped by local Pause.
- Base FPS=30 and saved-clock scale=1 throughout. Each recorded +0xb879c
  equals the f32 result of same-frame mDeltaTime/mFps exactly. For all 239
  consecutive increments, the saved-clock update equals
  `trunc_f32(scale * f32(delta_seconds * 30000))`: **0 mismatches, maximum
  integer error 0 ms**. Before truncation, actual-minus-predicted ranges from
  -0.99908447265625 to -0.00408935546875 ms, as expected for positive truncation.
  Total advancement is 105703 game milliseconds over the 239 intervals.
- mDeltaTime ranges 0.3306719959..0.7169221044. The four reported averaged FPS
  values are 77.78461456, 65.61553192, 65.96923065 and 66.60971069; they are
  not per-frame inverses of delta. Weather current/previous ids are 1/0 and
  transition weight is 1 throughout.

This closes the **ordinary synchronized-clock** measured comparison. It does
not compare unsynchronized local progression, pause transitions, battle modes,
day wrap, or final primary-light RGB. Those remain source-tested or unresolved
as specified above. No morning/noon/night original light capture was supplied.

Reproduce implemented checks:

```powershell
cargo test -p mtweather
$env:DDDA_NATIVE_PC = 'E:/SteamLibrary/steamapps/common/DDDA/nativePC'
cargo test -p mtweather installed_st200_chain_and_samples -- --ignored --nocapture
```

Six new synthetic tests cover local/saved/battle modes, distinct pause gates,
float truncation, forward/reverse day boundaries, HP threshold equality and
explicit unsupported inputs. The existing five reader tests also pass.
Next action: passively observe naturally available clock mode/wrap transitions
and independently sample uSky colour before any renderer integration.
