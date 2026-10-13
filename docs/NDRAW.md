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
are decoded below from the apply calls, not inferred from D3D11 naming.

### DTI identities

All sizes are bytes, all addresses are VAs. DTI registration site is the
source of class name, parent and size; the object vtable is distinct from
the DTI object's own vtable. `new` below means DTI factory, which can pass
null/default arguments and is not necessarily the normal creation path.

| Class (nDraw unless stated) | DTI | Registration site | Object vtable | Size | DTI new |
|---|---|---|---|---:|---|
| BlendState | 0x018D63E4 | 0x01387EB7 | 0x01445B14 | 0x28 | 0x0117CD00 |
| RasterizerState | 0x018D63A4 | 0x01387E37 | 0x01445A38 | 0x38 | 0x0117C870 |
| DepthStencilState | 0x018D63C4 | 0x01387E77 | 0x01445A90 | 0x20 | 0x0117CB00 |
| SamplerState | 0x018D6404 | 0x01387EF7 | 0x01445B5C | 0x3C | 0x0117CFE0 |
| InputLayout | 0x018D431C | 0x01386217 | 0x01441BC0 | 0x28 | 0x0112AE00 |
| RenderTargetView | 0x018D3F58 | 0x01385A97 | 0x01440420 | 0x28 | 0x011059A0 |
| DepthStencilView | 0x018D3F78 | 0x01385AD7 | 0x01440438 | 0x28 | 0x011059E0 |
| Texture | 0x018D3F98 | 0x01385B17 | 0x01440400 | 0x50 | 0x011045D0 |
| VertexBuffer | 0x018D42DC | 0x01386197 | 0x014418F0 | 0x28 | 0x01129200 |
| IndexBuffer | 0x018D42FC | 0x013861D7 | 0x01441A18 | 0x28 | 0x01129670 |
| Buffer | 0x018D5684 | 0x01386BE7 | 0x0144315C | 0x20 | 0x0114A7A0 |
| Resource | 0x018D2CEC | registry / constructor 0x00FF92B0 | 0x0143A7FC | common prefix 0x10 | 0x00A268E0 |
| Scene | 0x018D5664 | 0x01386BAA | 0x01442ED8 | 0x2E4 | 0x0114A570 |
| CommandCache | 0x018D6CAC | 0x01388FFA | 0x01447AF8 | 0x1B0 | 0x011F1400 |
| sRender | 0x018D08FC | 0x0137F1AA | 0x014298BC | 0x440E20 | 0x00DB2420 |
| sShader2 | 0x018D12F0 | 0x0138037A | 0x0142EB2C | 0xAC2A8 | 0x00E132C0 |
| rShaderPackage | 0x018D3084 | 0x01383C7A | 0x0143B834 | 0x90 | 0x01008790 |
| rShaderCache | 0x018D6444 | 0x01387F77 | 0x01445C04 | 0x68 | 0x0117D130 |

Texture/VB/IB inherit Buffer; other listed nDraw objects inherit Resource
(registration sites above). The registry's ctor field for Texture/VB/IB
points at the Buffer base constructor; use the full paths below.

### Common prefix and lifetime

`0x00FF92B0`: `+0` vtable, `+4` last-use/frame epoch initialized -1,
`+8` interlocked reference count initialized 1, `+0xC` hash/derived key
initialized 0. `0x00FF9420` decrements references and either calls the
deleting destructor or queues release through `0x00DA7E50` when the
last-use epoch is not older than global `0x018D2CE8`. Draw producers such
as `0x00DA8590` mark resources with `epoch+1`. Do not free native handles
when an individual COM call finishes.

`0x00FF92E0(this,data,bytes)` computes the key stored at `+0xC` using
table `0x014075C0`, initial accumulator `0xFFFFFFFF`; aligned-size input
is processed dword-by-dword, most-significant byte first. Texture uses
this slot differently: `0x01104F40` writes `(flags>>12)&0xFF` there.
Thus `+0xC` is not universally a hash of every resource's content.

### Packed state fields and application

Notation: `w0`, `w1`, `w2` are dwords starting at the packed payload;
`[a:b]` denotes inclusive bits a through b. The destination state number
is included so a port can verify the interpretation without relying on
these descriptive names. Each apply accepts an optional previous payload;
null means apply all, otherwise it compares individual fields.

**BlendState**, `0x0117CB70` -> `0x00DA7510`:

| Packed field | Meaning | D3D render state |
|---|---|---:|
| w0[0] | alpha blending enabled | 27 |
| w0[1:5], [6:10] | source/destination color blend factors | 19,20 |
| w0[11:13] | color blend operation | 171 |
| w0[14:18], [19:23] | source/destination alpha blend factors | 207,208 |
| w0[24:26] | alpha blend operation | 209 |
| w0[27:30] | render-target 0 write mask | 168 |
| w1[0:3], [4:7], [8:11] | targets 1,2,3 write masks | 190,191,192 |

Descriptor at `+0x10`: dword0 bit1 supplies enable; bits9:16 source
color factor,17:24 destination color factor,25:31 color op. Byte4/5
supply alpha factors, ushort6 low7 alpha op; bytes8..11 supply target
write masks. Factor table `0x01445AD0`, operation table `0x01445B48`
are conversion tables, not already-native enum identity (`0x0117CB70`).
Separate-alpha enable is set to 1 by context init `0x00DA6DA0` (RS206).
Dynamic blend factor is outside this object, in the fixed-state binding
`+0`; `0x00DAA5F0` swaps its red/blue bytes before RS193.

**DepthStencilState**, `0x0117C8E0` -> `0x00DA77D0`:

| Packed field | Meaning | D3D render state |
|---|---|---:|
| w0[0], [1], [2:5] | depth enable, write enable, comparison | 7,14,23 |
| w0[6], [7] | stencil enable, two-sided stencil enable | 52,185 |
| w0 byte1, byte2 | stencil read/write masks | 58,59 |
| w0[24:27], w1[0:3], w0[28:31], w1[4:7] | stencil fail, depth fail, pass, comparison | 53,54,55,56 |
| w1[8:11], [16:19], [12:15], [20:23] | CCW fail, depth fail, pass, comparison | 186,187,188,189 |

Descriptor dword0: bits0/1 depth enable/write,2:5 depth comparison,
6 stencil enable,7:14 read mask,15:22 write mask,23:26 fail,27:30 depth
fail. Dword1:0:3 pass,4:7 comparison,8:11 CCW fail,12:15 CCW depth
fail,16:19 CCW pass,20:23 CCW comparison. Conversion table
`0x01445A70`; constructor derives two-sided enable by comparing faces.
Stencil reference is dynamic binding byte `+4`, emitted as RS57 by
`0x00DAA5F0`, not part of DepthStencilState.

**RasterizerState**, `0x0117C6E0` -> `0x00DA7B30`:

| Packed field | Meaning | D3D render state |
|---|---|---:|
| w0 byte0, byte1 | fill and cull mode | 8,22 |
| w0[16], [17], [18], [19] | clipping, scissor, multisample AA, line AA | 136,174,161,176 |
| w1, w2 | float depth bias, float slope-scaled depth bias | 195,175 |

Descriptor dword0:0:2 fill,3:5 cull,6 orientation,7..10 the four flags
above; descriptor dword1 is signed integer depth bias scaled by
`5.960465e-8` in the constructor, dword3 supplies slope bias. Descriptor
dword2 is retained but does not enter this D3D9 payload. Tables:
fill `0x01445A2C`, cull `0x01445A20`. The +0x20/+0x2C payloads exchange
cull 2/3. Dynamic fixed-binding bit8 at `+4` selects the variant.

**SamplerState**, `0x0117CD70` -> `0x00DA7250`:

| Packed field | Meaning | D3D sampler state |
|---|---|---:|
| w0[0:2], [3:5], [6:8] | U,V,W address modes | 1,2,3 |
| w1 | packed ARGB border color | 4 |
| w0[9:12], [13:16], [17:20] | magnification, minification, mip filtering | 5,6,7 |
| w0[21:25], [26:29] | max anisotropy, max mip level | 10,9 |
| w2 | float LOD bias bits | 8 |

Descriptor dword0 low8 selects a filter combination; bits8:10,11:13,
14:16 address modes; bits17:21 anisotropy. Dword1 is LOD bias, dword2
rounded to max mip level, dwords4..7 are RGBA float border color.
Dword3 is preserved but not used in the packed D3D9 state here.
In the *non-null previous-state* branch only, min filter 3 is adjusted
using `[global 0x018D1D20]+0x34`: option1 forces filters to 2;
options2..5 force anisotropy 2,4,8,16. Preserve that branch asymmetry
when matching the original. sRGB texture state11 is emitted separately
by `0x00DAA8B0`, based on texture flags and renderer `+0x31`.

**Creation/caching conclusion.** `0x00E11E30` builds the sShader2
table at `+0x3C + shader-id*8`, allocating each state object from resource
records (kinds3/10 sampler,4 blend,5 depth,6 raster,9 input layout).
The normal descriptors come from record `+0x18`; layout uses `+0x20`
and count at `+0x18`. There is no descriptor-equality search in this
loop: this is stable per-record storage, not proof of content interning.
`0x00F9EF20` also constructs four BlendStates directly. Observed apply
paths read state payloads without modifying them, but refcounts/epochs
change. No whole-program write proof has established payload immutability.
Cache native state by validated generation/lifetime, not a naked reused
guest pointer. Hash equality alone is insufficient as a native identity.

### Buffer, texture, views and input layout

Buffer constructor `0x0114A620`: `+0x10` usage/pool kind, `+0x14` byte
length, `+0x18` map/storage bookkeeping initially zero, `+0x1C` tracking
flag. Kinds1/2/5 register via `0x00DA8150` and set that flag. Remaining
bytes of the 0x20-byte base are not all semantically named here.

VB/IB extend it with `+0x20` D3D buffer and `+0x24` optional CPU shadow
allocation. Creation/recreation `0x01129030` / `0x01129460`: kind1 gives
usage0x200/default pool; kind3 usage0/system-memory pool2; other kinds
usage8/managed pool1. IB creation uses format101 (16-bit indices).
`0x01128F70` / `0x011293A0` read-lock the D3D buffer and allocate/copy
the CPU shadow. They are mutable resources; DTI factories `0x01129200`
and `0x01129670` also initialize concrete vtables/handles. Stream apply
`0x00DAA7C0` reads those handles and byte lengths.

Texture full paths include `0x01105B80` (named/cache path), `0x01105A90`
(called by Scene ctor), metadata initializer `0x01104F40`, and GPU
creation `0x01105240`. Layout from these functions and their consumers:

| Offset | Field / evidence |
|---|---|
| +0x20 | width u16; height bits16:29; sRGB-capable flag bit30; named-cache flag bit31 (`0x01104F40`, `0x01105B80`, `0x00DAA8B0`) |
| +0x24 | byte0 metadata retained; byte1 mip count; bits16:26 depth; bits27:30 texture-kind discriminator (`0x01104F40`, `0x01105240`) |
| +0x28 | engine format (`0x01104F40`) |
| +0x2C | creation/format flags; bit1 size relative to display; bit19 suppresses sRGB read (`0x01104F40`, `0x00DAA8B0`) |
| +0x30 | D3D format (`0x011042B0`, `0x01104470`) |
| +0x34 | D3D base texture handle, nullable for standalone surfaces (`0x00DAA8B0`, `0x011042B0`) |
| +0x38 | backing/alias Texture pointer (`0x01105C80`) |
| +0x3C,+0x40 | width/height scale relative to display (`0x01104F40`, `0x01105C80`) |
| +0x44,+0x48 | arrays of RenderTargetView*/DepthStencilView* (`0x01105C80`, `0x00FF1090`) |
| +0x4C | number of subresource view entries (`0x01105C80`) |

`0x01105C80` recreates dimensions/handles and view arrays, including
backing-texture aliases. Texture identity is not immutable GPU storage.
`0x01105B80` calls cache helper `0x00DA7EA0`; its exact dedup key and
eviction contract are not yet established. Do not extend the state-object
hash claim to texture contents.

Views (`0x011042B0` RTV, `0x01104470` DSV): Resource prefix, `+0x10`
parent Texture*, `+0x14` D3D surface, `+0x18` width u16, `+0x1A` height
(RTV masks to15 bits and stores sRGB in top bit), `+0x1C` subresource
index, `+0x20` copied texture flags, `+0x24` format. RTV can obtain a
texture/cube face surface or create a standalone RT; DSV calls
CreateDepthStencilSurface. Multisample conversion table `0x01440450`.
External-surface RTV ctor `0x011041E0` actually takes **two** stack
arguments `(surface, dimensions)` and AddRefs surface; decompiler falsely
merges them. Assembly `0x011041FD` loads the second pointer, `0x01104250`
returns8. This path leaves +0x24 unproven; do not read it unconditionally.
`0x00DA6FE0` applies `view+0x14` via SetRenderTarget/SetDepthStencilSurface.

InputLayout constructor `0x0112AAB0(this,elements,count)` hashes `count*8`
source bytes, generates declaration elements via `0x01129750`, and owns
one allocation: `+0x18` copied source array, `+0x1C` source count,
`+0x10` generated element array, `+0x14` generated count, `+0x20` rounded
byte extent; byte+0x24 is stored but its meaning is unresolved. These
are **not** a COM declaration at +0x18: `0x00E0FCC0` combines up to four
InputLayouts from the sShader2 table, rewrites stream/semantic fields,
adds the declaration terminator and calls CreateVertexDeclaration.
The result belongs to shader-binding metadata (+0x18 there), consumed
by `0x00DAA8B0`. Source element second dword: type bits6:10,
count11:17, offset22:30, flag31 (`0x0112AAB0`); exact interpretation of
the remaining descriptor fields is still open.

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
