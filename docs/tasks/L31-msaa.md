# L31: multisampling (the AntiAlias option) against the driver

- **Owner:** claude-5
- **Branch:** `task/L31-msaa` (worktree `E:\ddda-rs-wt\L31`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

L29's census (Results section 6) found what the game multisamples:
- The "AntiAlias" option is `sRender+0xEC` (setter 0x00DA7D20).
- Nine modes at 0x01440450: none, 2x/4x/8x, and NVIDIA CSAA (4,2), (8,0),
  (4,4), (8,2), (8,4).
- Support is gated per format (0x011487C0 against the bits 0x00DA9EC0
  probes).
- Only the scene colour (A16B16G16R16F or A8R8G8B8), the scene D24S8 and
  "ZPrepass Dummy" are multisampled.
- The resolve is draw-command case 5: colour via StretchRect
  D3DTEXF_NONE into a texture level; depth via depth-to-depth
  StretchRect outside the scene.

The runtime records the multisample type, but `d3d9cmd::Target` does not
carry it, so the backend draws these targets single-sampled.

## Task

1. The device profile answers CheckDeviceMultiSampleType exactly as the
   recorded driver does, CSAA quality levels included. Check against
   `data/d3d9_profile.txt`; extend the recorder `scripts/d3d9_profile.py`
   if the profile lacks these answers.
2. Carry the sample count and quality in `d3d9cmd::Target`, and create
   multisampled wgpu targets.
3. Implement the resolves the game uses:
   - colour: StretchRect from MSAA to a texture;
   - depth: depth-to-depth StretchRect, which D3D9 drivers allow for
     specific formats. Measure what the driver does.
4. Compare against the driver through `d3d9ref` (extend it to MSAA
   targets and resolves). CSAA's coverage samples have no wgpu
   equivalent: measure what the driver produces and record the closest
   exact-or-bounded mapping.
5. Results and the RUNTIME.md row. Verification follows the merge-train
   rule: light crates on your branch, the rest in the integration build.

## Results

Status: implemented and compared against the driver (RTX 4060, 610.62).
This branch contains task/L29-targets, and through it claude-9's L25
caches.

### 1. Profile answers

`crates/ddruntime/data/d3d9_profile.txt` already holds
CheckDeviceMultiSampleType for every format, windowed and full-screen
(2,856 answers), and the runtime answers from it (`d3d9.rs`
`Profile::multisample`). Windowed and full-screen answers are identical.
For the formats the game probes (A8R8G8B8 0x15, D24S8 0x4B,
A16B16G16R16F 0x71, A2B10G10R10 0x1F), the recorded driver (RTX 4060,
610.62) offers:

| Type | Quality levels |
|---|---|
| NONE | 1 |
| NONMASKABLE | 4 |
| 2 samples | 1 |
| 4 samples | 1 |
| 8 samples | 1 |
| everything else | D3DERR_NOTAVAILABLE |

So the CSAA modes 4-8 of the game's table (4 or 8 samples at quality 2
or 4) are not available on this driver. The game's NVIDIA-only probe in
`FUN_00DA9EC0` compares the returned quality levels and leaves their bits
clear, and `FUN_011487C0` then refuses those modes. The player can choose
none, 2x, 4x or 8x. No recorder change was needed.

### 2. Implementation

- **`d3d9cmd::Target`** carries `samples` (the D3DMULTISAMPLE_TYPE) and
  `quality`. The runtime fills them from the surface description.
- **Surface creation** (`d3d9res.rs` `create_surface`): a multisample type
  must be offered for the format with the quality below its level count,
  and a multisampled render target cannot be lockable. Measured on the
  driver (`explore_multisampling`): 4x quality 0 succeeds; 4x quality 1
  or 2, 8x quality 2 and 16x give D3DERR_INVALIDCALL; NONMASKABLE quality
  0-3 succeeds and 4 does not. That is exactly the profile's answers.
- **GetRenderTargetData** refuses a multisampled target (Learn).
- **ddgpu:**
  - multisampled render textures (render attachment and binding only;
    WebGPU cannot copy them);
  - one sample count per draw, which must match across all attachments;
  - the sample count in the draw, clear and blit pipeline keys, and
    D3DRS_MULTISAMPLEMASK as the pipeline's sample mask;
  - read_back gives nothing for a multisampled target.
- **Colour resolve** (StretchRect from a multisampled source): a render
  pass resolves into a temporary single-sampled texture, which is then
  copied or blitted as before. **The driver resolves 8-bit targets
  gamma-correctly**: the samples are averaged as linear light and encoded
  back to sRGB. A white pixel a quarter covered resolves to 137 (the sRGB
  encoding of 0.25), not 64. ddgpu does the same by resolving through sRGB
  views. Float targets resolve linearly, as the driver does.
- **Depth-stencil StretchRect** is drawn, because WebGPU cannot copy
  depth-stencil textures:
  - depth via frag_depth (stencil cleared in the same pass), then one pass
    per stencil bit;
  - between equal sample counts every sample is copied (per-sample
    shading);
  - from a multisampled source to a single-sampled destination, the
    driver copies one sample: **sample 0 at 2x, 3 at 4x and 6 at 8x**
    (measured; in the standard sample positions, the bottom-most sample).
- **Oracle (`d3d9ref`):**
  - targets are created with their multisample type;
  - `render_copied` and `render_placed` replay StretchRect copies at given
    positions between the draws, outside the scene;
  - a clear now binds all its own targets. Before, render target 0 stayed
    the read target, and a clear with a multisampled depth-stencil did
    nothing.

### 3. Compared against the driver (`crates/ddgpu/tests/msaa.rs`)

| Test | Result |
|---|---|
| `multisampled_draws_resolve_like_the_d3d9_driver`: two random triangles with slanted edges, depth tested, one blended, into 2x/4x/8x A8R8G8B8 and A16B16G16R16F targets with a multisampled D24S8, resolved and compared byte for byte | 48 of 48 equal |
| `depth_stencil_copies_match_the_d3d9_driver`: depth 0.25 and stencil 0x5A written, copied 1x→1x, 2x/4x/8x→1x and 4x→4x, then probed with depth and stencil tests | 25 of 25 equal |
| `the_backend_resolves_and_copies_multisampled_targets` (backend only) | passes |

The multisampled draws match exactly, so the driver's sample positions
equal wgpu/Vulkan's standard positions at 2, 4 and 8 samples.

### 4. Not modelled (measured)

- **D3DRS_MULTISAMPLEANTIALIAS = FALSE.** The driver keeps the per-sample
  coverage, and in addition writes every sample of a pixel whose centre is
  covered (pixels at 3/4 coverage, and some at 1/2, become fully covered;
  others keep their partial coverage). wgpu has no switch for this, and a
  fragment shader's sample mask can only remove coverage, not add it. So
  ddgpu always antialiases. The game sets this state from its raster-state
  data (render state 161 in the L20 census); which passes turn it off is
  data, not code.
- **D3DRS_MULTISAMPLEMASK** is passed as the sample mask. The game never
  sets it (it is not in the L20 census), and the driver's results with a
  partial mask did not follow the documented per-sample meaning in the
  probe, so this is not compared.
- **StretchRect with stretching from a multisampled source.** The driver
  returns D3D_OK but writes zeros; ddgpu resolves and scales. The game
  only resolves at the same size (draw-command case 5).
- **CSAA.** This driver offers no CSAA qualities, so the game never asks
  for them. The backend reports an error if a quality above 0 ever
  arrives.
- **MSAA to MSAA depth copies between different sample counts** are an
  error in ddgpu (not measured; the game copies to a single-sampled or
  equal depth surface).

### 5. Verification

- **Verified:** `cargo test -p ddgpu` (draw 12, msaa 3, targets 4, cache,
  present), against the host driver where applicable.
- **For the integration build:** the ddruntime changes (`d3d9res.rs`
  validation, `d3d9copy.rs` GetRenderTargetData, `d3d9draw.rs` target
  fields). They are not built on this branch under the batched rule.
