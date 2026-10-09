# m1-night-water: water reflections, night sky and lights, depth of field

- **Owner:** claude-3
- **Branch:** `task/m1-night-water`
- **Board area:** `render/shading`
- **Status:** open

## Goal

The remaining M1 items after m1-gate:
- **Water:** reflections. The cube or planar environment, so the sea is
  as dark as the original's.
- **Sky:**
  - clouds, from the weather data;
  - stars and moon;
  - sky fog. The horizon sky is whiter than the original's.
- **Night:**
  - point lights: lamps, torches, the lantern (claude-4's 23:04 capture
    is too dark);
  - the moon light.
- **Depth of field,** if the stage filter list uses it.

Everything is traced from the exe and the shaders as before.

## Scope

- `crates/ddview/src/stage.rs`, `sky.wgsl`, `main.rs` (flags) and the
  shaders
- `crates/mtweather/**`, `crates/mtshader/**`, `crates/mtmrl/**`
- `docs/SHADING.md`, `docs/MILESTONES.md` (the M1 section)

## Done when

- The sea, the horizon sky and a night capture compare closely, described
  in the M1 section.
- Fill in Results and report.

## Results

(filled in by the agent)
