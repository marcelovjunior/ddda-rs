# Native renderer

Goal (2026-10-13): gameplay at 100+ fps first, 240+ fps as the target,
and the same image as the original.

## Where we are

Today the game renders through our Direct3D 9 translation:

```
game (lifted, 1:1) -> nDraw (lifted) -> IDirect3DDevice9 calls -> ddruntime d3d9* -> d3d9cmd DrawCall -> ddgpu (wgpu, Vulkan)
```

Gameplay draws ~3,500-5,200 times a frame from one render thread
(`guest thread 0x1034`). Each draw is flattened by the engine into D3D9
state calls, and the translation derives a wgpu pipeline, bind groups and
constant blocks from them again. That re-derivation is the cost:

| Build | Gameplay fps | Note |
|---|---|---|
| auto5 (2026-10-13 01:30) | 4.7 | |
| combo2 (L89 worker, L94, L97, L99, L102) | 8.9 | shards at opt-level 0 |
| o1b (all shards opt-level 1, L104) | 15.4-17.7 | fewer draws in that scene |

DXVK runs the original exe well above 100 fps on the same machine, so a
translation can be fast. The native renderer goes further: it hooks above
D3D9, where the engine still holds immutable state objects.

## What the engine gives us (MT Framework 2, nDraw)

The PC build's renderer is MT Framework's `nDraw` layer, designed around
D3D10/11-style immutable objects (names from the DTI registry):

- `nDraw::BlendState`, `RasterizerState`, `DepthStencilState`,
  `SamplerState`, `InputLayout`
- `nDraw::RenderTargetView`, `DepthStencilView`, `Texture`,
  `VertexBuffer`, `IndexBuffer`, `Buffer`
- materials: `nDraw::Material`, `MaterialStd` (+DM/PN/Est/Cld),
  `MaterialSSS`, `DDMaterialStd`, `DDMaterialInner`, `DDMaterialWater`,
  `FWMaterialStd`
- `nDraw::Scene`, `nDraw::CommandCache` (command lists), `sRender`,
  `sShader2`, `rShaderPackage`, `rShaderCache`
- post-processing plugs: `BloomPlug`, `ToneMapPlug`, `ShadowPlug`,
  `BokehPlug`, `ColorCorrectPlug`, `ColorFogPlug`, `ImagePlanePlug`,
  `TVNoisePlug`, `FilterBase`, `LightBase`
- instancing: `cInstancing`, `cInstancingCulling`, `cInstancingDirect`,
  `cInstancingDynamic`, `cInstancingFromNulls`,
  `cInstancingFromMatrices`, `cSCGPlusInstancing`

State objects that never change map one to one onto wgpu pipeline state:
a pipeline is built once per (shaders, input layout, blend, rasterizer,
depth-stencil, target formats) and found by pointer afterwards, with no
per-draw re-derivation.

## Plan

1. **Map nDraw** (NR-A): the functions between nDraw objects and the
   D3D9 device calls; the layouts of the state objects; how a draw is
   issued (CommandCache); how sRender orders a frame (scene passes,
   plugs). Output: docs/NDRAW.md with addresses, layouts and hook
   candidates.
2. **Measure a frame** (NR-B): a gameplay capture through
   `crates/d3d9frame` and the frame breakdown (L107): passes, draws per
   pass, redundancy, batching potential.
3. **Native device** (NR-C): a ddgpu API driven by state-object handles
   (pipeline cache keyed by handle tuples, persistent bind groups, a
   command recorder costing < 2 us per draw).
4. **Hook nDraw** through the override system (L60, `ddports`): the
   device-level nDraw functions call the native device instead of D3D9.
   The D3D9 path stays: as the fallback for anything not taken over yet,
   and as the oracle (the frame oracle compares native and D3D9 images
   frame by frame).
5. **Take over pass by pass**, each verified pixel by pixel: likely the
   model/shadow passes first, then post-processing plugs, UI last.
6. **Then** batch and instance across draws (the engine already has
   instancing classes), and move recording off the game's render thread.

## Loading

Also in scope: boot to the main menu (logos, disclaimer, press any
button) and loading a save. First measure where wall time goes (our
start-up, shader/pipeline creation, file I/O, archive inflate in lifted
code, fixed waits of the original), then cut our own costs: an archive
inflate port (`rArchive::DecompressStream`), parallel and asynchronous
reads, caches. Waits the original itself makes stay as they are unless
the user chooses otherwise.

## Tools for agents

- Ghidra over HTTP: `curl "http://127.0.0.1:8089/decompile_function?address=0x00db8bf0"`,
  `/get_xrefs_to?address=`, `/disassemble_function?address=`,
  `/get_function_callers?address=`, `/search_strings?search_term=`
  (`/mcp/schema` lists everything). Read-only: never rename, retype or
  save in the shared project.
- The DTI registry: `E:/ddda-re/notes/dti-registry.json`,
  `dti-labels.csv`.
- Prior art: `crates/ddview` (an M1 viewer that renders stage cells from
  the game's own files: mtmod, mttex, mtmrl, mtshader,
  `dd_material.wgsl`).
