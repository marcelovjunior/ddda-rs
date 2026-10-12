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

Status: code written, not yet built or compared (heavy work paused). This
branch contains task/L29-targets (and through it claude-9's L25 caches).

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

### 2. Design (implemented, to be measured)

- **`d3d9cmd::Target`** carries `samples` (the D3DMULTISAMPLE_TYPE) and
  `quality`. The runtime fills them from the surface description.
- **Surface creation** in the runtime (`d3d9res.rs` `create_surface`):
  a multisample type must be offered for the format with the quality
  below its level count, and a multisampled render target cannot be
  lockable (Learn, CreateRenderTarget). The driver's own answers are
  printed by `explore_multisampling`.
- **GetRenderTargetData** refuses a multisampled target (Learn).
- **ddgpu:**
  - Multisampled render textures. WebGPU cannot copy them, so they are
    render attachments and bindings only.
  - Every attachment of a draw shares one sample count (D3D9 requires
    matching types).
  - The sample count is in the draw pipeline key, the partial-clear
    pipeline and the blit pipeline, and D3DRS_MULTISAMPLEMASK is the
    pipeline's sample mask.
  - StretchRect from a multisampled colour source first resolves it (a
    render pass with a resolve target) into a temporary single-sampled
    texture, then copies or blits as before. A multisampled destination
    is drawn into.
  - read_back gives nothing for multisampled targets.
- **Depth-stencil StretchRect.** WebGPU cannot copy depth-stencil
  textures, so ddgpu draws the copy: depth through frag_depth from the
  source's depth aspect (stencil cleared in the same pass), then one pass
  per stencil bit setting the bits the source has. A multisampled source
  gives each pixel one sample (`DEPTH_COPY_SAMPLE`, to be measured).
- **Oracle:** targets are created with their multisample type, and
  `render_copied` replays StretchRect copies after the scene.
- **Tests:** `crates/ddgpu/tests/msaa.rs`:
  - `multisampled_draws_resolve_like_the_d3d9_driver`: 2/4/8 samples,
    A8R8G8B8 and A16B16G16R16F, with a multisampled D24S8, resolved and
    compared bit for bit;
  - `explore_multisampling`: creation answers, resolve rectangles and
    stretching, depth StretchRect, MULTISAMPLEANTIALIAS off,
    MULTISAMPLEMASK.

### 3. Open until measured

- the resolve filter (wgpu/Vulkan average against the D3D9 driver's);
- which StretchRect rectangles the driver accepts from a multisampled
  source;
- what depth StretchRect does with a multisampled source;
- MULTISAMPLEANTIALIAS = FALSE (D3D9 rasterises single-sampled into a
  multisampled target; wgpu has no switch for this);
- CSAA: not offered by this driver, so there is nothing to map. The
  backend reports an error if a quality above 0 ever arrives.
