# m1-finish: post-processing, sky, water and distant terrain

- **Owner:** claude-3
- **Branch:** `task/m1-finish`
- **Board area:** `render/shading`
- **Status:** review

## Goal

The remaining M1 look gaps (`docs/SHADING.md`, open questions from
m1-lighting):
1. **The post-processing chain:** tone mapping, colour correction, bloom
   and depth of field, traced from the render path and its shader
   constants. This is the likely cause of our 15-25% brighter days.
2. **The sky dome** (`rSky`, `uSky*`) by time of day.
3. **Water shading** (`DDMaterialWater`).
4. **Distant terrain:** the far ring cells, so far trees stop floating.
5. **Night:** the moon and point lights, if quick.

Compare against claude-4's day series (`E:\ddda-re\shots\refs-lit`) and
refs 001-005.

## Scope

- `crates/ddview/src/stage.rs` and the shader files
- `crates/ddview/src/main.rs`: flags only
- `crates/mtshader/**`, `crates/mtmrl/**`, `crates/mtweather/**`,
  `crates/mtswm/**`
- `docs/SHADING.md`

`event.rs` and `play.rs` use stage types: keep their names stable, or fix
those callers.

## Done when

- The day captures match within a few percent of brightness after the
  post chain.
- Sky, water and the far ring are drawn.
- Fill in Results and report.

## Results

Commits on `task/m1-finish`: 758f8fd (far terrain, landmarks, stage filter
chain), 0905e64 (water materials, reflection stand-in, fog background),
f3a4575 (sky dome), plus the docs commit. Details in `docs/SHADING.md`
("Post chain", "Sky", "Water", "Distant terrain and landmarks").

1. **Post chain** (traced from `st100_f00` and the table shader): vignette
   (`ImagePlaneFilter`) and the `ColorCorrectFilter` tone curve, one
   fullscreen pass under `--light`. `mtweather::post::tone_curve` is
   translated with a regression test against the shader. Ground brightness
   against the refs-lit series now: 12:21, 17:45 and 20:21 within 1-3 %;
   09:46 13-17 % bright; 15:00 39 % bright (ground in a rock's shadow, cause
   not found); 23:04 too dark. No bloom filter exists in the stage's
   filter scheduler; **DOF is not done**.
2. **Sky dome:** `CBSky` and the optical depth maps translated
   (`mtweather::sky`, regression tests), the 10-step Rayleigh / Mie scatter
   shaders and the dome shader generated instruction for instruction into
   `sky.wgsl`, run per pixel (a difference from the 32 x 32 screen grid).
   The sun comes from the game clock (standard solar position for the
   resource's latitude / tilt / season, *reconstructed*, matches four live
   reads within 0.006); night is dark. Compared only by eye with ref004
   (time unknown): same gradient, upper sky ~20 % darker. Not drawn:
   stars, moon, cloud domes, sky fog. Fine weather assumed.
3. **Water:** the sea and DDMaterialWater keep their materials;
   transparency volume blend and a Fresnel reflection whose environment is
   a fog-colour **stand-in** (no cube / planar reflection, normal maps or
   sun specular yet); the sea is lighter than the references.
4. **Far ring:** far cells draw their terrain under `fmfore`; the 15
   `uFmSetBase` landmarks are placed from the exe table (0x01873550) with
   high/low models by radius and the story-progress swap (`--progress`).
   ref001's castle and the far ground are drawn.
5. **Night:** moon and point lights not done.

Needs from head: a live capture with sky in view and its light JSON
(claude-4) to compare the sky's brightness at a known time.

