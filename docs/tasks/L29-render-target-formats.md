# L29: every render-target and depth format the renderer uses, against the driver

- **Owner:** claude-5
- **Branch:** `task/L29-targets` (worktree `E:\ddda-rs-wt\L29`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)
- **Status:** ready for the integration train (claude-5)

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

### 2. Oracle (`crates/d3d9ref/src/replay.rs`)

- `render_targets` reads every requested target back as it is stored:
  GetRenderTargetData into a SYSTEMMEM surface of the same format, rows
  tightly packed. Float formats come back as their raw words, and all
  targets of an MRT draw are read.
- `render_cleared` replays several clears in order. A clear now applies
  its own viewport, rectangles, scissor state and sRGB write. Before this,
  states left by an earlier call leaked into Clear: SRGBWRITEENABLE from
  the previous case re-encoded the clear colour.
- `clear_with_states`/`clear_probe` measure what Clear does with states
  set.
- Depth: the census shows the game never samples depth (no D24S8 textures,
  no FOURCC formats), so the oracle observes depth through depth and
  stencil tests (probe draws). A lockable or FOURCC depth path was not
  needed.

### 3. Backend (`crates/ddgpu`), compared against the driver

Tests in `crates/ddgpu/tests/targets.rs`, all against the host's D3D9
driver (RTX 4060, 610.62) and the wgpu backend on the same machine
(Vulkan, D24_UNORM_S8_UINT, which wgpu-hal picks when the driver offers
it):

| Test | What | Result |
|---|---|---|
| `target_formats_match_the_d3d9_driver` (`TARGETS_FUZZ`, default 200) | A8R8G8B8, A16B16G16R16F, A2B10G10R10, R32F, G16R16, A32B32G32R32F, R16F, G16R16F, G32R32F, A16B16G16R16. Random values in and out of [0, 1] (negative, beyond half range, denormal), blend factors, ops, separate alpha, blend factor, write masks and sRGB write; the target's bits compared | 1000 of 1000 equal, apart from the fp16 overflow below |
| `mrt_draws_match_the_d3d9_driver` | 2-4 targets, mixed formats (the device has MRTINDEPENDENTBITDEPTHS), per-target write masks, blending | 40 of 40 equal |
| `depth_precision_matches_the_d3d9_driver` | D24S8: a draw at z1, a second at z1 + k·2^-24 (k from -1 to 2 in quarter steps) with LESS/EQUAL/LESSEQUAL/GREATER, at five depths | 180 of 180 equal (24-bit quantisation identical) |
| `clears_match_the_d3d9_driver` | A full clear, then a clear with a random viewport, rectangles, scissor (test on or off), sRGB write and flags; colour, depth and stencil checked with a probe draw | 60 of 60 equal |

Measured and implemented:
- **Blending into every target format.** The recorded profile answers
  POSTPIXELSHADER_BLENDING for every render-target format, R32F and
  A32B32G32R32F included, and the driver blends them. ddgpu blended only
  8-bit and fp16 targets. Blending into 32-bit float targets needs wgpu's
  FLOAT32_BLENDABLE (requested; a draw reports an error without it).
- **G16R16 and A16B16G16R16 targets.** wgpu allows Rg16Unorm/Rgba16Unorm
  as render attachments only with TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
  (requested). A format the adapter cannot render to is now an error,
  where it used to panic. `WgpuBackend::optional_features()` lists what
  hosts that create their own device should request.
- **Clear values.** The driver stores `byte * (1/255 as f32)` in single
  precision in float targets: one ulp above k/255 for some bytes (233
  gives 0.91372555, not 0.91372549). For 16-bit float targets it truncates
  that value to half: 106 gives 0.415527, round-to-nearest would give
  0.415771. Unorm targets get k/255, rounded alike by both.
- **BLENDFACTOR** is converted the same way (single precision), which
  matters on 32-bit float targets.
- **Clear regions** (`explore_clear_states`, then the comparison test):
  Clear covers the viewport, each rectangle within it, and only the
  scissor rectangle when the scissor test is on. It encodes the colour to
  sRGB when SRGBWRITEENABLE is on and ignores colour write masks.
  `ClearCall` gains `scissor` and `srgb`; the runtime fills them
  (`crates/ddruntime/src/d3d9draw.rs`). ddgpu clears whole targets with a
  load-op clear and partial regions with a scissored clear pipeline. The
  game's clear command runs after the pass's viewport and raster state are
  set (draw-command case 4 in `FUN_00DAB480`), so these rules apply to
  it. Before this, ddgpu always cleared the whole target and reported an
  error for rectangles.
- **Already equal, now covered by tests:** float value storage and
  denormals; unorm clamping; sRGB write (A8R8G8B8 only, ignored for
  others, as the profile answers SRGBWRITE only for A8R8G8B8/X8R8G8B8);
  per-target write masks in MRT; D24 depth precision.
- `tests/cache.rs` (claude-9's guard for the L25 caches) now also uses a
  second target of a varying format and partial clears; long-lived and
  fresh backends still give the same frames.

### 4. What cannot be exact

- **Blend results beyond the half range in 16-bit float targets** become
  ±inf on the D3D9 driver and ±65504 on the Vulkan driver's blend unit.
  Seen in 3 of 400 random cases, all with blending, for example 100000 +
  dst×100000 into G16R16F, or alpha 65504 with INVSRCALPHA. Direct writes
  of out-of-range values agree, so this is fixed-function blending, which
  a shader cannot reach. The fuzz accepts exactly this pair (inf against
  the largest finite half of the same sign, with blending on) and nothing
  else.
- **Render-target formats the game never renders to** (L8, A8, L16,
  R5G6B5, X1R5G5B5) have no target mapping in ddgpu; a draw into one
  reports an error. The driver offers them as targets.
- **Multisampled targets** (the antialiasing option: CreateRenderTarget
  with the quality from 0x01440450, resolved with StretchRect) are not
  modelled. ddgpu renders them single-sampled, so edges are not
  antialiased. This is outside this brief's format scope; a later brief
  should take it (CheckDeviceMultiSampleType answers, wgpu MSAA targets,
  and resolve semantics against the driver).
- **Test stability:** one run of `cargo test -p ddgpu --test draw` with
  parallel test threads died with an access violation. It did not happen
  again in three parallel runs or a single-threaded run. Every test
  creates its own D3D9 device and wgpu device; the cause is not known
  (driver, threads or memory).

### 5. Commits and verification

Commits on `task/L29-targets` (based on main 755e374, with claude-9's L25
ddgpu work merged in at decdcd7, so L25 merges first or together):
- dd1126b, 52bf276, c906902: census;
- 836101d: oracle read-back and the target tests;
- af4208b: backend (target formats, blending, clears), `ClearCall`
  scissor/srgb, runtime fill;
- 1aaf5cf: cache test;
- 058d863: Results, RUNTIME.md, runtime test.

**Verified on this branch** (light crates): `cargo test -p ddgpu` (draw
12/12, targets 5/5 with `TARGETS_FUZZ=1000`, cache, present), all
compared against the host's D3D9 driver; ddgpu examples build.

**Not verified (for the integration build):** the ddruntime side: the
`ClearCall` fill in `crates/ddruntime/src/d3d9draw.rs`, and the new test
`a_clear_carries_the_viewport_scissor_and_srgb_write` in
`crates/ddruntime/tests/d3d9_device.rs`. Under the batched-verification
rule they were not built here. No private data to adopt.

### 6. MSAA census (groundwork for a later brief; Ghidra only)

- **Option.** "AntiAlias" (ANTIALIAS_NONE / MSAA2X / MSAA4X / MSAA8X,
  strings 0x01428F44..). It is a property of sRender registered in its
  ctor (0x00DB1FE9, setter `FUN_00DA7D20`) and stored at sRender+0xEC. The
  setter accepts a mode only if it is in the available-mode list at
  +0x43CD70, then sets flag 0x80 in +0x433E54 so the targets are rebuilt.
  "AltAntiAlias" (ANTIALIAS_ALT_NONE / FXAA / FXAA3 / FXAA3HQ) is a
  shader post-process, not a target property.
- **Modes.** Nine modes. The table at 0x01440450 holds the
  (D3DMULTISAMPLE_TYPE, quality) pairs:

  | Mode | Type, quality |
  |---|---|
  | 0 | NONE |
  | 1 | 2 samples, 0 |
  | 2 | 4, 0 |
  | 3 | 8, 0 |
  | 4 | 4, 2 |
  | 5 | 8, 0 |
  | 6 | 4, 4 |
  | 7 | 8, 2 |
  | 8 | 8, 4 |

  Modes 4-8 are NVIDIA's CSAA qualities. The table at 0x01442EF0 maps
  mode k to texture flags `k << 8` and a support bit.
- **Support.** `FUN_011487C0(mode, format)` keeps a mode only if
  `FUN_00DA9EC0` found it for the format:
  - modes 0-3 against the CheckDeviceMultiSampleType bits at sRender+0x43CBF2
    (A16B16G16R16F), +0x43CBF4 (A8R8G8B8), +0x43CBF6 (A2B10G10R10) and
    +0x43CC08 (MT 0x26);
  - modes 4-8 against the CSAA bits (+0x43CBF8/0x43CBFA/0x43CBFC/0x43CC0A),
    filled only on NVIDIA (vendor 0x10DE) by comparing the returned
    quality levels.

  D24S8 is asked as well (as the depth match).
- **Targets.** In `FUN_011488C0` (nDraw::Scene) the MSAA flags go on:
  - the scene colour target (param_1[0x9C], A16B16G16R16F in HDR mode,
    else A8R8G8B8);
  - the scene depth (param_1[0x97], D24S8);
  - "ZPrepass Dummy".

  No other target. A texture with MSAA flags is kind 8: `FUN_01105240`
  creates no texture for it, and its views create surfaces with
  CreateRenderTarget / CreateDepthStencilSurface at the table's type and
  quality (`FUN_011042B0`, `nDraw::DepthStencilView::ctor`).
- **Resolve.** Draw-command case 5 in `FUN_00DAB480` resolves:
  - **Colour:** StretchRect(bound render-target slot k, source rect, a
    texture level's surface, dest rect, D3DTEXF_NONE).
  - **Depth:** for slot 4 and up, outside the scene (EndScene, StretchRect
    of the depth-stencil surface to another depth-stencil surface, NULL
    rects, BeginScene), as D3D9 requires for depth-stencil StretchRect.
- **What a later brief needs:**
  - the driver profile's multisample answers for these formats (recorded
    already: CheckDeviceMultiSampleType, `crates/ddruntime/data`);
  - wgpu multisampled targets (sample counts 2/4/8; CSAA qualities have no
    wgpu equivalent, so they would be reported or mapped to the sample
    count);
  - StretchRect from a multisampled surface as a resolve (D3D9's resolve
    filter against the driver);
  - depth-stencil StretchRect.

  The runtime already creates the surfaces with their multisample type
  (`d3d9res.rs`); the backend renders them single-sampled.
