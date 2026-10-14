# nDraw direct: consuming MT Framework's draw commands without the D3D9 state machine

Design note L195, 2026-10-14 (agent `ndraw-design`). Research and
documentation only: no code was changed, nothing was built or run. Every
number below is quoted from an existing doc or a log with its source; items
marked **ESTIMATE** are this note's own extrapolation, items marked
**UNKNOWN** have no evidence yet.

Reads with: [NDRAW.md](NDRAW.md) (layouts and addresses of the nDraw
objects, sections 2, 3, 5, 9), [NATIVE_RENDERER.md](NATIVE_RENDERER.md)
(history), [tasks/L180-render-worker.md](tasks/L180-render-worker.md) (the
sink's per-draw cost work), [FRAME_PASSES.md](FRAME_PASSES.md) (what a frame
draws), [DECOUPLED_RENDER.md](DECOUPLED_RENDER.md) (why the sink must become
about 4x cheaper), `E:/ddda-rs-wt/L172/docs/tasks/frame-bound.md` (what bounds
the frame today).

## 0. Summary

1. The premise "we still emulate D3D9 per draw" is only half true. Since L119
   the render worker's sink (`crates/ddruntime/src/ndraw_native.rs`) already
   reads the executor's records and nDraw objects directly and skips the
   guest's four apply functions, the D3D9 `SetRenderState` / `SetTexture` /
   `SetVertexShaderConstantF` / `Draw*` calls and the device snapshot, for
   every colour target (`DDDRAW_NATIVE_TARGETS=all`, now standard): 5,364
   draws a frame taken, 0 refused in train 6 (MILESTONES L179).
2. What it still does per draw is **re-create a D3D9 view of the draw**: a
   `d3d9cmd::DrawCall` carrying a sorted `(RS number, value)` list, D3DVERTEXELEMENT9
   bytes, D3D9 shader tokens, a 21-slot x 14-state sampler mirror of the
   emulated device, and constant banks of `[u32;4]` registers. That is the
   remaining cost: `draw_call` 21.7% self, memcpy ~9%, `Mirror::textures`
   4.8%, hash lookups ~5%, malloc 3.3% of the render worker (frame-bound.md
   round 4), plus ddgpu's own per-draw memo compare and constant packing.
3. The MT immutable objects are used only as *cache keys for the translation
   into that D3D9 view* (`NativeStateIdentity` = address + hash at +0xC), not
   as the pipeline state. ddgpu already has the other half of the target
   design as an unused-by-default API: `ddgpu::native` (`Device`, `Recorder`,
   handles `Blend/Rasterizer/DepthStencil/Sampler/InputLayout/Shader/
   Pipeline/TextureSet/ConstantBuffer`, render bundles, raw Vulkan under
   `DDGPU_VK=1`) fed through `DDDRAW_NATIVE=1` `NativeJob`s (L126/L141).
   `NativeDraw` is still D3D9-shaped (render-state list, declaration bytes),
   and that path is not the default.
4. Proposed: a **DirectDraw** layer between the executor record and the
   backend, keyed by MT object identity with generation checks, that never
   builds D3D9 render-state lists, never mirrors the device's sampler/texture
   table, and gathers constants straight from the shader binding's value
   slots into the backend's uniform arena. Staged: (S1) decode side only, one
   pass (the shadow atlas, 489 draws a frame), lowered to the existing
   `DrawCall` so ddgpu is untouched and `verify` can compare field by field;
   (S2) backend side through `ddgpu::native` handles; (S3) all scene passes;
   (S4) clears/copies/queries and the retirement of the device mirror;
   (S5) retained replay and instancing hooks for DECOUPLED_RENDER A2.
5. Honest expectation: the sink's per-draw cost can fall from ~2-4 us to
   ~0.3-0.5 us for a draw whose bindings are unchanged and ~1-1.5 us for one
   that changes a binding (**ESTIMATE**, the L180 bench already shows 0.20-0.30 us
   for a cached draw and 1.31-1.43 us for a frame's first). That is worth
   roughly 1.5-3 ms of the render worker per frame. At ~60 fps the render
   worker is **not** the bound (main thread is, frame-bound.md); the gain
   becomes the frame bound at ~57+ fps and, above all, is the *replay cost*
   of the 240 fps route (DECOUPLED_RENDER.md section 6.3: sink 12.7 ms -> ~2 ms).

## 1. Current state (what exists, what is still D3D9-shaped)

### 1.1 The paths and their switches

Pipeline (NATIVE_RENDERER.md "Where we are", updated):

```
game (lifted) -> sRender::vf06 0x00DAF000 builds {sort_key, record} stream (main thread)
render worker (guest thread 0x1034, 0x00DAC090) -> executor 0x00DAB480 (port: mtdraw::executor)
   -> CommandSink::command(record)            crates/mtdraw/src/sink.rs
        Taken::Handled: ndraw_native decodes record -> DrawCall -> DeferredBackend -> ddgpu
        Taken::Default: lifted applies 0x00DA6FE0/5F0/7C0/8B0 -> D3D9 vtable -> ddruntime d3d9* -> DrawCall
```

| Switch | Effect | Source |
|---|---|---|
| `DDDRAW_NATIVE` unset / `drawcall` (default) | Sink decodes draws into `DrawCall`s; the applies, D3D9 setters and snapshot are skipped for them | ndraw_native.rs:1-98, `mode_of` :131 |
| `DDDRAW_NATIVE=0/off` | Everything through the D3D9 path (the oracle) | :131 |
| `DDDRAW_NATIVE=verify` | Takes nothing; every draw the decoder could take is compared field by field with the DrawCall the D3D9 path then makes | :3607-3719 (`expect`, `compare`, `differences`) |
| `DDDRAW_NATIVE=census` | Counts commands, distinct objects, refusal reasons | :2607 `decide` |
| `DDDRAW_NATIVE=1/native` | Takes the same draws as `NativeJob`s (consecutive same-target draws = pass; passes since the last default command = job) to `ddgpu::native` | :2315 `native_draw`, :2382 `Pending`; ddgpu `native_job.rs` |
| `DDDRAW_NATIVE=compare` | D3D9-shaped DrawCalls draw the real targets; NativeJobs draw shadow copies; GPU reduction compares | ddgpu `native_compare.rs` |
| `DDDRAW_NATIVE_TARGETS=all` | Offer every colour target, not only the largest (scene) one | :121 |
| `DDDRAW_NATIVE_OPS=0` | Leave Clears (type 4) and StretchRects (type 5) to the D3D9 path (default: native) | ndraw_ops.rs:35 |
| `DDGPU_VK=1` | Raw Vulkan pass encoding instead of wgpu encoders (current playtest default: playtest_fast2.bat) | ddgpu vk/mod.rs:75 |

The current playtest build runs `DDDRAW_NATIVE` unset (drawcall),
`DDDRAW_NATIVE_TARGETS=all`, `DDGPU_VK=1` (E:/ddda-re/play/playtest_fast2.bat).
`DDDRAW_NATIVE=1` is not the default: last in-game numbers are combo29 20.6 fps
(vs 17.0) with 6 jobs refused (indexed line strips) and sink refusals per frame
Instanced 90, FloatsUncovered 44, Query 6 (MILESTONES L141 native jobs);
compare mode there: 143,744 passes matched, 0 mismatched.

### 1.2 Which draws bypass D3D9 call emulation, which do not

| Command | Today | Evidence |
|---|---|---|
| Types 0-3 draws into any colour target (incl. instanced 2/3 since 51f5cc0c) | Decoded from the record and MT objects; no D3D9 calls | ndraw_native.rs `draw_call` :1719; L119 "L141: instanced draws" |
| Type 4 Clear, type 5 StretchRect | Native (`ClearCall`/`CopyCall` sent in order) unless `DDDRAW_NATIVE_OPS=0` | ndraw_ops.rs |
| Type 7 query begin/end | **Default path** (D3D9 occlusion query objects); draws inside a query span stay on the D3D9 path in `native`/`compare` mode (not in `drawcall`) | ndraw_native.rs :2625; MILESTONES L141 |
| Draws the decoder refuses (`Refusal`: BlendFactor, FloatsUncovered for ints/bools, Device, NullBinding, NoIndices, Missing...) | Default path: lifted applies + D3D9 vtable. Train 6: 13-34 D3D9-path calls/frame left (clears, StretchRect, 11 refused type-0 draws) | MILESTONES L179; ndraw_native.rs :219 |
| Everything else the guest calls on the device directly (texture/buffer creation and locks, Present, Reset, GetData, StretchRect outside the executor) | D3D9 emulation in ddruntime `d3d9*` | NDRAW.md Appendix A: 216 sites / 46 methods / 42 owners |

So the **draw path** is no longer the D3D9 state machine; what remains of D3D9
is (a) the *data model* of the sink's output, (b) the mirrored device state it
reads, and (c) resource creation/upload.

### 1.3 What the sink does per taken draw

`Native::command` (ndraw_native.rs:2539) takes a global `Mutex<Inner>`, then
`decide` (:2607) -> `draw` (:2637) takes `d3d9dev::device_lock()` (:2662) and
calls `draw_call` (:1719). In order:

1. Read record +4/+8/+0xC/+0x10 (target, fixed, shader, stream bindings).
2. `kept_part` (:1687) for the target binding and the stream binding: one
   `FxMap<u32,...>` lookup each (`target_bindings`, `streams`), rebuilt the
   first time per frame (`target_part` :1402, `stream_part` :1329).
3. Constants: `last`-draw model (:1739-1819): same binding as the previous
   draw reuses its banks; otherwise `skipped_registers` (:1956) models the
   original's "same value pointer => skip upload" rule, `binding_part` (:706,
   9% self / 20.7% inclusive in the L193 profile) decodes the binding's
   descriptors and reads every uploaded register from guest memory into
   `Constants` banks (copy-on-write, content-keyed `by_content`), then
   `held_registers`/`fill_uncovered` (:950) patch registers the binding does
   not write from the newest pending upload or the *emulated device*.
4. Fixed state: `identity_of` (:2276) -> `FxMap<NativeStateIdentity,
   Arc<FixedPart>>` (`fixed_part` :504 runs mtstate on a zeroed scratch context
   once per identity); then a `StatesKey` lookup builds the full sorted
   `(RS, value)` list with the dynamic RS57/193/15/24/25 and the device's
   other states appended (:1840-1880).
5. Samplers and textures: `shadow_samplers` (:1271, a port of 0x00DAA8B0's
   sampler section run against the context and the emulated device) then
   `Mirror::textures` (:1191): clones a `Mirror { rows: [[u32;14];21],
   textures: [u32;21] }` (~1.3 KB), applies the draw's shadow, hashes 21 slots
   x 15 words, and looks up a texture-list cache.
6. Draw parameters (`draw_params_of` :2284), index format lookup in
   `index_formats`, assemble `DrawCall` (~10 `Arc` clones, :1920-1940).
7. Back in `draw`: `reads_dirty` dirty check (:2586), `send_draw` -> the
   `DeferredBackend` queue -> the ddgpu thread.

ddgpu then (lib.rs): `try_draw` :2409 -> `derived()` :2212 hashes four `Arc`
addresses, compares `Arc::ptr_eq` x4 plus `targets` (array of `Option<Target>`
value compare), `depth`, `strides(d)` iterator, and `instanced` on every draw;
on a miss `derive()` :1954 parses shaders, matches declaration elements to the
shader's `dcl` usages, builds `PipelineKey` from `state(7/195/175/...)`
binary searches of the render-state list. `draw_bindings` :2242 keys on
`Arc` pointers. `constants()` :1083 compares to the last block (ptr_eq or
content) and otherwise `pack_constants` copies the compact register prefix
(+16 int4 + 16 bool + extras; blocks VS_BLOCK 4,400 B / PS_BLOCK 4,016 B max,
cache.rs:195) into the uniform ring. With raw Vulkan (`DDGPU_VK=1`) the same
inputs feed `vk::PipelineTemplate`s.

### 1.4 Where the per-draw CPU goes (measured, quoted)

| Source | Item | Figure |
|---|---|---|
| frame-bound.md round 4 (profile_prof5, 0x1034 busy 77%) | `draw_call` self | 21.7% (~2.8 ms; one 600-line fn with caches inlined) |
| | memcpy | ~9% (callers: `draw_call` 46%, `Native::draw` 22%, `[Constants;2]::default()` 20% = the per-draw `mem::take(&mut c.banks)` scratch) |
| | `Mirror::textures` | 4.8% |
| | hashbrown u32 lookups (`kept_part` target/stream, `Caches::meta`, `contains_key` in the dirty check) | ~5% |
| | `mi_malloc` | 3.3% (was 5.3%) |
| | `Instant::now` | ~4% (gated off since "L180: no clock reads per taken draw") |
| decouple2 log 19:01 (prof_n4) | `draw_call` self 12.4%, `binding_part` 9% self / 20.7% incl, `Mirror::textures` 7% incl, alloc 7%, `begin_frame` 3.9% | different build, same shape |
| L180 bench (release `NDRAW_BENCH`) | `draw_call` from the caches | 0.20-0.30 us; a frame's first draw 1.31-1.43 us; ~0.73 us for a draw that decodes its binding |
| in game, combo46 (before the speed commits) | per decided draw | 4.2-4.8 us; combo24 3.88 us; combo22 3.2-3.4 us |
| DECOUPLED_RENDER.md section 6.3 table | whole sink, ~5,400 draws | 12.7 ms/frame (2.35 us/draw) |
| frame-bound.md thread load | render worker busy | 75% (63-77%), waits 26%; main thread is the bound at ~57 fps |

Taken together: the cache-hit path is already near its floor for the
*current data model*; what the numbers say is that the **model** (rebuild a
D3D9 description per draw, reconcile it against emulated device history) is
what costs, and the bench misses it because it uses one binding.

### 1.5 What ddgpu already has (the other half)

`crates/ddgpu/src/native.rs` (1,944 lines): an immutable `Device` (handles
`Blend`, `Rasterizer`, `DepthStencil`, `Sampler`, `InputLayout`, `Shader`,
`Texture`, `View`, `Buffer`, `Pipeline`, `TextureSet`, `ConstantBuffer`;
`PipelineDesc` :169 = VS, PS, input, blend, rasterizer, depth-stencil, colour/
depth formats, samples, mask, topology, strip index, `sampler_ones`,
`depth_bias`), `create_pipeline` :661, a `Recorder` :1098 with
`set_pipeline/set_texture_set/set_constants/update_constant_*/set_vertex_buffer/
draw*`, render bundles, a resident-constant scatter (`native_uniforms.rs`) and a
raw-Vulkan bridge (`native_vk.rs`). Its input today is `NativeJob`
(`d3d9cmd/src/native.rs`) processed by `Worker` (`native_job.rs`:
`fixed` :678 builds blend/raster/depth handles from the D3D9 RS list keyed by
`NativeStateIdentity`; `input` :757 keyed by `(Shader, stride, ...)`; `textures`
:834 builds `TextureSet`s per `(Pipeline, stage)`; constants applied per
shader-keyed `ConstantBuffer` with `applied` pointer-compare). It was verified
pixel-exact in compare mode. Its limits are the reason it is not default: its
transport (`NativeDraw` with `Arc`s of D3D9-shaped data, a `Pending` -> `NativeJob`
channel), rejections (strip restart, queries, RT sampler fill, feedback,
instanced), and that the sink still produces both the DrawCall and the
NativeDraw in `compare` mode.

## 2. MT Framework's nDraw command structure

All layouts below are cited from [NDRAW.md](NDRAW.md); re-derived or added
facts say so. Addresses are the 32-bit 2017-12-13 Steam exe.

### 2.1 Stream and records

- Per frame the main thread builds an array of 8-byte entries `{u32 sort_key;
  u32 record}` (sRender+0x433DD8 count +0x433DE0, selected slot +0x33DB8 XOR 1;
  double buffered: the render worker executes the *previous* frame's stream,
  NDRAW.md section 3). Pass id is in the top 5 bits of the key; the merge
  sort is `0x00FEDD60`, queued by `0x00FEE130`. The record's type is its
  byte +2 low nibble.
- Executor `0x00DAB480` walks entries, type 6 pushes a nested list
  (256 deep), types 0-3 are draws, 4 Clear, 5 StretchRect, 7 query.

| Type | Record | Size |
|---|---|---|
| 0 | +4 targets*, +8 fixed*, +0xC shader*, +0x10 streams*, +0x14 element count, +0x18 start vertex | 0x1C |
| 1 | as 0, +0x18 start index, +0x1C base vertex (indexed) | 0x20 |
| 2 | as 0, +0x1C instance count, +0x20 start instance | 0x24 |
| 3 | as 1, +0x20 instance count, +0x24 start instance | 0x28 |
| 4 | +4 mask, +8 depth, +0xC stencil, +0x10 RGBA f32, +0x20 fallback draw record | |
| 5 | +4 dest Texture, +8 subresource, +0xC source selector, +0x10 rect, +0x20 dst x/y, +0x28 draw | |
| 6 | nested list +4 base, +8 entries, +0xC count | |
| 7 | +4 begin/end, +8 query | |

Adjacent compatible type-1 records are coalesced by the executor when
sRender+0x30 is set (`0x00DAB5B0..0x00DAB6AC`); the sink bypasses this
merging in drawcall/native/compare modes (ndraw_native.rs header) and sees
each raw record.

### 2.2 Bindings: four immutable-ish blocks per draw (NDRAW.md section 3)

| Block | Size | Layout | D3D9 equivalent today |
|---|---|---|---|
| Target | 0x5C | RTV*[4] +0, DSV* +0x10, flags +0x14 (bit9 selects viewport/scissor set), viewport rects +0x18/+0x28, scissor rects +0x38/+0x48, dims u16 +0x58/+0x5A | SetRenderTarget/DepthStencil/Viewport/ScissorRect |
| Fixed | 0x14 | blend factor RGBA +0, stencil ref byte +4 and raster-orientation bit8 of the dword +4, BlendState* +8, DepthStencilState* +0xC, RasterizerState* +0x10 | ~60 SetRenderState |
| Stream | 0x30 | VB*[4] +0, stride bytes +0x10, offsets +0x14, instance flags +0x24, IB* +0x28 | SetStreamSource/SetIndices (+Freq for 2/3) |
| Shader | var | dword0 alpha-test + topology flags (topology bits21:28), dword1 identity, +8 metadata*, +0xC value slots (pointers, low nibble tags float counts) | SetVertexDeclaration/Shader/Texture/SamplerState/ShaderConstantF/I/B |

The producer side (draw context, cDraw `0x00FED7E0`, `commit_primitive_state`
`0x00FEE320`/`0x00FEEF20`) snapshots dirty context blocks and publishes them as
"dependency packets" (L177 doc; the dirty bits are at context +0x154 in
`0x00DA8590`, which also marks resources with `epoch+1`). Consequence, and the
reason pointer-keyed caches hit: a gameplay frame has ~5,200 commands but only
67 distinct state objects, 222 VS, 183 PS, 52 input layouts, 23 samplers
(L111 run instr1, NATIVE_RENDERER.md). **UNKNOWN**: how many distinct *binding
blocks* per frame (the sink's `built:` report line counts target/stream parts
built and bindings decoded per frame, never recorded in a doc; the census in
2.6 should record it).

### 2.3 The immutable state objects (NDRAW.md section 2, 9)

| Object | Size | Descriptor at +0x10 | Hash +0xC | Mapping to Vulkan/wgpu (NDRAW.md section 9) |
|---|---:|---|---|---|
| BlendState 0x28 (ctor 0x0117CB70) | | 16 B: d0 bit1 enable, [9:16] src, [17:24] dst, [25:31] op; d1 alpha factors/op; d2 per-target write masks | `0x00FF92E0` over the packed payload | `BlendComponent` x2 per target, `ColorWrites`; Min/Max need One/One factors; blend *factor* is dynamic (fixed block +0) |
| RasterizerState 0x38 (0x0117C6E0) | | 16 B: fill, cull, orientation, 4 flags, signed depth bias, slope bias | hash of 2nd payload | cull/front-face/polygon mode; **D3D9 float bias stays in the pixel shader** (ddgpu `depth_bias` flag), not the hardware bias; d2 unused |
| DepthStencilState 0x20 (0x0117C8E0) | | 8 B: depth en/write/func, stencil en/masks/ops front+back | hash | `DepthStencilState` + dynamic stencil ref (fixed +4); 2-sided if face tuples differ |
| SamplerState 0x3C (0x0117CD70) | | 32 B: filter selector, 3 address modes, aniso, LOD bias, max mip, border RGBA f32 | hash | wgpu sampler; gaps: MirrorOnce, arbitrary border, LOD bias (shader-side), max-mip as min-LOD-clamp |
| InputLayout 0x28 (0x0112AAB0) | | source array at +0x18 (count +0x1C), generated elements +0x10/+0x14 | hash of count*8 B | `VertexBufferLayout` per stream; formats table 0x01441BD8 |
| RenderTargetView/DepthStencilView 0x28 (0x011042B0/0x01104470) | | +0x10 parent Texture*, +0x14 D3D surface, +0x18 w, +0x1A h (RTV top bit = sRGB), +0x1C subresource, +0x24 format | n/a | attachment view + format/samples in the pipeline key |
| Texture 0x50 | | +0x20 w/h/flags, +0x24 mips/kind, +0x28 engine format, +0x2C flags, +0x30 D3D format, **+0x34 D3D base-texture handle**, +0x38 alias | +0xC = `(flags>>12)&0xFF` (not a hash) | sampled view; handle +0x34 is the key into our runtime's resource table |
| VertexBuffer/IndexBuffer 0x28 | | +0x14 length, +0x20 D3D buffer, +0x24 CPU shadow | | backend buffer; mutable (locks) |

Identity facts that bound caching (NDRAW.md "Creation/caching conclusion"):
state objects are created per shader-table record at `0x00E11E30` with no
descriptor interning; payloads appear creation-only but there is no global
immutability proof; resources are refcounted with an epoch (`DAT_018d2ce8`) and
queued for release (`0x00DA7E50`); Texture recreation (`0x01105C80`) changes
handles under the same object. So: **object address + hash is a good key for
the state objects *within a validated lifetime*, not a naked address.** The sink
already uses `(address, hash)` for fixed state; for textures, buffers and views
the identity must come from a runtime-side generation (resource id).

### 2.4 Shader binding: how materials and constants are bound

(NDRAW.md section 5, `0x00DAA8B0`.) Shader binding +8 -> compiled metadata:
+0x10 VS, +0x14 PS, +0x18 declaration, +4 descriptor array, +8/+0xC VS/PS
summaries (low8 section size, bits8:11 sampler count, 12:18 float uploads,
19:22 int, 23:27 bool, 28:31 derived).

- **There is no constant buffer object.** Constants are *value slots*: the
  binding's `+0xC` array holds up to 32 pointers (descriptor "value index" is
  5 bits); a constant descriptor (4 bytes) says `bit0 int->float`, `bits1:5
  value index`, `bits6:15 source dword offset`, `byte2 dest register`, `byte3
  register count`. A "value slot" is in effect a named constant block of the
  shader's CTAB (FRAME_PASSES.md lists `CBShadowLight`, `CBShadowCast`,
  `CBSky`, `CBSoftBodySim`, `CBSoftBodyCollision`, `CBDOFFilter`,
  `CBGodRaysFilter`; the per-material/per-skin/per-object blocks are
  **UNKNOWN by name**, the register map per shader family is open, DECOUPLED
  A2 (ii)).
- Sampler descriptors: low12 sampler index, bits12:21 sampler-object value
  index, bits22:31 texture-object value index: samplers and textures are also
  *value slots* (pointers to SamplerState / Texture objects). VS samplers are
  D3D 257-260.
- Derived descriptors produce texture-dimension constants (kind 3 reciprocal,
  7 direct).
- Skipping rule (original behaviour we must keep, NDRAW.md and L180
  cb495a30): after a binding with the same VS (and PS for the pixel stage), an
  upload whose value slot holds the **same pointer** as the previous binding's
  is skipped, even if the memory behind it changed; unchanged binding pointer
  skips everything. The shader stage reads whatever registers the device
  held (FloatsUncovered: ints/bools unwritten are refused today).
- Alpha test is binding dword0 bits (enable 19 AND 20, func 3:10, ref 11:18);
  the executor context init sets RS 15/24/25 per frame; ddgpu implements it as
  shader `discard`.
- Topology: binding dword0 bits21:28 (0 point, 1 line list, 2 line strip,
  3 tri list, 4 tri strip); counts n, n/2, n-1, n/3, n-2.

### 2.5 CommandCache (`nDraw::CommandCache`, 0x1B0 bytes)

DTI at 0x018D6CAC, vtable 0x01447AF8 (no class-specific methods beyond the
destructor 0x011F0E80 and getDTI 0x011F08B0). `0x011F0ED0` copies draw records
types 0-3 into a downward-growing arena, remaps binding pointers and builds
8-byte entries; `0x011F0BE0` sizes it; `0x011F08C0` hashes dependencies
(NDRAW.md section 3). **New finding (Ghidra xrefs, this note):** `0x011F0ED0` has
exactly one caller, the DTI factory `0x011F1400`, which calls it with
`(0,0,0)`; `0x011F1400` itself has no code xrefs and the DTI object only
data-references (dtor, getDTI, the `new`). There is no static evidence the PC
build ever fills or consumes a CommandCache in gameplay. Treat it as dormant
until a runtime check (`DDRUNTIME_NDRAW_STATS` plus a hook on 0x011F1400)
says otherwise; it is therefore **not** an existing retained-draw mechanism
we can lean on, and a retained replay (DECOUPLED_RENDER A2) has to build its
own.

### 2.6 Where draws come from (counts) and where instancing is used

One representative gameplay frame (cap1 frame 7539, FRAME_PASSES.md, 5,295
draws in 951 runs): 864 tiny SoftBody GPU-sim runs (64x8/64x16 F32), texture
mip/convert chains (~35), sky, **shadow atlas casters pass 903: 489 draws**
(512x1536 R32 + D24S8), local shadow casters 900/905 (17+15), shadow
transforms/variance filter 906-913 (28), **scene depth/clip 918: 698**, **scene
normal/material 919: 1,354**, shadow receive 920: 75, light accumulation 921: 9,
half-res MRT 922: 145, **main scene 928: 814**, 930: 4, particles/effects 932:
181, depth/clip tail 933: 72, post (bloom, god rays, DOF, LUT, gamma) ~15,
**GUI + god rays + colour correction 949: 425**. Main-view draws (918-933) ~
3,500; shadow ~550.

Instancing today:
- Executor types 2/3 (instance count in the record; `SetStreamSourceFreq` loop
  at `0x00DAB875..0x00DABAB0`, the only code that sets frequencies); the sink
  takes them (`instanced_streams` :1367, 90 draws a frame in combo13; ddgpu
  `instancing()` lib.rs:166 and PipelineKey `buffers` step mode).
- The producers: `uOmObjBaseInstancing`/`uOmObjInstancing`/`uOmSwingBaseInstancing`/
  `uOmSwingInstancing` (DTI 0x19A557C.., object classes of 15-16 KB; swing
  objects are the vegetation sway family; `draw_instanced_mesh` 0x00C24640 is
  ported in `mtcore::semantic_instancing`). `cInstancingCulling` (0x18D4548,
  size 1120, vf 0x113D4xx..) with `cInstancingDirect/Dynamic/FromNulls/
  FromMatrices` (0x18D53E0/0x18D4588/0x18D53C0/0x18D4568) are the culling +
  instance-buffer builders. `cull_bounds` 0x113D520 is in the frame-bound
  profile (0.2 ms in the shadow job). Which unit types actually emit type 2/3
  in gameplay and how many draws they save is **UNKNOWN**; the L111 stats count
  type mix per frame and L193's `census.rs` (branch task/L193-decouple,
  `DDRUNTIME_CENSUS=1`, writes `census_out.md`) is built to answer it: draws
  by pass, mesh groups, material groups, static vs moving, instancing potential.
  No census output exists yet (docs/DRAW_CENSUS.md is absent on that branch at
  a1c10bff).

## 3. Target design: a direct consumer

### 3.1 Principles

1. **MT objects are the schema.** The unit of translation is an MT object
   (state, layout, shader metadata, view, texture, buffer), converted once per
   (object identity, generation) to the backend object; the per-draw work is
   reading the record, four pointer lookups and a draw.
2. **No D3D9 state numbers in the hot path.** No `(RS, value)` lists, no
   `D3DVERTEXELEMENT9`, no 21x14 sampler mirror, no `Constants.f: Vec<[u32;4]>`
   per draw. Descriptors map directly to wgpu/Vulkan (NDRAW.md section 9 tables
   already give the field-level maps; D3D9 numbering only reappears in the
   *lowering to DrawCall* kept for verify and for the fallback).
3. **Keep the game's observable semantics, not its call history.** Where the
   original's result depends on D3D9 device history (sampler apply deltas,
   upload skipping, stale textures), the consumer models the *result* with a
   small explicit state (section 3.6), and each such rule is a documented,
   testable deviation switch.
4. **The D3D9 path remains** as the fallback for refusals and as the oracle.
   The sink protocol (`Taken::Handled/Default`, `invalidate`/`flush_uploads`
   when switching back) is unchanged.
5. **One writer of the uniform arena, whole blocks.** Constants go from guest
   memory to the backend's uniform ring (or resident arena) in one gather pass;
   a block is re-gathered only when a value slot's pointer or a generation
   changed.

### 3.2 The unit: `DirectDraw` (host struct, no guest pointers)

```
DirectDraw {
  pipeline: PipelineId,        // (VS, PS, layout+strides, blend, raster, depth, colour/depth fmt, samples, topology)
  fixed_dyn: {stencil_ref: u8, blend_factor: u32, raster_variant: bool}   // dynamic, not in the key
  target: Arc<TargetPart>,     // attachments (resource ids, formats, size), viewport, scissor (existing TargetPart)
  textures: [TextureSetId; 2], // per stage: bind group id (or None)
  constants: [ConstBlockRef; 2]// arena offset (or "unchanged") per stage
  vertex: Arc<StreamPart>,     // (VB resource, offset, stride) x4 (existing StreamPart)
  index: Option<(Resource, Format)>,
  draw: {first, count, base_vertex, instances, first_instance}
}
```

`TargetPart` and `StreamPart` already exist (ndraw_native.rs:1395, :1323) and
already are keyed by binding address per frame; keep them. New are `pipeline`,
`textures` and `constants`.

### 3.3 Identity and caches (replacing ~14 maps in `Caches`)

| Cache | Key | Value | Validity / invalidation |
|---|---|---|---|
| State triple | (BlendState*, DepthStencilState*, RasterizerState*) pointers + their +0xC hashes (+ raster variant bit) | `(Blend, Raster, Depth)` backend handles + dynamic-needs flags (blend-factor used? stencil?) | built once from descriptors at +0x10 (no RS numbers); drop on object release notification or hash change; today's `FixedPart` is the same idea but yields an RS list |
| Input layout | (InputLayout* x up to 4 stream ids from the shader metadata, hash) | `InputLayout` handle + per-stream attribute lists | built from source array +0x18 via table 0x01441BD8; replaces `declaration` bytes + ddgpu's per-pipeline `dcl` matching (keep the matching, but once, in the pipeline cache) |
| Shader pair | metadata* (+0x10/+0x14/+0x18 handles) | `ShaderPair`: VS/PS `Shader` handles, constant gather plan (3.5), sampler plan, alpha test | per `MetaPart` today (:544), keep; key by metadata* + the D3D shader handles' generation |
| Pipeline | (ShaderPair, InputLayout, state triple, target format tuple, samples, topology, strip index, alpha-test/depth-bias flags) | `Pipeline` | the `ddgpu::native::PipelineDesc` (:169) is exactly this key; pointer-compared 3-4 small ids, no hashing of render-state lists. Persistent disk cache exists (`DDGPU_NATIVE_PIPELINE_CACHE=1`) |
| Texture set | per stage: tuple of (Texture*, SamplerState*) per sampler descriptor, in descriptor order | `TextureSet` (bind group) | key = small fixed array of (resource id, sampler id); resource id changes when the runtime recreates/ releases (generation); 23 distinct samplers and a few hundred textures per frame |
| Binding plan | shader binding* (per-frame published block) | `BindingPlan`: pipeline id, per-stage value-slot pointer vector, texture set ids | per frame by address (as `bindings` today) but **validated by content compare** of the 32 value-slot pointers + dword0/1 (cheap, 36 words), because bindings are arena blocks whose addresses are reused; content-equal => same plan (today's `by_content`) |
| Constant block | (stage, binding plan, value-slot pointer vector, **content epoch**) | arena offset | see 3.5 |

All caches are generation-stamped from a single `DirectGen` that increments on
device reset, on release of a guest resource the cache holds, and on
`sRender` re-creation; age-out as today (3 frames) for per-frame blocks, never
for MT state objects (they are small and long-lived).

### 3.4 Pipeline mapping from MT descriptors (no D3D9 numbers)

Straight from NDRAW.md section 9, reproduced as the implementation checklist
(each row is a unit test against the existing RS-list path):

- **Blend**: d0 bit1 enable; color factor/op `[9:16]/[17:24]/[25:31]` via table
  0x01445AD0 (factors 0..12) and 0x01445B48 (ops 0..4); alpha d1 bytes 0/1 and
  d1[16:22]; write masks d2 bytes 0-3 (targets 0-3). Separate alpha is always
  on (context init RS206=1). Min/Max -> factors One/One. Blend constant is a
  dynamic recorder call (`set_blend_constant`), and a **BlendFactor draw is
  refused today** (ndraw_native.rs:1826): the original applies RS193 only when
  the BlendState pointer changes, so the factor depends on history; the direct
  consumer replays that rule with one byte of state (the pointer last applied),
  which removes the refusal.
- **Raster**: fill table 0x01445A2C, cull table 0x01445A20, orientation variant
  chosen by fixed block bit8 -> `cull` + `front_face`; scissor flag d0[8]
  (dynamic scissor rect vs full); D3D9 float depth bias (`d1 * 5.96046e-8`, one
  ULP above 2^-24, + slope d3) stays in the pixel shader exactly as ddgpu does
  (`PipelineDesc::depth_bias`, `native.rs:187`): this is the one place the
  existing wgpu mapping is not "wgpu hardware bias".
- **Depth/stencil**: d0[0:5] depth en/write/func via table 0x01445A70; stencil
  d0[6:30], d1; two-sided when face tuples differ; masks d0 byte1/2 (read/write);
  stencil ref from fixed +4 (dynamic).
- **Sampler**: d0 low8 filter selector -> (mag,min,mip) table; address modes
  d0[8:16]; aniso d0[17:21]; LOD bias d1 (shader-side in ddgpu: PS extra slots 1-5,
  `write_bias` native.rs:1012); max mip from d2; border d4-d7. Known gaps shared
  with the existing path: MirrorOnce, arbitrary border colours.
- **Input layout / streams**: table 0x01441BD8 -> vertex formats (Float32xN,
  Float16x2/4, Sint16, Snorm16, Uint8x4, Unorm8x4), strides from the stream
  block (+0x10 bytes), instance flags +0x24 and the record's type 2/3 select
  `Instance` step mode with the executor's 0x80000001 / 0x40000000|count rules.
- **Targets**: RTV +0x14 surface/+0x10 parent/+0x1C subresource/+0x24 format
  -> attachment (existing `target_part` builds `Target` with resource ids; keep).

### 3.5 Constants: gather plans, whole blocks, kept register file

The sink today builds a `Constants` per binding (`binding_part`), then
ddgpu `pack_constants` copies it into the arena. Instead:

1. **Gather plan per shader pair**, built once from the metadata's constant
   descriptors (NDRAW.md section 5): for each stage a list of
   `(value_slot, src_dword_off, dst_reg, count, convert)` already merged into
   runs (adjacent registers from adjacent offsets of the same slot become one
   memcpy), the ints and bools likewise, plus the shader's read set
   (`reads()` :371, already computed) which sizes the compact block (ddgpu
   `ConstantLayout`: floats prefix + int4x16 + bool x16 + extras).
2. **A resident register file per stage** (`[u32;4] x 256/224` + ints/bools)
   that models the *D3D9 device's* registers across the whole frame. Applying
   a binding = for each plan run, if the run's value-slot pointer differs from
   the previous binding's (and the shaders match per the skipping rule) copy
   the source bytes into the file, mark the arena block dirty. A draw's block
   is the file's compact prefix: if nothing changed since the last emitted
   block it reuses the offset (today `constants()` ptr_eq / content compare).
   This subsumes `held_registers`, `fill_uncovered`, `skipped_registers`,
   `CowBanks`, `last`, `uploads` and the `FloatsUncovered` refusal for floats
   (unwritten registers simply keep their value, exactly like the device) and
   removes the `[Constants;2]::default()` scratch (memcpy 20% of the memcpy
   share).
3. **Value-slot sources are live memory** (the original re-reads them at every
   apply that is not skipped; L180 combo42: a binding reused later in the frame
   after its source changed). So the gather reads at draw time. A per-slot
   *version* would make this free when nothing changed; none exists in MT's
   blocks (**UNKNOWN**), so the cheap exact test is a content compare of the
   bytes about to be copied against the register file (a 16-byte-lane
   compare is cheaper than the copy+hash it replaces, and L180's page-checked
   16-register reads already exist: ndraw_native.rs `words`).
4. The arena is the existing resident scatter (`native_uniforms.rs`, delta
   upload) or the ring (`Ring::alloc`) depending on backend; the *interface* is
   `update_constant_range(handle, byte_off, bytes)` already on `Recorder`
   (native.rs:1523).

Hook for instancing / replay: the plan marks which `(slot, off)` runs carry
per-object data (those differing between consecutive draws of one
(VS, viewport) group; this is what `census.rs` already computes as "per-object
registers"). A `DirectDraw` retains `(binding plan, value-slot pointer vector)`;
a replay re-gathers only the marked runs (matrices, skin palette) and reuses
the rest.

### 3.6 Textures and samplers without the device mirror

Today: `shadow_samplers` + `Mirror::textures` reproduce 0x00DAA8B0's sampler
section against the emulated device because (a) the original applies a stage's
SamplerState only when the context's cached sampler *object* changes, (b) the
global filtering option `[0x018D1D20]+0x34` overrides anisotropy only on the
non-null-old branch, (c) a texture is set by object identity, so a recreated
D3D texture under the same nDraw Texture object is **not** rebound
(NDRAW.md "Sampler descriptor and dynamic texture state"; ndraw_native.rs header).

Direct model: per stage keep `current[stage] = (SamplerState*, hash, effective
sampler id, Texture*, resource id)` (21 slots, one small array, no states
copied). A draw's descriptors give new `(SamplerState*, Texture*)`; the
effective sampler is `f(new, old)` only when the option at `[0x018D1D20]+0x34`
is nonzero and new MINFILTER == 3 and old != null; with option 0 (**UNKNOWN**
whether the benchmark config uses 0; read it once per frame into `FrameInfo`)
the effective sampler is the new object's. The texture bound is the nDraw
Texture's *current* handle (+0x34) -> resource id. This deliberately differs
from the original in one case (texture recreated under an unchanged object: the
original keeps the stale D3D texture, we bind the current one); such a draw is
reported by the verify mode as a known/allow-listed difference, and so far
there is no evidence it occurs in gameplay (**UNKNOWN**; device reset/resize and
render-target alias recreation `0x01105C80` are the candidates). The unused-VS-
stage `SetTexture(0)` clear (NDRAW.md, 0x00DAA8B0) has no observable effect on
the draw and is dropped.

Texture set cache key = the 21-slot `(resource id, sampler id)` array restricted
to the shader's sampler descriptors (typically 3-8 stages), hashed with a
precomputed XOR of per-stage ids (not 21 x 15 words). Cache hit path: compare
the previous draw's set tuple first (L94b did this in ddgpu).

Also required: render-target textures lack channels D3D9 samples as 1
(`sampler_ones`, ddgpu), feedback sampling of a bound attachment needs a
snapshot copy, and the sRGB-read flag (`Texture +0x20 bit30 && !(+0x2C bit19)`
and the renderer byte +0x31) selects the view. All already implemented behind
`TextureBinding`/`target_view`; the Direct layer keeps feeding them the same
facts (`kind`, `usage`, `format`), obtained from a **lock-free texture info
table** instead of `objects()`: the runtime's CreateTexture/Create*Surface
hooks (D3D calls from `0x01105240`, `0x011042B0`, `0x01104470`) publish
`resource id -> TextureBinding info` at creation, so `Caches::texture`
(:1595, which today calls `objects()` under the global D3D mutex) disappears.

### 3.7 Threading and locks

`Native::command` takes `inner: Mutex` and `d3d9dev::device_lock()` per draw
(ndraw_native.rs:2540, 2662): two uncontended lock round trips per draw plus
the `objects()` accesses. The Direct layer reads only guest memory and its own
tables; it needs the device lock only (a) at the frame's start to snapshot the
device's constant registers/render states that the D3D9 path may have changed
(`begin_frame`, once), and (b) when a draw falls back to Default (`invalidate`).
Per-draw locks can go.

### 3.8 What stays on the D3D9 path (initially)

Queries (type 7: occlusion; in `drawcall` mode draws inside query spans are taken
and the D3D9 query still counts samples via `spans`, keep that), refused draws,
anything touching Present/Reset/locks. After S4 the D3D9 device is needed only
for resource creation and uploads, which are load-time or dynamic-buffer
events, not per draw (NDRAW.md section 6 hook candidate 4).

### 3.9 Backend interface (S2): from `DrawCall` lowering to handles

S1 lowers a `DirectDraw` to the existing `DrawCall` with `Arc`s cached per
MT identity (state-triple -> `RenderStateList`, input layout -> declaration
`Arc<[[u8;8]]>`, texture set -> `Arc<[TextureBinding]>`, constants -> `Arc<Constants>`
shared across draws that share a block). That keeps ddgpu's `derived()` memo
(ptr_eq on those Arcs) hitting without any change. S2 adds a
`GraphicsBackend::direct_draw(&DirectDraw)` (or reuses `native::Recorder`) taking
backend handles, skipping `derive/derived/draw_bindings/constants`. ddgpu then
does per draw: `set_pipeline` (if changed), `set_texture_set`x2 (if changed),
`set_constants` x2 (offsets), vertex/index buffers (if changed), `draw`. The
L110 floor is 0.33 us (ash) / 0.39 us (bundles) / 0.88 us (wgpu) per draw
including finish.

## 4. Verification: proving frames identical to the D3D9-path output

Constraint: gameplay is not deterministic across runs, so image comparison must
be between two paths run on the **same inputs in the same run** or on a captured
stream replayed offline.

1. **Decode oracle (CPU, exact, existing).** `tests/ndraw_native.rs` builds
   real nDraw objects and runs both the D3D9 path and the sink on them, checking
   the sink's DrawCall equals the D3D9 path's DrawCall (render states,
   constants, textures) incl. instanced types (L119, L141). The Direct decoder
   adds a `lower(&DirectDraw) -> DrawCall` and the same test applies unchanged
   to its output; add cases for the deviation switches in 3.6.
2. **`DDDRAW_NATIVE=verify` in game (existing, extend).** Takes nothing; the
   D3D9 path draws; every draw the decoder could take is compared field by
   field (`compare` :3654, `differences` :3646; 5.4M draws, 0 mismatches
   in train 5/combo46). For Direct: compute *both* the old decode and the
   Direct decode from the same guest memory and compare the lowered
   `DrawCall`s (and compare Direct's lowering with the D3D9 path's DrawCall as
   today). New env: `DDDRAW_DIRECT=verify` with per-field mismatch histogram and
   a first-N dump with the `VerifyContext` (record, binding, written regs).
   Acceptance: 0 mismatches over >= 1M draws on the benchmark save with
   `DDDRAW_NATIVE_TARGETS=all`, per pass (group by target size so the S1 pass
   has its own counter).
3. **Pixel compare in the same run (existing machinery).** `compare`
   mode: the D3D9-shaped DrawCalls draw the real targets; native work draws
   pre-created shadow copies; `native_compare.rs` reduces per-target
   colour/depth/stencil on the GPU (16 B back to the CPU per target) with
   `matched / mismatched / unverified / pending` counters (143,744 passes, 0
   mismatched at combo29). The Direct consumer plugs in as the "native"
   side for the selected pass; S2 first (it needs the backend handle path),
   because S1's lowered DrawCall goes through the same ddgpu code as the oracle
   and is covered by (1) and (2).
4. **Offline frame oracle (existing).** `DDDA_CAPTURE=<file>` wraps the
   backend (`d3d9frame::Capture`) and records every command; `d3d9frame::oracle::
   compare` replays a stream into the wgpu backend and the host D3D9 driver
   (`d3d9ref`) and names the first draw whose output differs
   (`ddgpu/examples/frame_oracle.rs`). Because the Direct layer lowers to
   `DrawCall` in S1, tee the lowering into the capture: the .ddframe then
   proves that *our DrawCall translation of the MT objects* renders like a real
   D3D9 driver given the same state, independent of the nDraw guest path. Needs a
   capture of a Direct-mode run, replayed through the oracle; replay cannot
   exercise the nDraw decode itself, only its output.
5. **Record/replay of nDraw itself (new, optional).** Snapshot a frame's
   record stream plus the reachable guest memory (bindings, state objects,
   value-slot sources) into a file and replay it through the old and the new
   decoder in a test (deterministic, no GPU); this is the regression net for the
   register-file model (3.5) and the sampler model (3.6). Cost: a pointer
   closure walker (the L111 observer already iterates the records).
6. **Deviation ledger.** The deliberate semantic differences (stale texture
   under a recreated object, dropped unused-VS-stage SetTexture(0), blend-factor
   rule implemented as state instead of refusal) get named counters in `verify`
   so that "0 mismatches" is checked against "0 unexplained mismatches".

Do not claim a fidelity result from FPS runs; do not use `DDGPU_VK_SHADOW`
sampling or `compare` timing as performance numbers (L159, docs say so).

## 5. Staged plan

Smallest first. Each stage ends with its verify gate; none changes
default behaviour until its gate passes. Savings are per-draw sink cost
**ESTIMATES** against the 2.35-4 us/draw range of section 1.4 and ~0.2-0.3 us
cache-hit floor; frame effect at ~5,400 draws/frame.

| Stage | Scope | Change | Per-draw sink | Frame (render worker) | Risk |
|---|---|---|---|---|---|
| S0 | none (measurement) | Split `draw_call` into non-inlined phases with `#[inline(never)]` timers behind `DDRUNTIME_NDRAW_STATS`; add the `built:`/distinct-binding counts per pass (target size) to the report; run L193 `DDRUNTIME_CENSUS=1` once and record pass x group x static/moving x instancing in docs/DRAW_CENSUS.md | n/a | n/a | none; fills the **UNKNOWN**s in 2.2, 2.6, 3.5 |
| S1 | **Shadow atlas pass 903** (target 512x1536 R32 + D24S8; 489 draws/frame in the cap1 frame; one shader set S15; depth-only style state; VS reads SoftBody via vertex samplers 257+) | `DDDRAW_DIRECT=shadow` (+ `verify`): state-triple, input-layout, shader-pair, texture-set and register-file caches keyed by MT identity; `lower()` to `DrawCall` with Arc sharing; no `Mirror`, no RS list; per-draw mutexes dropped for these draws | ~2-3 us -> ~0.4-0.7 us (decode side) | ~0.5-1.0 ms off the worker (489 x ~1.5-2 us) | low-medium: smallest state variety, no blend, no feedback, no MSAA, no queries; risk = vertex-sampler stages (257+) and skinned palette registers (large float runs) |
| S2 | Same pass | `direct_draw` into ddgpu (`native::Recorder` handles, VK path), bypassing `derive/derived/draw_bindings/constants` | backend side ~1 us -> ~0.3 us per draw (ddgpu memo compare of targets/strides, `pack_constants` gone) | + ~0.3-0.5 ms (ddgpu threads; off the critical path unless GPU thread binds) | medium: pipeline creation off the draw path (warm-up/persistent cache exists), bundle/VK recorder lifetime across passes, resource release invalidation |
| S3 | All scene passes: 918, 919, 920-923, 925-933, 922/923 (~3,500 draws); then GUI 949 (425 draws, many textures, alpha-test, scissor) | extend to blend states, alpha test, stencil, instanced types 2/3, MRT, half-res, feedback snapshot, sRGB target views; retire `Mirror`, `shadow_samplers`, `held_registers`, `fill_uncovered`, `LastDraw` | ~0.3-0.5 us cached / ~1-1.5 us binding change | ~2-4 ms (the 44% of worker time in 1.4, 60-70% removed) | medium-high: constants rule (3.5) and sampler/texture history (3.6) are where the D3D9 path hid exactness bugs (combo39/42/44/46 mismatches) |
| S4 | Clears (4), copies (5), query-span draws (7), refused draws | direct `ClearCall`/`CopyCall` stays as is (already native); retire the D3D9 fallback for BlendFactor and FloatsUncovered; D3D9 device only for resources | removes the last 13-34 D3D9-path calls/frame and the `invalidate` round trips | small; mostly removes mode switches | low-medium: occlusion query semantics (async queries L120) |
| S5 | Retained replay and instancing | `DirectDraw` frames retained per tick with the patch table (value-slot runs marked per-object); replay K times with interpolated matrices; merge groups sharing (pipeline, mesh range, textures) into instanced draws using the census' "instancing potential"; shadow/soft-body passes replayed on the tick only | replay of ~3,500 main-view draws target 1.5-2.5 ms (DECOUPLED_RENDER section 6.3) | the 240 fps route | high: draw identity across ticks, register map per shader family, d3d9wgsl instance-matrix input mode |

Order of work inside S1 (so the branch stays green):
1. `DirectGen`, state-triple + input-layout caches with unit tests against
   `fixed_part`/`mtstate::layout` (pure functions of descriptors).
2. Register-file model + gather plans with a randomized test against
   `binding_part`+`held_registers`+`fill_uncovered` (the L180 oracle tests
   `uncovered_floats_come_from_the_newest_upload_else_the_device`,
   `uploads_keep_the_last_value_of_each_register` already encode the rules).
3. Sampler/texture model vs `the_sampler_section_does_what_the_apply_does`
   (3,000 random cases vs `mtstate::shader`).
4. Wire as a second `CommandSink` implementation behind `DDDRAW_DIRECT`, falling
   through to the existing `Native` for non-selected passes.
5. `verify`, then in-game per 4.2, then the speed gate (>= 2 runs each, RULES.md:
   fps noise up to 7 fps).

Why the shadow atlas first: the simplest state (opaque depth/distance write),
the smallest shader/state variety, no alpha blend/test, no MSAA, no feedback;
it is also the pass the retained-replay design wants at tick rate (DECOUPLED
A2: shadow cascades are tick-bound), so its Direct form is the first thing the
replay can cache; and its 489 draws are large enough to measure (~9% of
draws) yet cannot hide a defect behind a whole frame. The alternative first
pass, 918 (698 draws, one A8 target + depth), is nearly as simple and larger
(13% of draws); choose it if S0's census says the shadow atlas draws mostly
use blend factor or multi-pass state changes.

## 6. Risks and open questions

| # | Risk / unknown | Why | Mitigation |
|---|---|---|---|
| R1 | Constant memory is live | Value-slot sources are re-read by the original at every non-skipped apply; L180 mismatches (combo39 ps c4, combo42 vs c9, combo44 ps c4) were exactly this | gather at draw time; keep the skipping rule bit-exact; verify per register |
| R2 | Pointer reuse | Binding blocks are arena memory; state objects freed and reallocated; texture handles change under an object | content-validated bindings; (address, hash) + release notification + generation for objects; never cache a naked guest pointer across a frame for mutable things |
| R3 | Sampler/texture history | Delta applies depend on the context's cached objects and the global filter option | explicit 21-slot model, option read per frame, deviation ledger |
| R4 | wgpu/Vulkan gaps | MirrorOnce, arbitrary border colour, LOD bias, D3D9 float depth bias, point/line fill, alpha-to-coverage unused | reuse existing ddgpu handling (shader-side bias/LOD), refuse (fallback to D3D9 path) otherwise |
| R5 | Strips with restart, line strips, queries, feedback | the reasons native jobs were refused (strip restart 62,805 at combo20; fixed by L133 for non-indexed, indexed line strips remain) | keep refusal -> Default |
| R6 | Vertex-shader sampler stages 257-260 (soft body) | the shadow pass reads SoftBody textures in VS | in S1 scope explicitly; the `TextureBinding.sampler` already covers 257+ |
| R7 | Interaction with L172/L193 branches | shadow job, decouple gates, census code in `ndraw_native.rs` (12 lines) and `census.rs` | Direct is a new sink next to `Native`; no edits to `draw_call` except hooks; rebase after the render-worker agent's current changes land |
| R8 | The estimate | The worker is not the bound at 60 fps; savings only show in fps once main-thread work (sim 9.5 ms, shadow tail 1.75 ms) shrinks or decoupling lands | S1/S2 judged on worker CPU (profile + per-pass timers) and verify, not on fps; fps judged only with >= 2 runs |
| U1 | Which unit types emit type 2/3 and how much instancing is available | no census data | S0 |
| U2 | Register map per shader family (world matrix, skin palette, camera) | CTAB names exist, mapping not extracted | from `d3d9shader` CTAB of the 402 shader blobs (FRAME_PASSES.md has hashes); needed for S5 only |
| U3 | Is `[0x018D1D20]+0x34` ever nonzero in the user's config | decides whether sampler history matters | read in S0 and print |
| U4 | Does anything fill a CommandCache at runtime | static xrefs say no (2.5) | hook 0x011F1400 in a stats run |
| U5 | Per-frame binding-block count and change rate | decides the cost of content validation | S0 counters |

## 7. Code and doc index (for the implementer)

- Sink protocol: `crates/mtdraw/src/sink.rs` (CommandSink, Taken); executor call
  sites `crates/mtdraw/src/executor.rs:300-336`.
- Sink implementation to mirror/replace: `crates/ddruntime/src/ndraw_native.rs`
  `draw_call` :1719, `Caches` :1480, `begin_frame` :1535, `binding_part` :706,
  `descriptors` :601, `fixed_part` :504, `meta_part` :573, `shadow_samplers` :1271,
  `Mirror` :1139/:1191, `target_part` :1402, `stream_part` :1329,
  `instanced_streams` :1367, `native_draw` :2315, `Pending` :2382, `Native::command`
  :2539, `decide` :2607, `draw` :2637, verify `compare` :3654.
  Clears/copies: `ndraw_ops.rs`.
- Semantic decoders (reuse unchanged): `crates/mtstate` (descriptor -> packed,
  `layout`, `shader`, `targets`, `streams`; 17,928-call original-instruction
  vectors, NDRAW.md section 9).
- Backend DrawCall world: `crates/d3d9cmd/src/lib.rs:155` (`DrawCall`),
  `native.rs` (`NativeDraw`, `NativeJob`); ddgpu `lib.rs` `derive` :1954,
  `derived` :2212, `draw_bindings` :2242, `constants` :1083, `try_draw` :2409;
  `cache.rs:158` (`PipelineKey`), :195 (`VS_BLOCK/PS_BLOCK`).
- Backend native-handle world: `crates/ddgpu/src/native.rs` (`Device` :238,
  `PipelineDesc` :169, `create_pipeline` :661, `Recorder` :1098,
  `update_constant_range` :1523), `native_job.rs` (`Worker::fixed` :678,
  `input` :757, `textures` :834, `prepare` :913, `run` :1104),
  `native_uniforms.rs`, `native_vk.rs`, `native_compare.rs`.
- Frame capture/oracle: `crates/d3d9frame` (`Capture` lib.rs:833,
  `oracle::compare`), `crates/d3d9ref`, `crates/ddgpu/examples/frame_oracle.rs`,
  [FRAME_ORACLE.md](FRAME_ORACLE.md).
- Tests to extend: `crates/ddruntime/tests/ndraw_native.rs`, the in-file tests
  of `ndraw_native.rs` (:2860-3556), `crates/mtdraw/tests/differential.rs`.
- Census: `crates/ddruntime/src/census.rs` on branch `task/L193-decouple`
  (`DDRUNTIME_CENSUS=1`, `DDRUNTIME_CENSUS_OUT`); output not yet produced.
- Ghidra evidence added by this note: CommandCache `0x011F1400` /
  `0x011F0ED0` / `0x011F08B0` / vtable `0x01447AF8` xrefs (2.5); cInstancing*
  and uOm*Instancing DTI rows from `E:/ddda-re/notes/dti-registry.json` (2.6);
  `0x00DA8590` context dirty-bit/epoch marking (2.2).
