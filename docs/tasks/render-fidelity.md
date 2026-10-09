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

(filled in by the agent)
