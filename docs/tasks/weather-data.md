# weather-data: Weather, fog and sky data

- **Owner:** codex lane
- **Branch:** `task/weather-data`
- **Board area:** `world/weather`
- **Status:** open

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

(filled in by the agent)
