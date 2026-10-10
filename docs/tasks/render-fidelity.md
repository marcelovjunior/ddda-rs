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

No renderer change or visual match is claimed by this evidence checkpoint.
Next: select the supported MRL bump/UV variants, preserve their tangent
basis and sample/decode the normal map in the custom material; verify
the WGSL pipeline and compare the requested poses. Detail-normal, skin
edit blends, AO and shadow differences remain separate investigations.
