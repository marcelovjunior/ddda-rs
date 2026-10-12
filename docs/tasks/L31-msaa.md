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

(open)
