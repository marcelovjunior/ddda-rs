# m1-night-water: water reflections, night sky and lights, depth of field

- **Owner:** claude-3
- **Branch:** `task/m1-night-water`
- **Board area:** `render/shading`
- **Status:** review

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

Commits on `task/m1-night-water`: 19e81c9 (environment cubes, unlit
forward water), 3c1fbbc (night spot light), f178847 (depth of field),
6d2e153 (cloud domes), 1a5324d (effect render-target meshes left out),
plus the tests and docs commit. Details in `docs/SHADING.md` (Water, Sky,
Night, Post chain); the comparison in `docs/MILESTONES.md` (M1).

- **Water:** traced the sea's shader family (no light buffer: unlit,
  albedo x diffuse x VertexColor x Occlusion, inferred vertex inputs) and
  the environment cubes (`DDBaseCube<i>_CM` x the weather manager's
  `mEnvMapBaseColor` / `mEnvMapColor`). The sea is now dark navy: ref004
  (56,68,78) / ours (44,55,62), was (74,97,116); 9-20 % dark in refs
  001-005 (normal maps and the foam not done).
- **Sky:** the weather's cloud domes (`uSkyCloudModel`, lighting rules
  translated, their shader's combination assumed) under the stage fog
  make the horizon blue: ref004 (142,166,189) / (134,153,169), was
  (184,184,184). Stars and the moon are **not done** (no night-sky
  reference; their sources are listed in SHADING.md). The sky fog is the
  stage fog on the cloud domes; no `CBSkyFog` shader exists in the
  disassembled packages.
- **Night:** the sky's spot light (traced unit, colour fade and spot
  shader) above the player: refs-lit 23:04 ground (5,9,11) / (5,13,14),
  was (1,4,5). Lamp and lantern point lights and the moon light are **not
  done** (claude-4 reports the oracle cannot take a lamp-lit capture
  without the maintainer).
- **Depth of field:** stage 100's DOFFilter (type Light) traced by the
  sub-agent and drawn with DofFilter=ON; the frame carries depth in alpha.
- Day series unchanged (12:21 / 17:45 within 1-2 levels).

