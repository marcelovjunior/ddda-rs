# The GPU soft-body solver (L113)

About 900 of a gameplay frame's ~977 render passes are MT Framework's GPU
soft-body simulation (`uSimSoftBody`): cloth and hair solved by
position-based dynamics in tiny float render targets. This page is the
evidence for its replacement, and the first step of it: the passes run
as compute dispatches in the GPU backend (`DDGPU_NATIVE_SIM=1`, below).

Sources: capture `E:/ddda-re/captures/cap1.ddframe` (gameplay frames
7,340 to 7,558, analysed at frame 7,550 with
`cargo run --release -p d3d9frame --example sim_loops -- cap1.ddframe 7550 --shaders`),
the shaders' constant tables, and Ghidra (read-only). Codex-10's
[FRAME_PASSES.md](FRAME_PASSES.md) names the same passes `SoftBody` from
the captured reflection data and lists their consumers.

## The frame

Frame 7,550 has 5,432 draws. The first 904 are the soft-body passes, in
one run before anything else:

| Bodies | Target size | Passes per body |
|---:|---|---:|
| 12 | 64x8 | 40 |
| 7 | 64x16 | 40 |
| 2 | 64x8 | 72 |

Every pass draws one quad (a 4-vertex triangle strip, viewport = the
target) into `A32B32G32R32F` render-target textures. The vertex shader is
the same for all of them (a quad with `fScreenHalfPixelOffset`).

## One body's passes

A body with 40 passes (64 columns, 8 or 16 rows: particles of 64 strands):

| Passes | Pixel shader (capture id) | Constants (from the shader's table) | Reads | Writes |
|---|---|---|---|---|
| 1 | skinning (#55) | `CBSkinning`, `fSbLWMatrix`, `fSbDiffMatrix`, `__tMatrixMap__invsize` | the body's state textures (3 x F32), a rest-pose F16 texture, `tMatrixMap` (1024x64 F32 bone matrices, uploaded every frame) | 2 targets (MRT) |
| 1 | integration (#56) | `CBSoftBodySim`, `CBSoftBodyRtParam2`, `fSbGravity`, `fWinds`, `fWindParams`, `fWindNum`, `fSbDirectGrassWind`, `fSbDirectGrassWindStrength`, `fSbLWMatrix`, `fSbLWMatrixInv` | the previous state, the skinned positions | 2 targets (MRT) |
| 4 x 8 | constraint relaxation (#57) | `CBSoftBodyQuad`, `CBSoftBodyRtParam` | one of 8 per-body F16 constraint textures (uploaded at load), the current positions, the skinned positions | 1 target, ping-pong between two |
| 4 x 1 | collision (#58), after each round of 8 | `CBSoftBodyCollision` | the positions, four 64x1 F32 collider textures | 1 target |
| 2 | back to model space (#59) | `fSbDiffMatrixInv` | the positions | 1 target |

The 72-pass bodies run more relaxation rounds.

The results are read by vertex shaders in the shadow and scene passes
(texture fetch in the vertex shader, e.g. a shader with
`tSoftBodySrcTex1`, `tSoftBodySrcTex4` and `fWorld`).

## Where the game records it

Addresses in the 2017-12-13 Steam exe; names are research labels.

- `uSimSoftBody` (DTI `0x018D1E04`, object vtable `0x014343B8`, object
  size `0xBF0`): one per simulated body.
- `uSimSoftBody::vf11` `0x00F20640` (the unit's render-time method)
  gathers the wind sources (`uGrassWindDirection`, `uGrassWindPoint`),
  opens a "SoftBody" marker (`0x00FEDB80`) and records the passes:
  - `0x00F119C0`, `0x00F0F9B0`: skinning and integration;
  - a loop over `param_1[0x56]` rounds: `0x00F11610(ctx, k & 7, ...)` for
    `k < param_1[0x2CF]` (constraint passes), then `0x00F10110` (collision;
    `0x00F10E30` on a condition);
  - `0x00F0EC00`: the final passes;
  - `0x00FEDC00` closes the marker.
- A pass is recorded through the deferred render context (`param_2`): the
  state changes into its log at `+0x30C7C` (state ids, e.g. `0x3A9`,
  `0x3AA`, `0x3AC` for texture slots), then a 32-byte draw record. The
  executor (`0x00DAB480`) later turns these into the D3D9 calls.
- `uSoftBody::vf11` `0x00E80040` -> `0x00F23260` is the soft body's model
  draw, which binds the results.

## What a replacement must keep

- The results bit for bit: they feed the vertex positions of the scene and
  shadow draws.
- The order: the simulation runs where the executor reaches the
  "SoftBody" range, before every consumer in that frame.
- The inputs per frame: bone matrices, wind, constants; and the state the
  previous frame left in the ping-pong targets.

## Cost

Since L124 the render thread bounds the frame. It translates each of the
~900 passes as about 15 D3D9 calls (targets, up to 7 textures, shaders,
constants, viewport, the draw). The replacement therefore has to sit where
the commands are replayed (the executor), not only in the GPU backend.

## Step 1: the passes as compute dispatches (`DDGPU_NATIVE_SIM=1`)

`crates/ddgpu/src/compute_quads.rs`. A draw runs as a dispatch in one
compute pass, instead of a render pass of its own, when a general rule
proves the results cannot differ (no game shader is named):

- `d3d9shader::flow`: the pixel shader's interpolated inputs reach nothing
  but the coordinates of texture fetches (taint analysis with branch joins
  and loop fixpoints); the fetches they address sample one texel (point
  filters, one level) and none has an implicit level. All six soft-body
  pixel shaders qualify.
- The vertex shader, evaluated on the CPU for the 4 vertices, draws an
  axis-aligned rectangle with w = 1 over every pixel centre of a viewport
  that is the whole target; culling is off.
- Float32 render-target textures of one size, no depth, blending, alpha
  test, scissor or sRGB write, all channels written, no target sampled,
  no occlusion query counting.

The dispatch runs the draw path's translation of the same pixel shader
(d3d9wgsl, same options except the alpha test, which is off), with the
interpolants evaluated at each pixel centre, writing storage textures.

Verified:
- `cargo test -p ddgpu --test quads`: a synthetic two-pass ping-pong with
  dependent fetches and MRT, both paths read back bit for bit; a blended
  draw stays a render pass.
- `cargo run --release -p ddgpu --example sim_oracle -- cap1.ddframe 7540 7550`:
  the capture replayed into both paths (uploads, clears, releases and the
  small float draws), every soft-body target compared at each frame's
  end: 11 gameplay frames, ~126 targets each, **0 differences**; every
  soft-body draw ran as a dispatch (904 per frame). Peak memory 1.75 GB
  (the reader keeps blobs in the file; a watchdog stops it above 4 GB).

Measured (`sim_oracle ... --bench 50`, frame 7,550's 904 draws replayed
into a warm backend, wgpu on Vulkan, this machine):

| Path | Record (CPU) | Submit and run | Total |
|---|---|---|---|
| render passes | 5.30 ms | 29.81 ms | 35.1 ms |
| compute dispatches | 3.47 ms | 12.83 ms | 16.3 ms |

Not yet: the render thread still translates every pass as ~15 D3D9
calls (step 2, the executor route, L112/L123), and the game has not been
run with the switch (the head runs it).
