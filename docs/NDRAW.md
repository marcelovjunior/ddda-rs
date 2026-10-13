# DDDA nDraw / D3D9 boundary

L108, work in progress, 2026-10-13. All addresses refer to the 32-bit
2017-12-13 Steam executable identified in [METHODOLOGY.md](METHODOLOGY.md).
Evidence is static disassembly/decompilation and the private DTI registry;
no game run, renderer implementation, image comparison or timing is claimed.
Descriptive function names below are research labels, not original symbols.

## Findings available for downstream work

- `sRender` singleton pointer is `0x018D08F4` (written by constructor
  `0x00DB1A50`, store `0x00DB1BBA`); its `+0x98` is the D3D9 device
  (`0x00DAF7C0`, `0x00DAB480`). There is no separate device global established
  by this research. A draw-local state cache carries another pointer at `+4`.
- `0x00DAB480`, with sRender in ECX, executes command records and owns the
  draw calls. Commands retain target, state, stream and shader-binding
  objects up to the four apply calls `0x00DA6FE0`, `0x00DAA5F0`,
  `0x00DAA7C0`, `0x00DAA8B0`. These are the initial hook boundary.
- State constructors preserve descriptors AND prepack D3D9 state. Resource
  `+0x0C` holds the hash computed by `0x00FF92E0`. `0x00DAA5F0` compares
  object identity and then this hash before calling the packed-state apply
  routines. This proves redundant application suppression, not global
  allocation deduplication or unconditional immutability.
- `nDraw::CommandCache` is a real copied command cache, not merely a D3D
  state cache: `0x011F0ED0` copies type 0/1/2/3 records of 28/32/36/40 bytes,
  remaps their payload pointers, and builds eight-byte entries.

## 1. D3D9 call census

[L20](tasks/L20-d3d9-completeness.md) established 213 sites / 46 device
methods by device-pointer flow. Its private census is the starting point;
the owner-by-owner expanded table and additional coverage audit follow in
the next batch. It is not an exhaustive proof for arbitrary escaped device
pointers. The source object must be verified: an arbitrary virtual call at
offset `0xE4` is not sufficient evidence for SetRenderState.

## 2. Object identity and layouts

DTI addresses, sizes, vtables and creation paths will be tabulated in the
next batch. Confirmed state constructor entries:

| Object | Size | Constructor | Descriptor bytes at +0x10 | Packed D3D9 payload |
|---|---:|---|---:|---|
| RasterizerState | 0x38 | `0x0117C6E0` | 16 | +0x20 and +0x2C, two 12-byte cull variants |
| DepthStencilState | 0x20 | `0x0117C8E0` | 8 | +0x18, 8 bytes |
| BlendState | 0x28 | `0x0117CB70` | 16 | +0x20, 8 bytes |
| SamplerState | 0x3C | `0x0117CD70` | 32 | +0x30, 12 bytes |

The constructors copy those descriptors, build those payloads, and call
`0x00FF92E0`; RasterizerState hashes its second payload. Field semantics
must be traced into the apply calls, not inferred from D3D11 naming.

## 3. Draw path and threads

`0x00DAC090` is the render worker: waits on sRender `+0xD0`, resets that
event, checks shutdown byte `+0xDE`, calls `0x00DAB480` when `+0x433E54`
is zero, optionally calls `0x00DAA4B0`, then signals completion `+0xD4`.
The guest thread ID `0x1034` is a measurement reported by
[NATIVE_RENDERER.md](NATIVE_RENDERER.md), not a constant in this function.
Main-thread submission and the synchronous path still require tracing.

## 4. Frame and plugs

Open: Scene ordering, plug scheduling, target transitions and Present.
The executor begins a D3D scene and ends it before resetting bindings
(`0x00DAB480`, `0x00DA6EA0`); that does not by itself establish a gameplay
pass order. See [FRAME.md](FRAME.md) for the previously researched main loop.

## 5. Shader binding

`0x00DAA8B0` binds declaration (`vtable+0x15C`), vertex shader (`+0x170`),
pixel shader (`+0x1AC`) from a shader binding's metadata object at `+8`.
That metadata carries declaration at `+0x18`, VS at `+0x10`, PS at `+0x14`.
It emits constant-register calls, including VS float `+0x178` and PS float
`+0x1B4`; these are not native constant-buffer bindings. Detailed descriptor
decoding, shader creation and package/cache ownership remain in progress.

## 6. Initial hook ranking

1. **Command executor `0x00DAB480`**: one complete command stream, state
   objects still intact. Must preserve nested commands, queries, clears,
   copies, instancing and resource lifetime; exact submission ABI and
   command root initialization need assembly verification.
2. **Object-aware applies `0x00DAA5F0` / `0x00DAA8B0`**: fixed-state and
   shader bindings remain intact. Must preserve dynamic blend factor,
   stencil reference, rasterizer orientation, register uploads and texture
   bindings. Frequency is per command before identity-based early return,
   not measured calls per frame.
3. **Packed-state applies `0x00DA7510` / `0x00DA77D0` / `0x00DA7B30` /
   `0x00DA7250`**: lower but original resource identity has already been
   stripped to a payload pointer. Useful as an oracle/transition boundary;
   less attractive as permanent native pipeline-cache keys.

These are candidates, not approved ports. ABI, exhaustive side effects and
oracle coverage must be completed before replacing any lifted function.
