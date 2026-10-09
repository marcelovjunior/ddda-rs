# weather-data: Weather, fog and sky data

- **Owner:** codex lane
- **Branch:** `task/weather-data`
- **Board area:** `world/weather`
- **Status:** review

## Goal

Typed readers for `rWeatherEffectParam` (wep), `rWeatherFogParam` (wfp) and `rSky`. Then trace the list consumers and how the game clock interpolates between time keys.

## Evidence

Start from `docs/SURVEY.md`, section "7. Day/night and weather": the resource classes,
counts, sample structures, exe loader addresses and reference coverage
are there. The tools are listed in `docs/tasks/README.md`.

## Scope

- `crates/mtweather/**`
- `docs/WEATHER.md`
- `crates/ddtool/src/<name>.rs`, if you add a CLI module (shared hookups as in `docs/CODEX.md`)
- this brief's Results

## Done when

Every WEP/WFP/SKY instance parses with time-key ranges and record bounds checked. WEATHER.md explains which file applies where and when, with exe addresses (consumers, clock interpolation). claude-3 (terrain-shading) will consume this: tell it on the board when the reader is usable. Distinguish traced from guessed, and fill in Results. Then report
to `head`.

## Results

Implemented `mtweather` WEP v1/WFP v1/SKY v6 bounded typed readers, raw
texture paths, correction-type lookup, and executable-derived WEP/WFP
sampling. Five synthetic tests pass; the opt-in installed corpus test passes
all 149 ARC instances: 64 WEP paths/instances (1,961 rows), 55 WFP
paths/instances (712 rows), 2 SKY paths/30 instances. Every WEP/WFP list
was also sampled at all 1,440 minutes. No game resources are committed.

Evidence and integration notes are in `docs/WEATHER.md`: loaders
0x00cea2b0/0x00ceaa80/0x00e2c350; stage selection and midnight sentinel
0x004c19f0; WEP sampling 0x004c4330; WFP sampling 0x00cead20;
clock 0x004c0e30 and decomposition 0x00437210; correction consumers
0x005d2a20/0x005d3360, fog sink 0x00c65cd0, primary directional-light
path 0x00c688a0. WEP has two backwards adjacent keys (st330_f list 1,
st610_f list 3); the reader retains their order, matching the executable.
WFP exponent density is stored but not sampled by the traced function.

claude-3 received usable API and lighting/fog consumer leads on the board.
Unknown: final shader constant writers,
some SKY field meanings, scheduler-to-SKY bindings, stage380 override path,
all clock mode gates and real-time rate. Cross-weather blending, zone
overrides and clock stepping are traced/documented but not implemented.
Not compared against the running original. Next: scheduler/light constant
tracing and authorized oracle comparison before rendering integration.
