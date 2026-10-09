# m1-lighting: sun, ambient, fog and foliage for the M1 look gate

- **Owner:** claude lane
- **Branch:** `task/m1-lighting`
- **Board area:** `render/shading`
- **Status:** review

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

claude-3, branch `task/m1-lighting`. Details: `docs/SHADING.md` (sections
Alpha test, Swing billboards, Projection and placement, Lighting).

**Framing (traced, checked live).** The game renders with
`uCamera.mFov + CameraFov` (`uCameraBase::vf21` 0x007D3E00; CameraFov is
`[GRAPHICS] CameraFov` in the player's config.ini, 15 here, so 55 -> 70
degrees, confirmed by the projection in sCamera's viewport 0). `--ref` now
adds it and takes `origin_cell` from the JSON. Layout rotations use
extrinsic XYZ (R = Rz Ry Rx), measured on a log in the live capture (not
traced). With both fixes refs 001, 002, 004 and 005 line up with our renders
(sword, skull and bones, armour pile, tree trunk, rocks in place).

**Foliage billboards (data + traced shaders).** New `mtswm` reads
`rSwingModel` (per-vertex swing pivots); nodes with flag 0x200 are
billboards. `dd_material.wgsl` turns their cards to the camera exactly as
the swing vertex shaders do (no wind). Bushes and tree crowns now look like
the references from above and below.

**Alpha-test reference (traced).** MRL `bits[1]`: enable bit 20, compare
bits 21-23, reference byte 1 -> `CBROPTest.fAlphaRef` (0x0112E4D0,
FUN_00FEE320). `mtmrl::Material::alpha_test`; the viewer uses it instead of
0.5.

**Lighting.** From the compiled shaders: light pre-pass (deferred
`colour * N.L * shadow` per light) plus SH ambient (hemisphere and
unshadowed lights; projections translated into `mtweather::lighting`,
packing assumed) and linear depth fog. Inputs are the sky light units and
`uSkyColorFog`, read live by claude-4 (read-only day series). `--light`
renders with them. Compared with six captures of one spot (08:14 to 23:04):
sunlit and shadowed areas fall where the game has them and ground colour
tracks the time of day, ours 15-25 % brighter by day (sRGB), slightly
darker at night. Assumed, not traced: the SH packer and the light-group
rule (`mGroup & model.light_group`), chosen because the group-0x10 lights
double the ambient against the captures.

**Refs 001/002/004/005:** time of day unknown (not recorded), so they are
rendered with the 12:21 capture's lighting and compared for framing,
materials, foliage and shadow placement only.

**Tests and build.** `cargo test -p mtmrl -p mtshader -p mtswm -p mttex
-p mtweather` pass (new: alpha test, SWM parsing, SH projections);
`cargo build --release -p ddview` passes; clippy clean on touched code.

**Remaining differences.** Post-processing (tone map, colour correction,
bloom, DOF; likely the day-time brightness gap) is not traced; sky dome,
water shading and far terrain are not drawn (ref001's distant hills, floating
far trees); wind sway; 8-byte swing records of `fmfore_*`; night lighting
(moon, point lights from the `st100_light` scheduler); the cylindrical
billboard variant; blending happens in linear, not sRGB; characters and the
chest (dynamic objects) are absent; `MaterialNull` shadow proxies skipped.
