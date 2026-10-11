# L20: Direct3D 9 completeness (device methods, fixed function, queries, state coverage)

- **Owner:** claude-5
- **Branch:** `task/L20-d3d9-completeness` (worktree `E:\ddda-rs-wt\L20`)
- **Status:** in progress (claude-5)

## Why

The D3D9 layer (`crates/ddruntime/src/d3d9*.rs`, the backend command crate
`crates/d3d9cmd`, the wgpu backend `crates/ddgpu`, the driver oracle
`crates/d3d9ref`, the shader translator `crates/d3d9wgsl`) renders shader
draws pixel-exact against the host's real D3D9 driver. Not done yet:

- IDirect3DDevice9 slots without an implementation (calls stop with the
  method name): 10 SetCursorProperties, 11 SetCursorPosition, 12 ShowCursor,
  16 Reset, 19 GetRasterStatus, 20 SetDialogBoxMode, 21/22 Set/GetGammaRamp,
  33 GetFrontBufferData, 46 MultiplyTransform, 49-54 materials and lights,
  55/56 clip planes, 59-61 state blocks, 62/63 clip status,
  70 ValidateDevice, 71-74 palettes, 77/78 software vertex processing,
  79/80 N-patch mode, 85 ProcessVertices, 115-117 patches.
- Draws with no vertex and/or pixel shader bound (the fixed-function
  pipeline: FVF, transforms, lighting, texture stage states).
- Occlusion queries: `d3d9copy.rs` answers event/timestamp queries; an
  occlusion query's GetData stops ("not measured yet").
- Render-state coverage against the driver: only clears, constant-colour,
  textured and simple blended draws are compared so far.

## Task

1. **Census first** (into Results, with addresses): which of the missing
   slots the game calls, whether it draws without shaders (SetFVF,
   SetVertexShader(NULL)/SetPixelShader(NULL) before draws), whether it uses
   occlusion queries, and which render/sampler states and formats it sets.
   The device is reached through the MT Framework renderer's wrappers. Use
   Ghidra over HTTP (`curl "http://127.0.0.1:8089/decompile_function?address=0x..."`,
   `/disassemble_function`, `/get_xrefs_to`) and the lifted code. A note:
   a naive scan for `call [reg+slot*4]` finds nothing, because the vtable
   load is far from the call. Work from the wrappers.
2. **Implement what the game uses, in the order it is reached**, each with
   D3D9's documented semantics and a test:
   - Device methods in `crates/ddruntime/src` (new module(s) next to
     `d3d9state.rs`/`d3d9copy.rs`; register slots in `d3d9dev.rs`
     `INTERFACES`).
   - State blocks: while recording (Begin/EndStateBlock), Set* calls go
     into the block and do not change the device (Wine's
     `update_stateblock_state` model).
   - Reset: resources in D3DPOOL_DEFAULT must be released first, otherwise
     D3DERR_INVALIDCALL; the swap chain and back buffer are rebuilt with
     the new present parameters.
   - Gamma ramp: feed it to the backend's present.
   - Methods the game provably never calls stay unimplemented. List them in
     Results with the evidence.
3. **Fixed function** (only if the census shows the game uses it):
   generate WGSL from the fixed-function state, in `d3d9wgsl` or `ddgpu`,
   and check it against the driver through `d3d9ref` like the shader draws.
4. **Occlusion queries** in `ddgpu` (wgpu occlusion query sets resolved
   when the guest asks; the count semantics against the driver), if the
   game uses them.
5. **State coverage against the driver**: extend `ddgpu/tests/draw.rs` and
   the fuzz (`DDGPU_FUZZ`) so every render state, sampler state and texture
   format the census lists is exercised and compared with `d3d9ref`:
   stencil, depth bias, colour write masks, blend ops and separate alpha,
   cull and fill modes, scissor, sRGB read/write, and the formats. Fix
   what differs. Record anything that cannot match exactly and why.

## Rules

- Read first: `AGENTS.md`, and `docs/RUNTIME.md` (implementation status,
  "Graphics backend design", findings).
- Never invent behaviour: the D3D9 docs, the SDK headers and the driver
  oracle decide. Cite them in Results.
- Lifted game code is never replaced. No special cases by address.
- The game install is read-only.
- Builds and memory (the machine runs five agents):
  - Always `CARGO_INCREMENTAL=0`, and put your target dir on C:
    (`CARGO_TARGET_DIR=C:/ddda-targets/L20`).
  - `ddgpu`, `d3d9wgsl`, `d3d9ref` and `d3d9cmd` are light: use
    `CARGO_BUILD_JOBS=4`.
  - Anything that builds `ddruntime` (it pulls the huge `ddnative` crate)
    uses `CARGO_BUILD_JOBS=2`, and only when at least 8 GB of RAM is free.
  - Batch your `ddruntime` edits, so it rebuilds rarely.
- Another agent (claude-2, L17A) edits `crates/ddruntime/src` for Windows
  services (kernel32/user32 and friends). Keep your changes to the D3D9
  files and small registrations, to avoid conflicts.
- Keep your prompt cache warm: never sit inside one tool call for more than
  a few minutes; run long jobs in the background with output to a file and
  poll it.
- Do not spawn subagents.
- Commits on your branch only, as
  `git -c user.name=janne -c user.email=marcelovjunior55@gmail.com commit`,
  with no AI co-author trailer. Never push. Never commit game data, anything
  from `E:\ddda-re`, or decompiler output.
- Fill in Results, update the `docs/RUNTIME.md` rows, and commit. End your
  run with a short summary for the head.

## Results

### 1. Census (2026-10-11, Ghidra 12.1.4 over HTTP and a capstone scan of the unpacked exe)

**How the device is reached.** `IDirect3D9::CreateDevice` (slot 16, call at
0x00DB1960 in `FUN_00DB1480`) writes the device to `sRender+0x98`
(`sRender` is the global at 0x018D08F4). The only other copy is the draw
context's `+4`, written at 0x00DA6DBE (`FUN_00DA6DA0`, which also sets the
per-frame alpha-test and stencil defaults). Calls go
`mov r,[dev]; mov r2,[r+slot*4]; push dev; call r2`, so the scan follows
values through registers and stack slots: the device loaded from
`[sRender+0x98]` or `[ctx+4]`, its vtable, a method pointer at
`[vtable+slot*4]` and an indirect call with the device pushed last (this).
It found 213 call sites for 46 distinct methods in the renderer
(0x00DA0000-0x00DC0000), the shader loader (0x00E0F000) and the resource
classes (0x01103000-0x01149000). A scan for stores of the device pointer
found no other copy in the renderer. The scan scripts and their raw output
are private (`E:\ddda-re\notes\L20`).

**Device methods the game calls** (call sites):
Release 2 (0x00DAA539, 0x00DB0BD9);
TestCooperativeLevel 9; GetAvailableTextureMem 0x00DB1976; GetDirect3D
0x00DAA4F5; GetDeviceCaps 0x00DAA507/0x00DAA528; CreateAdditionalSwapChain
0x00DAFC05, 0x00DB19D8, 0x01147B6A; **Reset (16) 0x00DAF9FF, 0x00DAFA3B,
0x00DAFA90, 0x00DAFAD4** (`FUN_00DAF7C0`, the mode-change path: window
size, fullscreen toggle and display options); Present 4; GetBackBuffer
0x00DAE8A4; **SetGammaRamp (21) 0x00DA6D8B** (`FUN_00DA6D00`: swap chain
0, flags 0, a 256-entry ramp from the brightness option; called from
0x00DAE584 and 0x00DA9D26, guarded by the renderer byte `+0x43CBED`);
CreateTexture/VolumeTexture/CubeTexture, CreateVertexBuffer/IndexBuffer,
CreateRenderTarget, CreateDepthStencilSurface, GetRenderTargetData
(0x01103EAC), StretchRect (0x00DABF28, 0x00DABF79, D3DTEXF_NONE),
SetRenderTarget 5, SetDepthStencilSurface 2, BeginScene/EndScene, Clear
(0x00DABD00, 0x00DAE5E2: count 0, no rectangles), SetViewport
(0x00DA7172: MinZ 0, MaxZ 1 always), SetRenderState 80, SetTexture 9
(including **sampler 256, D3DDMAPSAMPLER**, and 257..260 in the reset
`FUN_00DA6EA0`), SetSamplerState 22, SetScissorRect, DrawPrimitive
(0x00DAB928, 0x00DABD8A), DrawIndexedPrimitive (0x00DAB69C, 0x00DAB78F,
0x00DABA9C), CreateVertexDeclaration, SetVertexDeclaration,
CreateVertex/PixelShader, SetVertex/PixelShader, Set{Vertex,Pixel}ShaderConstantF/I/B,
SetStreamSource, **SetStreamSourceFreq** (0x00DAB88F, 0x00DAB94A,
0x00DAB9F5, 0x00DABABA: hardware instancing, D3DSTREAMSOURCE_INDEXEDDATA
`| count` and D3DSTREAMSOURCE_INSTANCEDATA `| 1`), SetIndices, CreateQuery.
All of these were already implemented except Reset and SetGammaRamp.

**Missing slots the game never calls**, by the census above: 10-12
cursor methods (the game uses USER32 `ShowCursor`/`SetCursorPos`), 19
GetRasterStatus, 20 SetDialogBoxMode, 22 GetGammaRamp, 33
GetFrontBufferData, 46 MultiplyTransform, 49-54 materials and lights,
55/56 clip planes, 59-61 state blocks (no D3DX effect framework is
imported: the D3DX9_43 imports are only GetShaderConstantTableEx,
GetShaderOutputSemantics and SaveTextureToFileInMemory), 62/63 clip status,
70 ValidateDevice, 71-74 palettes, 77/78 software vertex processing, 79/80
N-patches, 85 ProcessVertices, 115-117 patches. Also never called: SetFVF
(89), DrawPrimitiveUP/DrawIndexedPrimitiveUP (83/84), SetTransform.
These stay unimplemented (a call stops with the method name). Limits of
the evidence: a call through a copy of the device the scan cannot follow
(for example passed as an argument to a helper outside these ranges) would
be missed. No such copy was found, and a call to an unimplemented slot
stops with the method name, so a miss would show up at once.

**Fixed function: not used.** Every draw goes through the command
executor `FUN_00DAB480` (cases 0-4: draw, indexed draw, both instanced,
and a clear-or-draw), which first binds the pass's declaration (slot 87)
and both shaders (slots 92/107) from the MT Framework shader object in
`FUN_00DAA8B0`. SetFVF is never called. `SetVertexShader(NULL)` and
`SetPixelShader(NULL)` only occur in the end-of-pass reset `FUN_00DA6EA0`
before the next pass binds again. All 29,198 shaders in the packages are
vs_3_0/ps_3_0, and D3D9 does not combine vs_3_0 with fixed-function pixel
processing. Step 3 is not needed.

**Occlusion queries: used.** `nDraw::Query` (ctor 0x01148093,
`vf07` 0x01148130) creates a ring of four D3DQUERYTYPE_OCCLUSION queries;
draw-command case 7 issues `D3DISSUE_BEGIN` (`FUN_011481B0`) and
`D3DISSUE_END` (`FUN_011481F0`) around draws, and `FUN_01148290` reads them
with `GetData(&count, GetDataSize(), 0)`. The caller `FUN_00E271A0` compares
the **sample count** with a threshold (`param_2+0xECC`), so the counts must
be exact, not just visible/hidden.

**Render states set** (`FUN_00DA6DA0`, `FUN_00DA7510` blend,
`FUN_00DA77D0` depth-stencil, `FUN_00DA7B30` raster, `FUN_00DAA5F0`,
`FUN_00DAA8B0`): ZENABLE 7, FILLMODE 8, ZWRITEENABLE 14, ALPHATESTENABLE
15, SRCBLEND 19, DESTBLEND 20, CULLMODE 22, ZFUNC 23, ALPHAREF 24,
ALPHAFUNC 25, ALPHABLENDENABLE 27, STENCILENABLE 52, STENCILFAIL 53,
STENCILZFAIL 54, STENCILPASS 55, STENCILFUNC 56, STENCILREF 57,
STENCILMASK 58, STENCILWRITEMASK 59, CLIPPING 136, MULTISAMPLEANTIALIAS
161, COLORWRITEENABLE 168, BLENDOP 171, SCISSORTESTENABLE 174,
SLOPESCALEDEPTHBIAS 175, ANTIALIASEDLINEENABLE 176, TWOSIDEDSTENCILMODE
185, CCW_STENCILFAIL/ZFAIL/PASS/FUNC 186-189, COLORWRITEENABLE1-3
190-192, BLENDFACTOR 193, DEPTHBIAS 195, SEPARATEALPHABLENDENABLE 206,
SRCBLENDALPHA 207, DESTBLENDALPHA 208, BLENDOPALPHA 209. SRGBWRITEENABLE
(194) is never set (it stays FALSE).

**Sampler states set** (`FUN_00DA7250`, `FUN_00DAA8B0`): ADDRESSU/V/W 1-3
(3-bit fields: WRAP, MIRROR, CLAMP, BORDER, MIRRORONCE), BORDERCOLOR 4,
MAGFILTER 5, MINFILTER 6, MIPFILTER 7, MIPMAPLODBIAS 8, MAXMIPLEVEL 9,
MAXANISOTROPY 10 (anisotropic filtering from the texture-filter option:
MINFILTER 3 becomes linear or anisotropic 2..16), SRGBTEXTURE 11. Pixel
samplers 0-15 and vertex samplers 257-260.

**Texture formats.** The renderer maps MT texture format codes to D3DFMT
through a 47-entry table at 0x01440698 (indexed in `FUN_01104F40`):
A8R8G8B8 21, X8R8G8B8 22, R5G6B5 23, A1R5G5B5 25, A4R4G4B4 26, A8 28,
A2B10G10R10 31, A8B8G8R8 32, G16R16 34, A16B16G16R16 36, L8 50, Q8W8V8U8
63, V16U16 64, D24S8 75, L16 81, Q16W16V16U16 110, R16F 111, G16R16F 112,
A16B16G16R16F 113, R32F 114, G32R32F 115, A32B32G32R32F 116, DXT1, DXT3,
DXT5. Primitive types: point, line list, line strip, triangle list,
triangle strip (no fans).

### 2. Device methods (`crates/ddruntime/src/d3d9dev.rs`, `d3d9copy.rs`, `d3d9state.rs`)

- **Reset (16).** Invalid present parameters fail with D3DERR_INVALIDCALL
  and touch nothing (Wine `d3d9_device_reset` validates first). Otherwise
  every binding is released and the state goes back to a fresh device's
  (Wine `state_unbind_resources`, Microsoft Learn IDirect3DDevice9::Reset:
  "all state information to be lost"). D3DPOOL_DEFAULT textures,
  buffers, standalone surfaces and additional swap chains that are still
  alive fail the call. So do the implicit swap chain, its back buffers and
  the automatic depth-stencil surface while the application holds a
  reference (Wine `reset_enum_callback`; Learn: release explicit render
  targets, depth-stencil surfaces, additional swap chains and DEFAULT
  resources). A failed Reset leaves the device waiting for another:
  TestCooperativeLevel returns D3DERR_DEVICENOTRESET and Present returns
  D3DERR_DEVICELOST (Wine `D3D9_DEVICE_STATE_NOT_RESET`). On success the
  completed parameters are written back, the old back buffers and
  automatic depth-stencil surface are freed, and new ones are bound with
  a fresh state: viewport and scissor at the new size.
- **Ownership fixes found on the way.** The device now owns the automatic
  depth-stencil surface (one reference besides the binding), so
  SetDepthStencilSurface(another) no longer frees it. Additional swap
  chains are held by the application only, so their last Release removes
  them and GetNumberOfSwapChains drops.
- **SetGammaRamp (21).** The ramp is kept with its swap chain. Present
  hands it to the backend (`GraphicsBackend::gamma`) when the chain is
  full-screen: the recorded caps have D3DCAPS2_FULLSCREENGAMMA and no
  CANCALIBRATEGAMMA, and Learn says an unsupported mode ignores the ramp.
  ddgpu maps each channel of the presented frame through the ramp's
  high byte. GetGammaRamp (22) stays unimplemented: the game never calls
  it.
- **SetTexture(256)**, D3DDMAPSAMPLER, is accepted (`FUN_00DA6EA0` clears
  it). Its sampler states start like the vertex samplers'.
- **Freed objects** (surfaces, textures, buffers, queries) are reported to
  the backend (`GraphicsBackend::release`). The guest heap reuses 16-byte
  object blocks, and ddgpu kept textures and targets by address, so a
  surface created after a Reset could pick up a freed one's texture.
- **Occlusion queries.** Issue(BEGIN/END) are passed to the backend.
  GetData follows the driver (measured, ddgpu
  `occlusion_query_states_on_the_d3d9_driver`): S_OK with 0xDDDDDDDD
  written before any Issue, S_FALSE between BEGIN and END, and S_OK with
  the sample count after END. The recording backend counts nothing and
  answers S_FALSE. An END without a BEGIN counts nothing (Wine's model;
  not measured, and the game always pairs them).
- Tests: `crates/ddruntime/tests/d3d9_device.rs` (new file, so it doesn't
  collide with L17A in `tests/runtime.rs`).

### 3. Fixed function

Not needed: the game never draws without shaders (census). Nothing was
added.

### 4. Occlusion queries and instancing (`crates/ddgpu`, `crates/d3d9cmd`, `crates/d3d9ref`)

- New backend commands `query_begin`, `query_end`, `query_result`. ddgpu
  gives every draw made while queries count its own slot in a wgpu
  occlusion query set (256 slots per set, freed when a result is read or
  a query is begun again or released), resolves a query's slots when the
  guest asks, and sums them.
- wgpu's own counts are not guaranteed exact: its DX12 backend uses
  BINARY_OCCLUSION, and its Vulkan backend begins queries without the
  PRECISE flag. On this machine (RTX 4060, Vulkan, driver 610.62) the
  counts equal the D3D9 driver's for every case tried: depth pass and
  fail, depth off, alpha-test kill, culling, scissor, and an empty query
  (`occlusion_counts_match_the_d3d9_driver`). Overlapping queries are
  not compared. The driver does not count them independently (a query
  spanning six draws while others ran counted only the last draw), and
  the game issues one at a time (`FUN_011481B0`/`FUN_011481F0`).
- **Instancing** (census: the game draws instanced with
  SetStreamSourceFreq). ddgpu now draws INDEXEDDATA|n indexed draws n
  times with INSTANCEDATA|1 streams stepping per instance. The base
  vertex does not offset instance data, as on the driver. Measured on the
  driver (`instanced_draws_match_the_d3d9_driver`): a non-indexed draw
  ignores the count and draws once, but its instance-data streams still
  give every vertex instance 0's element (the game's draw command 2 does
  this). All three cases equal the driver.
- d3d9ref replay gained `render_counted` (occlusion queries around
  draws), `occlusion_states`, and stream frequencies. It now creates
  textures before any state, so a refused format leaves the device
  unchanged.

### 5. State coverage against the driver

`render_states_match_the_d3d9_driver` (`DDGPU_FUZZ` cases, default 150)
randomises one group per case on a neutral baseline:
- texture formats and sampler states (address modes incl. border, border
  colours, filters, mip filters, LOD bias, MAXMIPLEVEL, anisotropy, sRGB
  read);
- blend (ops, separate alpha, blend factor, write masks);
- alpha test, cull and scissor;
- depth (function, writes, bias, slope-scaled bias);
- stencil (all functions and ops, masks, two-sided, depth-fail paths);
- sRGB write with blending.

Each case draws a textured triangle over a D24S8 target, then a probe
draw whose depth and stencil tests show what the first draw left in that
target. **Result: 982 of 982 compared cases equal the driver within one
step per channel** (`DDGPU_FUZZ=1000`; the driver refuses
A8B8G8R8/X8B8G8R8/Q16W16V16U16 as textures). The older
`random_states_and_formats_match_the_d3d9_driver` and the other draw
tests still pass. `alpha_rounding_and_fill_modes_match_the_d3d9_driver`
asserts the alpha-test rounding and point/solid fill.

Fixed from the comparisons:
- the depth-stencil surface was only attached with ZENABLE on, so a
  stencil-only draw had no stencil test;
- two-sided stencil used the clockwise states for both faces;
- MIPFILTER NONE samples level 0 whatever MAXMIPLEVEL says, but the
  driver chooses MAG- or MINFILTER on the level clamped to MAXMIPLEVEL
  (so MAXMIPLEVEL > 0 always minifies), while Vulkan chooses on its own
  clamp (handled with a [0, 1/4] clamp);
- the alpha test compares the alpha rounded to 8 bits (47.6/255 equals
  ALPHAREF 48);
- MIPMAPLODBIAS: wgpu samplers have none, so it is added by the pixel
  shader (d3d9wgsl `lod_bias`);
- DEPTHBIAS: hardware bias steps are implementation-defined (the NVIDIA
  Vulkan driver steps 2^-23 for D24, giving twice D3D9's offset, which
  adds the value itself), so depth + DEPTHBIAS + SLOPESCALEDEPTHBIAS *
  max(|dz/dx|, |dz/dy|) is written as frag_depth (d3d9wgsl `depth_bias`,
  only for draws with a bias);
- border colours (transparent black, opaque black, opaque white;
  ADDRESS_MODE_CLAMP_TO_BORDER requested);
- anisotropy;
- point and wireframe fill (POLYGON_MODE features requested);
- sRGB texture views;
- new formats A2B10G10R10, G16R16, A16B16G16R16, Q8W8V8U8, V16U16, L16,
  Q16W16V16U16, and render targets A2B10G10R10 and A16B16G16R16.

**Cannot match exactly (and why):**
- **Fractional-LOD trilinear weights.** Both NVIDIA drivers apply their
  trilinear optimisation with different weights at the same LOD
  (mip-level blend factor at LOD fraction 0.25/0.5/0.75: D3D9
  0.0625/0.5/0.9375, Vulkan 0.4375/0.875/1). Integer LODs match. Fixing
  this would mean filtering in the shader with one driver's curve.
- **Anisotropic filtering** at MAXANISOTROPY > 1: sample patterns are
  implementation-specific, and wgpu needs linear mips for anisotropy, so
  MIPFILTER NONE/POINT drop to linear filtering.
- **Border colours on formats without all channels.** D3D9 applies the
  border only to the channels the format has (the rest read 1, e.g.
  R5G6B5 alpha, R32F green/blue/alpha). wgpu offers only three border
  colours, so this matches only with the white border. Other border
  colours report an error.
- **D3DTADDRESS_MIRRORONCE**: no wgpu equivalent; reported as an error.
- **sRGB reads of 16-bit packed formats** (R5G6B5, X1R5G5B5, A1R5G5B5,
  A4R4G4B4): D3D9 decodes the 4/5/6-bit value, ddgpu the value widened to
  8 bits. Off by up to 2.
- **Wireframe fill**: D3D9's line rasterisation lit different pixels
  than the Vulkan driver's for the same triangle.
- **Clear and SRGBWRITEENABLE**: D3D9's Clear converts the colour when
  SRGBWRITEENABLE is on (seen in the fuzz); ClearCall does not carry
  render states. The game never sets SRGBWRITEENABLE, so this is not
  modelled.
- **Render-target formats** beyond A8R8G8B8 are not compared (the oracle
  reads 8-bit targets only).
