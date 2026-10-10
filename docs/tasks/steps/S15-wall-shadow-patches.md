# S15: The dark patches on Cassardis walls

- **Branch:** `step/s15-wall-shadows`
- **Files:** `crates/ddview/src/stage.rs`, `sky.rs`, `main.rs` shadow
  setup, `dd_material.wgsl`
- **Kind:** diagnose and fix one visible defect

## Context

docs/tasks/render-fidelity.md Results: the sun shadow now uses the
original's three cascades, the 10 cm caster offset (0x00C6C7A0 / vertex
shader 0x0A75E0) and its 3x3 comparison filter. The street edge now
matches, but a right-hand wall in the same-pose Cassardis street capture
shows dark patches the original doesn't have. Candidates codex-1 listed:
which meshes cast (casters that the original excludes), slope bias, the
filter. Captures: codex-1's same-pose pair (see that brief's Results for
paths).

## Do

Find why that wall darkens (try toggling casters by model/part, the
receiver's normal offset, the cascade the patch falls in), compare with
the original's shadow-cast pass rules, and fix it without eye-tuning.

## Done when

The same-pose capture no longer has the patches, the street edge stays as
good, tests pass, and the Result names the cause with its source.
