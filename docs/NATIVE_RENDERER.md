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
| combo3 (+L95 L96 L98 L105 L106, opt-level 1) | 8.0 / 9.3-10 | heavy scene, ~5,250 draws; bound: backend worker |
| combo4 (+L90 compact constants, L94b, L100) | 10.9 / 12.3-12.7 | bound: render thread again |
| combo5 (+L112 executor port, L118 step 1) | 12.3 / 12.7-13.7 | ~5,500 draws |
| combo6b (+L120 async occlusion, L116 warm pipelines, L109 inflate port) | 12.8 / 13.7-14.0 | gameplay reached at 86 s (was 105-112) |
| combo8 (+L118 step 2, L124 finisher thread) | 10.9 / 13.0-13.7 | backend worker 47% + finisher 47%: the bound is the game's render thread again |
| combo9 (+L101 cheaper D3D9 calls) | 13.7 / 13.7-14.7 | render thread 88%: snapshot 26%, draw calls 20%, flush_reads 16% |
| combo11 (+L121 copies in the open encoder; autoplay presses Enter early) | 14.1 / 15.3 | gameplay reached at 59 s |
| combo12 (+L118 step 3) | 14.7 / 15.3-16.7 | render thread 89% (snapshot 24%, draw calls 31%) |

(average / end of the run; L114, inlining the memory and flag helpers into
the shards, gave no gain for 2.5x the compile time and was dropped.)

An nDraw-level count (L111, run instr1): a gameplay frame issues ~5,200
commands but uses only 67 distinct state objects, 222 vertex and 183 pixel
shaders, 52 input layouts and 23 samplers. A pipeline cache keyed by
object identity has a tiny working set.

The wgpu floor (L110, 5,000 draws, everything pre-created, CPU per draw
including finish and submit): wgpu 0.88 us, wgpu render bundles 0.39 us,
raw Vulkan (ash) 0.33 us; a render pass costs ~7 us in wgpu and ~1 us in
ash. So: native device on wgpu first (100+ fps once the ~900 tiny
simulation passes are gone, L113), raw Vulkan for the hot path later
(240+).

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

## A gameplay frame (measured)

Capture `E:/ddda-re/captures/cap1.ddframe` (combo3, frames 7535-7555),
analysed with `d3d9frame`'s `frame_stats` (L107):

- ~5,350 draws in **~977 render passes** per frame.
- ~900 of those passes are one-draw passes on tiny `64x8` / `64x16`
  `A32B32G32R32F` targets, in loops of 38 or 70 iterations (a two-target
  pass, then single-target ping-pong passes): an iterative GPU
  simulation, not scene drawing (which system: open, L108/L111).
- The scene itself: ~3,730 draws in ~15 passes on the `1920x1080`
  colour target with `D24S8` depth; plus shadow/reflection targets
  (`128`-`384`), full mip chains down to `1x1` (luminance), and
  `8x8`/`32x32` half-float targets (tone mapping, bloom).
- How often each part of a draw's state differs from the previous draw:
  shaders 38%, declaration 29%, render states 5%, textures/samplers 55%,
  vertex streams 60%, index buffer 22%, VS/PS constants 34%, targets 18%.
- Shader constants that actually change per frame: ~250 KB (8,200 VS and
  7,300 PS float4 registers); the translation uploads ~9 KB per draw
  (~48 MB per frame).

Consequences for the design: render-pass churn has to go (the
simulation loop as compute or one batched pass); constants must be
uploaded as deltas; pipelines must be keyed by state-object identity
(render states rarely change, shaders and textures often).

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
