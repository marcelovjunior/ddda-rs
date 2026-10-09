# Weather, fog and sky (DDDA PC)

Implemented and tested: `mtweather` reads WEP v1, WFP v1 and the complete
SKY v6 record, and samples WEP/WFP time keys. Evidence below is the
2017-12-13 Steam executable named in METHODOLOGY.md and the maintainer's
read-only `nativePC` corpus. **Not compared against running-game values or
screenshots.** No viewer or simulation integration is included.

## API and validation

```rust,ignore
let effects = mtweather::WeatherEffect::parse(&wep_bytes)?;
let fog = mtweather::WeatherFog::parse(&wfp_bytes)?;
let sky = mtweather::Sky::parse(&sky_bytes)?;
let effect = effects.sample(0, 12 * 60); // list index, integer clock minute
let fog_at_noon = fog.sample(12 * 60);
```

Readers reject wrong magic/version, truncated records, oversized counts,
trailing bytes, non-finite floats, invalid hour/minute keys and unterminated
or overlong texture paths. Keys allow 00:00 through 24:00 (only minute zero
at hour 24). Empty lists are valid and sample to `None`; defaults belong to
the consumer. The sampler normalizes supplied minutes modulo 1440.

Original file order is deliberately retained: two shipped WEP lists have
backwards adjacent keys (`effect\wep\w_st330_f`, zero-based list 1;
`effect\wep\w_st610_f`, list 3). Sorting them would change the executable's
first-strictly-later-key search. These are reported by the corpus test,
not silently repaired. No adjacent duplicate keys occur in this corpus.

Reproduce without copying game resources into the repository:

```powershell
cargo test -p mtweather
$env:DDDA_NATIVE_PC = 'E:/SteamLibrary/steamapps/common/DDDA/nativePC'
cargo test -p mtweather installed_corpus -- --ignored --nocapture
```

2026-10-09: five synthetic tests passed; the opt-in corpus test passed:

| Type | Distinct paths | ARC instances checked | Rows |
|---|---:|---:|---:|
| rWeatherEffectParam | 64 | 64 | 1,961 |
| rWeatherFogParam | 55 | 55 | 712 |
| rSky | 2 | 30 | n/a |

The corpus test reads every location, including duplicated SKY resources,
and samples each effect list and fog resource at all 1,440 minutes. Synthetic
tests cover fixed-point colour quantization (including decreasing channels),
midnight WEP wrap, fog endpoint clamping, empty lists, all truncated prefixes,
oversized counts, paths, malformed times, non-finite floats and backwards order.

## File layouts

All numeric words are little endian. Layouts were checked against loaders
`0x00cea2b0` (WEP), `0x00ceaa80` (WFP), `0x00e2c350` (SKY), and the
local Riftstone weather format reference. No reference implementation was copied.

WEP: `wep\0`, u32 version 1, six u32 counts, then six consecutive row
lists. Each row is 44 bytes: hour, minute, RGBA bytes, colour blend,
intensity, intensity blend, environment-map power scale, shadow RGBA bytes,
shadow colour blend, shadow intensity, shadow intensity blend. All blend,
intensity and scale fields are f32. Runtime rows are 48 bytes because the
first word is a vtable pointer. Field names agree with the reference and
the runtime colour consumer described below.

WFP: `wfp\0`, u32 version 1, u32 count, 36-byte rows. Each row is hour,
minute, density, exponent density, start, end, RGB float triple. Loader
runtime offsets differ: hour +4, minute +8, start +0xc, end +0x10,
density +0x14, exponent density +0x18, RGB +0x20. The reader preserves both
density fields, even though the traced sampler omits exponent density.

SKY: `SKY `, u32 version 6, followed by every loader field in order:

| Runtime offset in rSky | Serialized type/order |
|---|---|
| +0x60, +0x90, +0x80, +0x70, +0x94 | float4, f32, float4, float4, f32 |
| +0x98, +0x9c | two u32 |
| +0xa0..+0xbc, +0xc4..+0xcc | eight then three f32 |
| +0xc0 | NUL-terminated sun texture path, loaded as rTexture |
| +0xd0 | 16 floats |
| +0x110..+0x120, +0x140 | five f32, float4 |
| +0x130, +0x124..+0x12c, +0x150 | f32, three f32, f32 |
| +0x134 | NUL-terminated moon texture path, loaded as rTexture |

`Sky` uses explicit offset-based field names where physical meanings have
not been independently established in this trace. Those are retained typed
values, not invented interpretations. Texture strings retain raw bytes;
they are not lossy Unicode conversions. The executable string routine
`0x00cfee30` consumes through NUL but truncates the destination to 258 bytes;
the reader rejects longer paths instead of hiding that truncation.

## File selection and weather states

`sWeatherManager` is at global pointer `0x018fa520`. Its current weather id
`mWeatherType` is +0x268, `mWeatherTypeOld` +0x26c, transition weight
+0x270. `0x004c19f0`
initializes five weather slots, copies WEP rows into runtime lists, and
loads fog resources. Its normal path selection is:

| Weather slot | WEP stage suffix | WEP fallback | WFP stage suffix |
|---:|---|---|---|
| 0 | `_f` | `w_cm_f` | `_fog_f` |
| 1 | `_c` | `w_cm_c` | `_fog_e` |
| 2 | `_r` | `w_cm_r` | `_fog_r` |
| 3 | `_d` | `w_cm_d` | `_fog_t` |
| 4 | `_r` | `w_cm_r` | `_fog_g` |

WEP prefix is `effect\wep\w_st%03d`; fallback lives in `effect\wep`.
WFP prefix is `scr\st%03d\etc\st%03d`. Unlike WEP, this fog loader has
no common-file fallback in the inspected path. For stages in the executable
table at `0x01566730` (250, 320, 500, 501, 502, 600, 602, 603, 604,
605, 610, 611, 700, 701, 702, 705, 706; -1 sentinel), WEP slots other
than 3 use `_f` / `w_cm_f`; slot 3 retains `_d`. Stage 380 also has a
flag-dependent slot-0 override in `0x004c19f0`; its precise replacement
path is unresolved. Reflected properties in `sWeatherManager::vf03`
(`0x004bca80`) call slots 0/1/2/3 Fine/Cloudy/VeryCloudy/Darkness;
slot 4 is the extra-weather path.

`0x004c2800` loads the lighting scheduler
`scr\st%03d\sdl\st%03d_light` (or numbered `light%02d`). `0x004c3140`
loads the stage sky scheduler `st%03d_sky`, `_sky_T`, with special
`st370_sky_G` and `st380_sky_p` paths. The stage-to-scheduler remap table
and scheduler bindings of `scr\sky\st_all_sky_f` and the second SKY
resource remain to be decoded. A SKY file alone is therefore insufficient
to reproduce a stage's time-varying light.

## Clock and interpolation

Update entry `0x004c0c70` calls clock update `0x004c0e30`, then weather
update `0x004c1110`, followed by zone-light processing. The earlier survey's
`0x004c0bf0` is initialization, not the per-frame clock update.

Clock +0x4c is milliseconds within an 86,400,000-ms day. `0x00437210`
computes seconds `(milliseconds / 1000) % 86400`, then hour, minute and
second at manager +0x50/+0x54/+0x58. WEP and WFP use **integer minutes**
`hour * 60 + minute`; fractional seconds are not used by these samplers.
The clock is also passed to both sky units by `0x00f30160`.

Normal advancement in `0x004c0e30` uses `mTimeScale` (+0x44) times
`(sMain.mDeltaTime [+0x70] / sMain.mFps [+0x38]) * 30000`, guarded against a near-zero
denominator, and handles day wrap. There are pause/event gates, a saved-clock
branch and a 24-times special branch with time caps. Default scale,
delta normalization and all mode transitions are **not resolved enough to implement
a faithful real-seconds clock rate**. No rate is guessed or exported by mtweather.

For WEP, `0x004c19f0` appends a copy of the first row at 24:00 to each
nonempty runtime list. `0x004c4330` scans rows in file order to the first
key strictly greater than current minute. Before the first row it clamps;
otherwise it interpolates from the preceding row to that upper row using
`t = (minute - lower_minute) / (upper_minute - lower_minute)`. Scalars use
`a*(1-t) + b*t`. Each byte of both colours uses
`a + (((b-a) * trunc(t*256)) >> 8)`, not floating-point RGB interpolation.
The sampler mirrors the appended midnight row without mutating the parsed file.

WFP `0x00cead20` uses the same strict-upper search but clamps beyond the
last stored row; **it does not append a midnight row**. Density/start/end
use scalar lerp; RGB uses `(b-a)*t+a`. Exponent density is neither copied
nor interpolated by this function. `FogValues` intentionally excludes it.

`0x004c3d40` advances the transition weight by engine delta / manager
+0x520 duration, clamping to one. `0x004c4330` samples both current and
previous weather, then blends them with that weight (the same quantized
colour operation). `0x004c4da0` performs the analogous fog transition.
The crate samples single resources; cross-weather transitions, zone
overrides and clock progression are documented but not implemented.

## Actual consumers and integration boundary

WEP sampled values live at manager +0x834 plus list index * 0x30.
These are **effect/model colour corrections**, not six primary sun/ambient
light definitions. `cEffectZoneModel` consumers `0x005d2a20` and
`0x005d3360` select a list through table `0x01875570`:

| Consumer correction ids | WEP list index | Reflected manager list name |
|---|---:|---|
| 1, 2, 3 | 0 | EffectColor |
| 4, 5 | 1 | WaterEffectColor |
| 6 | 2 | AddEffectColor |
| 7 | 3 | AddNEffectColor (ADD NIGHT) |
| 8 | 4 | SubEffectColor |
| 9 | 5 | DarkSkyEffectColor |

Id 0 is disabled/sentinel (table value 7), not an extra WEP list.
`correction_list` exposes only valid ids. The consumer normalizes packed
RGB bytes by 1/255, applies colour/intensity blend and shadow parameters,
and permits zone overrides. List names are backed by reflection at
`0x004bca80` (checked using `dti_props.py`, not inferred from colours).
The final material/shader constant writer is unresolved. List 0 intensity also
feeds manager +0x260 as `intensity_blend * intensity + (1-intensity_blend)`.

Fog: `0x004c4da0` writes manager start/end/density +0x96c/+0x970/+0x974
and RGB +0x980. `uSkyColorFog::vf08` at `0x00c65cd0` copies these into
`uFog` start/end +0x188/+0x18c, density +0x194 and RGB +0x1a0, unless a
zone supplies fog overrides. The exponent member +0x978 is copied from
the manager separately; it was not sampled from WFP. Zone transition
handling has additional asymmetries and is not translated here.

Primary directional lighting: `uSkyInfiniteLight::vf08` calls
`0x00c688a0`, which uses sky colour (`0x00f32fd0`), scheduler-provided
weather parameters and optional zone-light override, then writes light RGB
at +0x50 and direction at +0x230. This is a separate path from WEP.
Terrain shading agent claude-3 was notified of the reader and these leads;
its shader evidence identifies `sLightParamDL0`, `fSHCoef[7]` and `CBFog`
as the remaining renderer-side connections.

Next action: trace scheduler bindings and the light/fog constant writers,
then compare sampled noon/night and midnight-boundary values with the
original through the authorized oracle agent before renderer integration.
