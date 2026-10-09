# m1-finish: post-processing, sky, water and distant terrain

- **Owner:** claude-3
- **Branch:** `task/m1-finish`
- **Board area:** `render/shading`
- **Status:** open

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

(filled in by the agent)
