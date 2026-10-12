# L29: every render-target and depth format the renderer uses, against the driver

- **Owner:** claude-5
- **Branch:** `task/L29-targets` (worktree `E:\ddda-rs-wt\L29`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

L20 matched every render state, sampler state and sampled texture format
against the host's D3D9 driver, but the oracle (`crates/d3d9ref`) reads
back only 8-bit A8R8G8B8 targets. The game's renderer draws into other
targets too: float targets for HDR lighting and post-processing, single-
and two-channel targets, shadow maps, depth-stencil surfaces that it may
sample as textures (vendor FOURCC formats such as INTZ/RAWZ/DF24), and
possibly several targets at once (MRT). A frame is only right if all of
these are.

## Task

1. **Census** (Results, with addresses): every render-target, depth-stencil
   and texture format the renderer creates or checks for, and how each is
   used (render target, sampled, MRT index, depth sampled as a texture).
   Use the format table and CheckDeviceFormat calls the L20 census found,
   plus CreateRenderTarget, CreateTexture with D3DUSAGE_RENDERTARGET or
   D3DUSAGE_DEPTHSTENCIL, and SetRenderTarget indices > 0.
2. **Oracle:** extend `d3d9ref` to read back every one of these targets
   exactly. That means float formats as their raw bits, depth through the
   driver's documented paths (a FOURCC depth texture sampled into a float
   target, or lockable depth where the driver allows it), and all targets
   of an MRT draw.
3. **Backend:** implement what is missing in `ddgpu`, compare draws into
   each format against the driver (extend the state fuzz with target
   formats, MRT and depth sampling), and fix what differs. That includes:
   - float blending and clamping rules;
   - sRGB writes;
   - depth precision (D24 vs a wgpu Depth32Float or Depth24Plus emulation,
     and what that does to depth comparisons and sampled depth);
   - the D3D9 behaviour of FOURCC formats, as the driver exposes them.
4. **Record** what cannot be exact and why, with measured bounds, in
   Results, and update the `docs/RUNTIME.md` rows.

## Results

(open)
