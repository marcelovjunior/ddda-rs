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

### 1. Census (Ghidra 12.1.4 over HTTP, the exe's bytes)

**How targets are made.** Every texture goes through nDraw::Texture
(`FUN_01104F40` stores the MT format; `FUN_01105240` creates it), built
by four wrappers: `FUN_01105A20`, `FUN_01105A90`, `FUN_01105B20` (no
callers) and `FUN_01105B80` (named, pooled). The resource type passed to
them decides the D3D9 call in `FUN_01105240`:
- type 2: CreateTexture with D3DUSAGE_RENDERTARGET, D3DPOOL_DEFAULT,
  format from the MT table at 0x01440698;
- type 5: a depth-stencil. nDraw::DepthStencilView makes
  CreateDepthStencilSurface (0x01104566); with flag 0x80 it also makes an
  A8R8G8B8 render-target texture;
- type 1: D3DUSAGE_DYNAMIC, DEFAULT pool;
- type 3: SYSTEMMEM;
- everything else: MANAGED.

The usage passed to CreateTexture is only 0, RENDERTARGET or DYNAMIC,
never D3DUSAGE_DEPTHSTENCIL. Render-target views (`FUN_011042B0`) use the
texture's surface, or CreateRenderTarget (0x01104458) with the
multisample type and quality from the table at 0x01440450 when the
texture is the multisampled kind (the antialiasing option).

**Formats the renderer creates as render targets** (from the 198 calls of
the wrappers in 42 caller functions, plus 44 calls in the named ones (ctors, vf10, vf19, vf21, vf22), which add no other format; MT code → D3DFMT through 0x01440698):
- **A8R8G8B8** (MT 0x27, or 0x28 when sRender+0x31, the sRGB option, is
  set; same D3DFMT): G-buffer, LightMask/AmbientMask, DSFBuffer, the
  water depth map, SSAO normals and temps, mirror, distortion, heightmap,
  1x1 and 256² helpers, and post-effect temps (`FUN_0100EA80`,
  `FUN_0100F190`, `FUN_010125E0`).
- **A16B16G16R16F** (MT 2 and 0x26): the HDR light accumulation. Used
  when `FUN_011488C0` gets the HDR mode and `FUN_00DA9EC0` found
  render-target use plus post-pixel-shader blending for it (flag 2 of
  sRender+0x43CC04). Also clouds ("Temp cloud"), "RTMirror", reduction
  passes, and the generic creator `0x0100DA00`.
- **A2B10G10R10** (MT 6): second light buffer and post-effect temps when
  `FUN_00DA9EC0` found render-target use plus blending for it (flag 4).
- **R32F** (MT 0xE): "LogScene", LogScene64/16/4/4C (luminance
  reduction), SSAO "LocalLinearDepthMap"/"GlobalLinearDepthMap"/
  "TempDepthMap", "OcclusionFactor", `FUN_00EE2640`.
- **G16R16** (MT 0xC): "TempTexture" in `FUN_00EE06B0`/`FUN_00EE2640`,
  and cube faces in `FUN_00FE5A80` (managed there).
- **A32B32G32R32F** (MT 1): 64-wide targets in `FUN_00F23830` (or
  A16B16G16R16F); managed 64xN textures in `FUN_00F13AD0`.
- **Any table format from data:** the resource creator `FUN_00FDA400`
  (called by `vf10` at 0x00FDABB5) takes the format as a parameter.

**Depth.** Every depth-stencil is **D24S8** (MT 0xF, type 5): scene,
reduction, cubemap, mirror, water, "DummyZ" and "E2DDephtScratch"
surfaces, multisampled with the antialiasing option. **Depth is never
sampled as a texture:** no D24S8 texture is created (no
D3DUSAGE_DEPTHSTENCIL), and the exe contains none of the FOURCC codes
INTZ, RAWZ, DF24, DF16, RESZ, ATOC or NULL as immediates or data (byte
search of .text, .rdata and .data). Passes that need depth render it
themselves into R32F (SSAO linear depth). So step 3's FOURCC item does
not apply to this game, and depth contents only matter through depth and
stencil tests.

**Capability checks** (`FUN_00DA9EC0`, on the X8R8G8B8 adapter format;
the runtime answers them from the recorded driver profile):

| Format | Usage asked |
|---|---|
| A16B16G16R16F | texture with POSTPIXELSHADER_BLENDING (sRender+0x43CBEE) and with QUERY_FILTER (+0x43CBEF); vertex texture (QUERY_VERTEXTEXTURE, 0x43CC00 bit 2); RENDERTARGET surface plus blending texture (0x43CC04 bit 2) |
| R32F, A32B32G32R32F | vertex texture (0x43CC00 bits 4, 8) |
| A8R8G8B8, A2B10G10R10 | RENDERTARGET surface plus blending texture (0x43CC04 bits 1, 4) |
| R2VB (FOURCC 0x42563252) | as a surface (bit 0x10) |

CheckDeviceMultiSampleType is asked for D24S8, A8R8G8B8, A16B16G16R16F
and A2B10G10R10 at 2..4 samples, plus the CSAA qualities on NVIDIA
(vendor 0x10DE).

**MRT.** The draw context's target setter (`FUN_00DA6FE0`) binds four
render targets (SetRenderTarget 0..3) and a depth-stencil from each pass
description, so a pass can draw into up to four targets. Which passes do
(the G-buffer and light-accumulation names suggest the deferred passes)
is data in the pass descriptions, not code.

