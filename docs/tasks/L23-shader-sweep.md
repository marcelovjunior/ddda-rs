# L23: every game shader translated and matching the driver

- **Owner:** claude-7
- **Branch:** `task/L23-shader-sweep` (worktree `E:\ddda-rs-wt\L23`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

The game ships 29,198 distinct SM2/SM3 shaders in 76 shader packages.

- `crates/d3d9shader` decodes all of them exactly like D3DDisassemble.
- `crates/d3d9wgsl` translates them to WGSL. Translation is checked against
  the host's D3D9 driver through `crates/d3d9ref`, in
  `crates/d3d9wgsl/tests/against_d3d9.rs` (`DDDA_SHADER_PACKAGES`,
  `DDDA_SHADER_LIMIT`, `DDDA_SHADER_TRACE`).
- Pixel shaders matched 600/600 for root/Stage100 plus Title. Vertex
  shaders and the full sweep are unfinished.

A shader that differs means a wrong frame.

## Task

1. Run the full sweep, pixel and vertex, over all 76 packages, with
   several random constant, input and texture sets per shader. It runs
   in the background and in chunks; the crates are light and need no
   heavy slot. Record per package: translated, matching, and differing with
   the first difference.
2. Fix every difference in the translator, with the D3D9 rule behind it
   (the spec, the driver's measured behaviour, or the bisection tooling in
   the test: `prefix`/`prefix_to`). Rules already found are the D3D9
   multiply rule, relative-address range, half-pixel offset and
   derivatives (see `docs/RUNTIME.md`, implementation status and findings).
3. Make sure every translated shader passes WGSL validation in naga and
   wgpu: the backend creates pipelines from them.
4. Record in Results the final counts and any shader that cannot match
   exactly, with the reason (for example driver precision on transcendental
   instructions, given as a measured ULP bound). Update the `docs/RUNTIME.md`
   row.

## Results

(open)
