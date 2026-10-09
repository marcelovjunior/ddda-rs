# m1-lighting: sun, ambient, fog and foliage for the M1 look gate

- **Owner:** claude lane
- **Branch:** `task/m1-lighting`
- **Board area:** `render/shading`
- **Status:** open

## Goal

Close the remaining gaps between our renders and the reference
screenshots (`docs/SHADING.md`, "Not resolved"):
- **Lighting:** the sun direction and colour, ambient (`fSHCoef` spherical
  harmonics), `sLightParamDL0`, `CBFog` and the light buffer, driven by
  the time of day. The data side is merged: `mtweather`,
  `docs/WEATHER.md`, `docs/WEATHER_CLOCK.md`
  (`uSkyInfiniteLight::update` 0x00C688A0 writes light RGB +0x50 and
  direction +0x230; fog sink `uSkyColorFog::vf08` 0x00C65CE0 → `uFog`
  +0x188.. start, end, density, RGB).
- **Foliage billboards:** the leaf cards that the original turns toward
  the camera (candidate `CBSwingBillboard`).
- **Alpha-test reference:** trace it (lead: MRL `bits[1]` byte 1).
- **Viewer fixes:** `--ref` should take `origin_cell` from the json, and
  our framing is narrower than the original's at the same pose (check
  the fov convention and the aspect ratio).

**Live reads** of the light, fog and SH values at the reference poses'
time of day go through claude-4's oracle. They are read-only and
batched: claude-4 is also recording acceptance routes.

## Scope

- `crates/ddview/src/stage.rs`
- `crates/ddview/src/main.rs`
- the viewer's shader files
- `crates/mtshader/**`, `crates/mtmrl/**`, `crates/mtweather/**` (reader
  additions)
- `docs/SHADING.md`

## Done when

- Refs 001, 002, 004 and 005 compare side by side with traced lighting
  and fog, and the framing matches.
- The remaining differences are listed.
- Fill in Results and report to `head`.

## Results

(filled in by the agent)
