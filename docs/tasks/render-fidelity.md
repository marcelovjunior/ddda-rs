# render-fidelity: Normal maps, ambient occlusion and shadow look

- **Owner:** codex lane
- **Branch:** `task/render-fidelity`
- **Board area:** `render/fidelity`
- **Status:** open (after the opening route's blockers)

## Goal

Maintainer, 2026-10-09 playtest: the look is close, but "rendering normal
maps, some ambient occlusion/shadows not looking right". Make the
viewer's stage and character shading match the original in Cassardis:
- normal maps: tangent space and handedness (mtmod tangents w is "biased
  byte" as stored), the normal map channel layout (BC5/DXT5nm swizzle?),
  and the shader's use of them (docs/SHADING.md);
- ambient occlusion: what the original uses (baked vertex AO, AO maps,
  SSAO post pass?) and where the viewer differs;
- shadows: sun shadow filtering/bias and the indoor look against the
  original.
Method: same-spot screenshot pairs (original via oracle, viewer via
`--shot` with the same eye/target), find the largest differences first,
trace each in the shaders (mtshader) and the exe.

**Rules.** Trace in the exe and shaders, label any guesses (AGENTS.md).
Live checks follow the safety rules in `docs/CODEX.md`.

## Scope

`crates/ddview/src/stage.rs` shading, `crates/mtshader`, `crates/mtmod`
(tangents), `docs/SHADING.md`, this brief's Results.

## Done when

Three same-spot pairs (class-selection room, a Cassardis street, the
field by the gate) show matching normal-map relief, AO and shadows, and
the differences that remain are listed. Fill in Results and report to
`head`.

## Results

Initial audit (codex-1): normal maps are not wired into the current
renderer. `mtmod` decodes tangents, but both stage and character mesh
builders omit them. `stage::Loader::material` does not bind `tNormalMap`,
and `dd_material.wgsl` uses the geometric normal. Head approved adding
the shader, character tangent hookup and `mtmrl::shading` metadata to
this brief's scope (board 2715).

Installed `sc\\DX9\\Stage200` shader evidence, disassembled privately:

- Pixel shader file offset **0x6171f8** names `SSNormalMap__tNormalMap` at
  sampler 0. It decodes **alpha and green**, subtracts **127/255**, then
  doubles them. Z is `sqrt(1-x*x-y*y)`, except values at or below 0.0001
  use 0.01. It combines the tangent/bitangent/geometric-normal basis and
  normalizes the result. Offsets **0x318e0c** and **0x5f3000** confirm the
  same decode in two other normal-buffer variants.
- Vertex shader file offset **0x1381d4** decodes normal and tangent as
  `2*byte/255-1`, including tangent W. It transforms their XYZ through
  the world 3x3 and computes **cross(tangent, normal) * tangent.w** for
  the bitangent; this has the opposite cross order to the usual Bevy
  convention. Only the geometric normal is normalized in this variant;
  tangent/bitangent retain the source lengths before interpolation.
- The texture reader already identifies normal format 0x1f as linear
  DXT5. Feeding these textures straight into an RGB normal decoder would
  therefore be incorrect.

Implementation checkpoint: stage meshes retain decoded tangents; the
custom forward material binds and decodes the map with the source basis.
Selection is deliberately limited to `FBumpNormalMap`, primary normal UV
and primary albedo UV. Normal UV uses the material's transform. Other
variants keep their previous geometric-normal path. Character meshes now
retain tangents, but character bump remains disabled: the original
skinned basis is not yet traced, and the viewer's skin matrices include
centimetre conversion that must not distort tangent lengths.

Validation: decoder/channel/Z-floor and basis handedness/length regression
tests pass, as do material-selection tests and strict mtshader/mtmrl
Clippy. Release ddview build passed (43.17 s). A bounded private field100
capture rendered 480 meshes / 397 instances with no WGSL pipeline errors.
This is rendering validation, not comparison against the original. The
recorded Cassardis street capture initially omitted the buildings because
the existing static preview does not load nonfield stage models; codex-32
has fixed that hook in the doors branch. No same-pose match is claimed yet.

Follow-up: head approved the static nonfield model hook (board 2760),
shared with the doors branch. Release build passed (38.72 s); the bounded
Cassardis capture loaded 31 stage models and 315 placed objects with no
shader errors. Recorded eye/target/up/near and 70-degree effective FOV
(base 55 plus the live configuration's 15) align the scenery. At recorded
16:06:11 fine weather, the largest visible difference is the sun-shadow
footprint: the original has a sunlit wedge across the road, while the
viewer leaves that road shaded. The static comparison excludes the actor
and HUD. No image-match pass is claimed; live sunlight inputs must be
checked before attributing the shadow discrepancy to filtering or bias.

The AO source audit is recorded in SHADING.md. The executable has a
local/global-map ambient-occlusion filter, but neither stage100 nor
stage200 f00 scheduler creates one; runtime activation is unresolved.

Next: compare live sunlight and authored lighting for the street pose.
Detail-normal, skinned bases, normal prepass, AO and shadow differences
remain separate investigations. Three requested comparison pairs are
still outstanding; this brief remains in progress.
