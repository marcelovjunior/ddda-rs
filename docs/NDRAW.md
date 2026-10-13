# DDDA nDraw / D3D9 boundary

L108 research map, 2026-10-13. All addresses refer to the 32-bit
2017-12-13 Steam executable identified in [METHODOLOGY.md](METHODOLOGY.md).
L108 evidence is static disassembly/decompilation and the private DTI registry.
Section 9 adds L117's original-instruction differential tests and semantic
decoder. Neither task claims a game run, image comparison or FPS result.
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

Appendix A lists **216 identified sites / 46 methods / 42 owning functions**.
It corrects [L20](tasks/L20-d3d9-completeness.md)'s 213-site device-flow
census with SetRenderTarget `0x00DA7035`, SetDepthStencilSurface
`0x00DA7094`, SetStreamSource `0x00DAA878`. Scope and alias-analysis
limits are explicit there; this is not a proof covering arbitrary escaped
device pointers. An arbitrary virtual call at offset0xE4 is not sufficient
evidence for SetRenderState.

Owner groups: renderer init/reset `0x00DA6D00..0x00DA6EA0`, object/state
application `0x00DA6FE0..0x00DA7B30` and `0x00DAA5F0..0x00DAA8B0`,
executor `0x00DAB480`, renderer lifecycle/presentation `0x00DA9B10` through
`0x00DB1480`; shader factories `0x00E0FCC0..0x00E10000`; Texture/views
`0x01103DD0..0x01105240`; VB/IB `0x01129030..0x01129670`; display/query
resources `0x01147AB0..0x01148130`. The table names every owner explicitly.

## 2. Object identity and layouts

Confirmed state constructor entries:

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
truncated toward zero to max mip level (L117 assembly/vector correction),
dwords4..7 are RGBA float border color.
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
`0x01105B80` calls cache helper `0x00DA7EA0`, which searches string-key
pairs and texture compatibility, then appends a 12-byte registration at
sRender+0x43BED8 (count+0x43CADC). Compatibility `0x01104110` compares
dimensions, mip/array counts, texture kind, engine format, sRGB capability,
encoding flags and relative-size flag; it does not compare pixel contents.
Alias-group checks use Texture+0x38. Full eviction semantics remain open.
This is named compatible-storage reuse, not general content deduplication.

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
byte extent; byte+0x24 retains source bit31 at the first byte of the final
four-byte scan group (L117 vectors). These
are **not** a COM declaration at +0x18: `0x00E0FCC0` combines up to four
InputLayouts from the sShader2 table, rewrites stream/semantic fields,
adds the declaration terminator and calls CreateVertexDeclaration.
The result belongs to shader-binding metadata (+0x18 there), consumed
by `0x00DAA8B0`. Source element second dword: type bits6:10,
count11:17, offset22:30, flag31 (`0x0112AAB0`); exact interpretation of
the remaining descriptor fields is still open.

## 3. Draw path and threads

### Submission and executor context

`0x00DB1A50` creates worker `0x00DAC090` suspended, passes sRender,
stores thread handle+0xC8 and OS thread ID+0xCC, and resumes it. Start
event is +0xD0, completion event+0xD4. `0x00DA9CD0`, called from main
frame `0x0041DD60`, advances the resource epoch and XORs slot+0x33DB8
with1. It sets pending+0x21, snapshots mode+0x26 into+0x27, resets
completion and signals start when threaded mode is enabled. Main-thread
frame production `sRender::vf06 0x00DAF000` then constructs/sorts the next
stream. This is double buffering, not main-thread D3D emission per draw.

`0x00DAC090` waits start, sets working+0x20, resets start, checks
shutdown+0xDE, executes `0x00DAB480` if reset flags+0x433E54 are zero,
optionally calls `0x00DAA4B0`, signals completion, and clears working.
Guest ID0x1034 comes from [NATIVE_RENDERER.md](NATIVE_RENDERER.md)'s
measurement; it is not hard-coded. `0x00DAECC0`, `0x00DAED10`, and reset
`0x00DAF7C0` drain pending work: synchronous mode calls timing helper
`0x00DAB420`, executor, then `0x00DAA540`; threaded mode waits completion.
Never assume the executor can only run on one fixed OS thread.

`0x00DAB480` aligns ESP to16 and reserves0xE74 bytes plus three saved
registers. It passes a pointer into that frame to context initializer
`0x00DA6DA0(this=sRender,ctx)`, which zeroes **0xE28** bytes, stores device
at ctx+4 and initializes root cursor/end at+0x220/+0x224 from
sRender+0x433DD8/+0x433DE0 arrays. Selected slot is +0x33DB8, XOR1 when
mode+0x26 is set. Root count is multiplied by8. Thus apparent uninitialized
locals in DAB480 decompilation are actually initialized by DA6DA0.

| Executor-context offset | Meaning and consumer |
|---|---|
| +0 | alpha-test cached bits (`0x00DAA8B0`) |
| +4 | device (`0x00DA6DA0`) |
| +8,+0xC,+0x10,+0x14 | last target/fixed-state/shader/stream binding pointers |
| +0x18,+0x1C,+0x20 | BlendState/RasterizerState/DepthStencilState pointers (`0x00DAA5F0`) |
| +0x24,+0x28,+0x2C,+0x30 | VS,PS,declaration,IB COM handles |
| +0x34,+0x44,+0x54 | four VB handles, four strides, four byte offsets (`0x00DAA7C0`) |
| +0x64,+0x74 | viewport/scissor cached rectangles (`0x00DA6FE0`) |
| +0x84,+0x94 | four RTV pointers, DSV pointer (`0x00DA6FE0`) |
| +0x98,+0xD8 | vertex/pixel sampler-object cache bases (`0x00DAA8B0`) |
| +0x118,+0x158 | vertex/pixel texture-object cache bases (`0x00DAA8B0`) |
| +0x198 | pixel sRGB texture mask (`0x00DAA8B0`) |
| +0x21C | nested traversal array, 256 triples (base,cursor,end) |
| +0xE1C | current traversal depth (`0x00DAB480`) |
| +0xE20 | maximum stream-derived vertex count (`0x00DAA7C0`) |

Unused/intervening fields are not implied to be free space. Cache-array
capacities should follow the exact shader descriptors rather than this
table's distance between bases.

### Command format

An entry is `{u32 sort_key; u32 record_ptr;}`. Executor reads record+2
low nibble as type (`0x00DAB6AC`); eight-byte entry walking and nested
traversal are explicit at `0x00DAB504..0x00DAB58A`. It does not dispatch
through the record as a C++ vtable.

| Type | Record fields beyond header | Action in 0x00DAB480 |
|---:|---|---|
| 0 | +4 targets; +8 fixed state; +0xC shader; +0x10 streams; +0x14 element count; +0x18 start vertex | nonindexed draw; record0x1C bytes |
| 1 | same, +0x18 start index; +0x1C base vertex | indexed draw, MinVertexIndex0; record0x20 bytes |
| 2 | type0 fields; +0x1C instance count; +0x20 starting instance | instanced nonindexed; record0x24 bytes |
| 3 | type1 fields; +0x20 instance count; +0x24 starting instance | instanced indexed; record0x28 bytes |
| 4 | +4 clear mask; +8 depth; +0xC stencil; +0x10 RGBA floats; +0x20 fallback draw record | Clear if exact representable color/all selected targets, otherwise shader draw |
| 5 | +4 destination Texture; +8 subresource; +0xC source selector; +0x10 source rect; +0x20 destination x/y; +0x28 associated draw | StretchRect; depth variant brackets copy with EndScene/BeginScene |
| 6 | +4 nested base/metadata; +8 entries; +0xC count | push traversal triple; return to parent when exhausted |
| 7 | +4 begin/end selector; +8 query | query begin `0x011481B0` or end `0x011481F0` if nonnull |
| 8..15 | no payload interpreted by dispatch | default skip |

For types0..3 the applies run targets -> fixed state -> streams -> shaders.
Topology comes from shader-binding dword0 bits21:28:0 point list,1 line
list,2 line strip,3 triangle list,4 triangle strip, mapped to D3D types1..5;
counts are n,n/2,n-1,n/3,n-2. Unknown topology follows the actual switch
default, not an invented topology. Indexed NumVertices uses cached stream
extent minus base vertex. Instancing sets stream frequency to
`0x40000000|count` for vertex streams or `0x80000001` for instance streams,
adjusts instance-stream offsets, then restores frequency1 and offsets.
These details are at draw sites listed in Appendix A.

When sRender+0x30 is nonzero, adjacent compatible type1 records can be
coalesced. The compare covers target/state/shader/stream pointers, base
vertex and index continuity; it is not safe to equate raw type1 records
with DrawIndexedPrimitive calls (`0x00DAB5B0..0x00DAB6AC`). Type5 also
coalesces matching adjacent copies. Type4's float-to-byte exactness test
is observable: do not replace every type4 with a hardware clear.

Bindings retained by draw records (apply consumers and cache-copy sizes
`0x011F0ED0` agree): target block0x5C bytes = RTV[4]+DSV at+0x10,
flags+0x14, two viewport rectangles+0x18/+0x28, two scissor rectangles
+0x38/+0x48, target dimensions u16s+0x58/+0x5A. Fixed block0x14 bytes =
blend factor+0, stencil reference/orientation+4, BlendState+8,
DepthStencilState+0xC, RasterizerState+0x10. Stream block0x30 bytes =
VB*[4]+0, stride bytes+0x10, offsets[4]+0x14, instance flags[4]+0x24,
IB*+0x28; +0x2C is not read by the reviewed stream apply. Shader block
is variable: 12-byte header followed by values/pointers (section5).

### CommandCache object

Separate from the executor's 0xE28-byte cache. `0x011F1400` allocates
0x1B0, initializes Resource and calls `0x011F0ED0(this,entries,count)`.
`0x011F0BE0` sizes copied state and collects shader relocation descriptors;
`0x011F0ED0` copies draw types0..3, remaps binding pointers and copies
selected transient constant data. It does **not** retain all command types
as a general-purpose D3D command buffer.

| Offset | Meaning / evidence in 0x011F0BE0 / 0x011F0ED0 |
|---|---|
| +0x10,+0x14 | allocation base, allocated byte size |
| +0x18 | downward-growing payload offset from allocation base |
| +0x1C | eight-byte entry-array pointer |
| +0x20..+0x11F | up to32 shader relocation descriptors, eight bytes each; packed ID in bits1:12 and count above bit12; second dword points to offsets |
| +0x120..+0x19F | u16 indices used by dependency hash 0x011F08C0 |
| +0x1A0 | retained draw count |
| +0x1A4 | low26 dependency count; high6 relocation-descriptor count |
| +0x1A8 | bit mask after entry array, initialized to all ones |
| +0x1AC | dependency hash returned by 0x011F08C0 |

`0x011F08C0` decompiles misleadingly as void/empty loops. Assembly
`0x011F08F0..0x011F09F1` actually hashes selected input values masked
with0xFFFFF000 through table0x014075C0 and returns EAX. Hash consumer and
all invalidation paths remain an explicit follow-up, not assumed immutable.

## 4. Frame and plugs

The known frame order is:

1. Main prepares/switches command buffers at `0x00DA9CD0` as above.
2. `sRender::vf06 0x00DAF000` updates sShader2, prepares context at
   sRender+0x134, emits Common, calls camera vf09 for scene/viewports,
   then Overlay and SysMenu, copies to display and handles SubDisplay.
   It invokes `0x00FEE130` to sort/merge commands and publishes pointer/count
   to the selected +0x433DD8/+0x433DE0 pair.
3. Camera vf09 `0x00DC4770` walks eight 400-byte viewport records,
   selecting active views matching display. It calls scene setup
   `0x00FF63B0(context,Scene,1,name)`, then the unit renderer and restores
   context. The singleton0x018D0B6C is **sCamera**, not sRender or sScene
   (constructor `0x00DC4540`).
4. Executor `0x00DAB480` calls BeginScene, follows sorted/nested entries,
   EndScene, then clears nonzero RT slots, depth, indices, four streams,
   shaders and textures (`0x00DA6EA0`). It does not call Present itself.
5. Main `0x0041DD60` calls `0x00DAED10`: drains/waits pending execution,
   device Present at `0x00DAEE53`, then additional swap-chain Presents.
   Device-lost return sets reset bit0x10. Other device Present sites are
   lifecycle helpers, not evidence of multiple ordinary frame Presents.

See [FRAME.md](FRAME.md) for simulation scheduling around these calls.
Producer work is not limited to the main thread: `0x00FEE130` queues
sort tasks (`0x00FEE0F0`) through sMain's job array and waits with
`0x00DBE560(2)`. That is distinct from the D3D consumer worker.

### Scene target/pass tables and sorted order

Scene ctor `0x011488C0` constructs texture resources and fills24 pass
slots: four color Texture* values per pass at `Scene+0x18+pass*0x10`,
depth Texture* at `+0x198+pass*4`, auxiliary texture at `+0x1F8+pass*4`.
It also stores working textures starting around+0x258 and configuration
near+0x2C8. `0x00FF1090` changes context+0x164 low5 pass ID, extracts the
first RTV/DSV from those textures (+0x44/+0x48), and binds through
`0x00FEFB90` / `0x00FF00A0`. Scene flags alter clear/viewport behavior.

The scene setup at `0x00FF63B0` emits passes2,3,6,4,7,8,9,10,11,17,18,
19,20,22 on its conditional branches (20 can be revisited with force1).
That is **source emission order**, not final execution order.
`0x00FEDD60` stable-merges eight-byte entries by ascending unsigned first
dword. The type5 producer `0x00FF23D0` explicitly places pass in the
top5 sort-key bits and `(context+0x178)+(context+0x164>>5)` in low27.
Other commands can have specialized lower-key construction; preserve the
original sorted entry stream rather than reconstructing a pass list.

### What the Plug names do and do not prove

The strings BloomPlug0x0158C208, BokehPlug0x0158C214,
ColorCorrectPlug0x0158C220, ColorFogPlug0x0158C234,
ImagePlanePlug0x0158C264, ToneMapPlug0x0158C2A4,
ShadowPlug0x0158C800, TVNoisePlug0x0158C80C are registered by
`0x00740860 -> 0x00CF5D60 -> 0x00CF5C40` as named allocation/group
records. They have **no class entries in the available DTI registry**.
The string registration order is not render order, and assigning DTI
vtables to those names would fabricate evidence.

The actual reflected filter consumers establish these useful order facts:

| Consumer | Static scheduling evidence |
|---|---|
| uBloomFilter vf11 `0x01033CA0` | requires Scene+0x290; selects pass0x13 and subpriority1 (`context+0x164 |=0x20`), then emits bloom work via0x01032B20; later generic filter scheduling can restore configurable order |
| uColorCorrectFilter vf11 `0x010460C0` -> `0x010310D0` | conditional prepass selects0x13, priority1000 (`32000>>5`); later uses filter-configured pass/order |
| uToneMapControl vf11 `0x01035E50` -> `0x01034620` | requires Scene+0x290; selects0x13, priority10000 (`320000>>5`) |
| uTVNoiseFilter vf11 `0x0100C730` | uses filter scheduler0x0100B7A0 and copies Scene+0x24C |
| uImagePlaneFilter vf11 `0x01017DB0` | inspects current pass0xB, then emits its selected draw/copy path |
| uBokehFilter vf11 `0x0103E6B0` | creates several target changes and fullscreen operations; order cannot be reduced to one unconditional pass |
| uColorFogFilter vf11 `0x00BFC7D0` | inherited no-op slot in registry; actual effect path cannot be inferred from the Plug string |

`0x0100B7A0(filter,context)` maps filter u16+0x30 through table0x0143BF4C
to pass, uses filter+0x34 as priority, and for selector6 includes
filter+0x38 shifted20. This is positive evidence for configurable order.
Within the same sort-key base, the specific pass0x13 prepasses above sort
Bloom -> ColorCorrect -> ToneMap. This is **not** a universal ordering
of every draw those filters emit. Exact active Shadow/Bokeh/Fog/Noise
ordering for gameplay requires tracing their producers or a head-run
capture. L111 command instrumentation is the next measurement boundary.

## 5. Shader binding

`0x00DAA8B0` binds declaration (`vtable+0x15C`), vertex shader (`+0x170`),
pixel shader (`+0x1AC`) from a shader binding's metadata object at `+8`.
That metadata carries declaration at `+0x18`, VS at `+0x10`, PS at `+0x14`.
It emits constant-register calls, including VS float `+0x178` and PS float
`+0x1B4`; these are not native constant-buffer bindings.

sShader2 singleton0x018D12CC is written by ctor0x00E121D0. It loads
`system\\shader\\ShaderPackage` as rShader2, builds the indexed object
table with0x00E11E30, and loads platform packages via0x00E0E470. The
rShaderPackage loader0x010082D0 validates SPK magic0x004B5053, version
0x22 and sizes, relocates data at resource+0x84. rShaderCache loader
0x0117D310 validates SCH magic0x00484353 and versions0x35/0x19,
relocates arrays, checks compatibility and retains accepted data at+0x60.
Neither resource is itself a GPU constant buffer.

Shader factories0x00E0FF00 / 0x00E10000 search sShader2's 12-byte cache
records at+0x803C, count+0xA8040, for kind1/2, record key and variant bit.
Misses create VS/PS from the token pointer and append; this **is** proven
shader-object caching. Uncached helpers0x00E0FEC0 / 0x00E0FFC0 also exist.
`0x00E12850` combines shader handles, declaration and binding descriptors;
its decompiled argument list is unreliable, so a native factory hook
needs assembly ABI recovery first. Shader token bytes stay external.

Shader binding header: dword0 alpha-test/topology flags, dword1 shader
identity fields, +8 compiled metadata pointer, +0xC indexed values/pointers.
Metadata+4 descriptor-array pointer; dwords+8/+0xC encode VS/PS sections:
low8 is section size in dwords, bits8:11 sampler count,12:18 float upload
count,19:22 integer upload count,23:27 boolean upload count,28:31 derived
float upload count (`0x00DAA8B0`). Metadata+0x1C is source binding-ID
array and+0x20 its count (`0x011F0BE0`), not an array of native UBOs.

Each four-byte constant descriptor encodes bit0 integer-to-float conversion,
bits1:5 binding value index, bits6:15 source dword offset, byte2 destination
register, byte3 register count. Value pointer low4 bits are removed; for
float uploads a nonzero low nibble limits count to `nibble*16`. Integer and
boolean sections use their own D3D setters, not float reinterpretation.
The converted-float path casts signed int components to float and uploads
one vec4 at a time. Derived descriptors use bits1:7 for binding index and
byte1 kind:3 reciprocal texture dimensions,7 dimensions; count/register
still come from bytes3/2. Sources: VS calls0x00DAACAA/0x00DAACCB/
0x00DAAF1B, PS0x00DAB188/0x00DAB1A5/0x00DAB3EB, and surrounding code.

Sampler descriptors: low12 sampler index, bits12:21 sampler-object value
index, bits22:31 texture-object value index. Vertex stages map to D3D
samplers257..260; pixel stages use0..15. State payload+0x30 is applied by
0x00DA7250; texture+0x34 supplies COM handle. Alpha-test enable is binding
flags19 AND20, comparison comes from bits3:10 via0x01445A70, reference
bits11:18; native shaders/pipelines must preserve it. On unchanged shader
handles, pointer comparison of old/new value arrays skips uploads. A
pointer's content can therefore require an invalidation/copy discipline;
blindly treating the addresses as immutable byte contents is unsafe.

## 6. Hook candidates, ranked

1. **Command executor `0x00DAB480`**: one complete command stream, state
   objects still intact. Must preserve nested commands, queries, clears,
   copies, instancing and resource lifetime; use the verified root/context
   contract above. Expected frequency: one invocation per submitted stream,
   thousands of raw draws per gameplay stream (measurement in renderer plan;
   this investigation has not counted a frame).
2. **Object-aware applies `0x00DAA5F0` / `0x00DAA8B0`**: fixed-state and
   shader bindings remain intact. Must preserve dynamic blend factor,
   stencil reference, rasterizer orientation, register uploads and texture
   bindings. Frequency is per command before identity-based early return,
   not measured calls per frame.
3. **Packed-state applies `0x00DA7510` / `0x00DA77D0` / `0x00DA7B30` /
   `0x00DA7250`**: lower but original resource identity has already been
   stripped to a payload pointer. Useful as an oracle/transition boundary;
   less attractive as permanent native pipeline-cache keys.

4. **Resource creation `0x00E11E30`, `0x01105240`, `0x01129030`,
   `0x01129460` and shader factories**: establish persistent native objects
   without rediscovering their meaning per draw. Expected frequency is
   load/reload/reset or dynamic-buffer events, not every draw. Must pair
   creation with mutation/release paths; do not intercept only constructors.
5. **Scene builder `0x00FF63B0`**: retains semantic passes, but is a large
   configuration-dependent producer with game-visible side effects. It is
   higher and riskier than the command/apply boundary; frequency per active
   camera viewport. Leave original producer logic in place initially.

### Verified ABI boundary

Arguments are guest32 addresses; ESP below means function-entry ESP.
`__stdcall` describes the observed stack-only inputs/callee cleanup where
ECX is not needed; original C++ type declarations are not recovered.

| Entry | Contract | Native implementation must reproduce |
|---|---|---|
| 0x00DAB480 | ECX=sRender, no stack args, RET0 | entire executor/context behavior; aligned stack writes if original applies remain |
| 0x00DA6DA0 | ECX=sRender; [ESP+4]=ctx; RET4 | zero0xE28, root traversal, initial render states |
| 0x00DA6FE0 | stack(ctx,target), RET8 | RT/DS identity, viewport clipping, scissor intersection, cache writes |
| 0x00DAA5F0 | stack(ctx,fixed), RET8; preserves/forwards incoming ECX internally | identity/hash suppression, dynamic factors/reference/orientation, cache writes |
| 0x00DAA7C0 | stack(ctx,streams), RET8 | four stream tuples, indices, max vertex extent and cache writes |
| 0x00DAA8B0 | ECX=sRender; stack(ctx,shader-binding), RET8 | declaration/VS/PS, alpha test, textures/samplers, constants and cache writes |
| 0x00DA7510 / 0x00DA77D0 / 0x00DA7B30 | stack(ctx,new-payload,old-or-null), RET12 | exact packed-state delta application; resource identity already lost |
| 0x00DA7250 | stack(ctx,stage,new-payload,old-or-null), RET16 | sampler deltas and global-option branch |

Assembly entry/return checks were made for DAB480, DA6FE0, DAA5F0,
DAA7C0, DAA8B0, DA7510 and DA7250; DA6DA0/DA77D0/DA7B30 also appear
in the reviewed owner disassemblies. Caller `0x00DAB480` restores ECX
to sRender around object-aware calls; preserve that incoming register in
an ABI adapter even for helpers that do not consume it.

These are research contracts, not implemented/tested replacements. L112
owns the first executor port. Full-register/stack differential replay and
head-run D3D9-vs-native frame comparison remain required. In particular,
replacing an apply with a pure host function while returning to the lifted
executor must maintain guest cache fields and stack-visible side effects.

## 7. Remaining evidence gaps

- This census enumerates all **identified** device owners; arbitrary
  interprocedural device aliases are not exhaustively excluded.
- Resource padding/unused flags, input descriptor fields not consumed by
  declaration construction and named-texture eviction remain unresolved.
- State payloads appear creation-only on reviewed paths, but there is no
  global immutability proof or demonstrated descriptor interning.
- Conditional filter order is mapped above; a single total Plug order
  would be false precision. Shadow and remaining configured filter routes
  need producer tracing and a concrete gameplay command capture.
- CommandCache patch/invalidation consumers and shader-factory exact ABI
  need follow-up before those boundaries are ported.

Next action: correlate L111's head-run command counts/target tuples with
the executor and apply boundaries here, then differential-test L112's
executor port. No FPS or image-equivalence result is claimed by L108.

## 8. Companion command instrumentation (L111)

L111 (`task/L111-executor-stats`) adds an opt-in observer at
`0x00DAB480`, with Device9 slot17 / SwapChain9 slot3 successful-Present
frame boundaries. This is a companion implementation report from
`docs/tasks/L111-executor-stats.md`, not an executable-derived claim or
code included in L108. Its validation/merge status belongs to that brief.

`DDRUNTIME_NDRAW_STATS=1` enables it; default is off.
`DDRUNTIME_NDRAW_STATS_EVERY=N` reports every Nth successful Present
(default60; invalid/zero uses60); each report covers that frame only.
Use N=1 for capture correlation. The composable `ddports::ensure_with`
wrapper observes before calling the retained port/mod/original executor.
It is intended to preserve guest memory/registers and original execution.

Reports include all16 command-type counts, raw draw/element totals,
distinct state/shader/stream/texture identities, draws by four-RT-plus-DS
surface tuple, and ordered target runs including clears/copies. Identity
counts are not content deduplication. These count submitted records:
`0x00DAB480` type1 merging, type4 clear-to-draw fallback and type5 copy
suppression can produce different D3D call totals. Compare ordered target
surface tuples and type mix before interpreting differences as errors.

Bad pointers, overflow, nesting beyond256 or over one million records
stop observation and mark counts partial; they do not stop the original
executor. These are diagnostic limits, not recovered game constraints.
L108 has not run this observer or compared its output with a capture.

## 9. L117: executable-verified state semantics for the native path

`crates/mtstate` is a dependency-free semantic decoder. It accepts guest
memory through `Memory`, returns ordered `Call { slot, args, data }`
records, and updates the apply functions' context cache fields. `args`
excludes COM `this`; `data` copies pointer arguments at call time, removing
only transient addresses. This is **not** a guest ABI override: register,
x87 and original stack-frame preservation belongs to an adapter such as
L112. It does not replace rendering or create a wgpu pipeline itself.

`descriptor::{raster,depth,blend,sampler}` converts the retained descriptors
to the packed payload. `packed` plus the public ordered `Field` tables
implements full/delta application. `fixed`, `streams`, `targets`, `shader`
decode the object-aware boundaries; `layout` builds and combines declarations.
Descriptor and declaration enum/range errors are explicit, not substitutes
for supported formats. The shader decoder accepts derived kinds3/7 with
count0/1: other forms return `DecodeError`; larger counts or unknown first
kinds can read unspecified original stack data. Stage indices and pointers
otherwise require valid objects.

### Original-instruction proof and limits

Generator: `research/native/state_vectors.py`, default private output
`E:/ddda-re/notes/native-L117/state-vectors.json`. It maps the user's unpacked
exe, constructs guest graphs, runs constructors/applies in Unicorn, and
records fake-device vtable calls with S_OK returns. Original helper logic
runs, including the hash, memcpy, memset and declaration emitter. Only
InputLayout allocator lookup/allocation is mocked, explicitly at0x00CF5E20
and the allocator's+0x20; declaration creation returns a synthetic handle.
The JSON records the executable SHA-256. No private data is committed.

**3,584 cases / 17,928 D3D9 calls** pass ordered call/data comparison and
all selected cache/payload/constructed-layout word comparisons. There are
256 cases each for fourteen categories: four state constructors, four
packed applies, fixed/streams/targets/shader, InputLayout constructor and
declaration assembly. Cases include single-bit payload sweeps, null/equal/
different old values, equal/different hashes, alpha-test transitions,
sampler filtering options0..6, signed borders, exceptional float conversion,
viewport/scissor clipping, both cull variants, integer/boolean/float and
derived constants, tagged upload limits and four-stream declarations.
This is finite differential coverage, not proof of every possible input.

Reproduce from this worktree (PowerShell; no game or GPU):

```powershell
E:/ddda-re/tools/pyenv/Scripts/python.exe E:/ddda-re/tools/heavy.py codex-10 -- E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/state_vectors.py
$env:CARGO_INCREMENTAL='0'
$env:CARGO_TARGET_DIR='C:/ddda-targets/L117'
$env:CARGO_BUILD_JOBS='4'
$env:MTSTATE_REQUIRE_VECTORS='1'
cargo test -p mtstate -- --nocapture
```

Use `--exe`/`--out` for another private installation/output and
`MTSTATE_VECTORS` for the test input. Without vectors the differential test
reports a skip unless `MTSTATE_REQUIRE_VECTORS` is set; four synthetic
contract tests remain runnable. MXCSR=0x1F80, x87 CW=0x037F.

### Fixed descriptors to packed state to wgpu

Here `dN` means descriptor dword at object+0x10+4N; `wN` means packed
payload dword. Bit ranges are inclusive. Section2 gives every packed bit
and numeric D3D state; these tables give the complete constructor mapping
and native implications. wgpu names refer to the repository's29.0.4 API,
also checked against its local `wgpu-types` source. D3D state names follow
[Microsoft's render-state reference](https://learn.microsoft.com/en-us/windows/win32/direct3d9/d3drenderstatetype).

| Rasterizer descriptor / source0x0117C6E0 | D3D payload at+0x20, flipped+0x2C | wgpu meaning / requirement |
|---|---|---|
| d0[0:2], lookup0x01445A2C (valid0,1) | w0 byte0 FILLMODE8:2 wire,3 solid | PolygonMode::Line/Fill; line polygon feature required |
| d0[3:5], lookup0x01445A20 (0,1,2) | w0 byte1 CULLMODE22:1 none,2 CW,3 CCW | With FrontFace::Cw, None/Front/Back; preserve viewport coordinate convention |
| d0[6] orientation | swaps cull2/3 in first payload; second payload has opposite orientation | pipeline cull variant, selected by fixed-binding bit8 |
| d0[7] | w0[16] CLIPPING136 | no one-field equivalent for all D3D9 clipping; `unclipped_depth` concerns depth only |
| d0[8] | w0[17] SCISSORTESTENABLE174 | choose clipped scissor or whole render area dynamically |
| d0[9] | w0[18] MULTISAMPLEANTIALIAS161 | not attachment sample count; native raster/MSAA behavior needs capability/parity validation |
| d0[10] | w0[19] ANTIALIASEDLINEENABLE176 | no direct portable wgpu line-AA switch |
| signed d1 | w1 DEPTHBIAS195 = f32(i32(d1)) * f32::from_bits(0x33800001) | preserve D3D9 float bias; not the wgpu integer depth-format-unit bias |
| d2 | retained, not emitted | do not apply a guessed depth-bias clamp |
| d3 | w2 SLOPESCALEDEPTHBIAS175, unchanged float bits | slope term; use same depth-bias implementation as D3D9 oracle |

The exact multiplier is stored at0x01445A34 and multiplied at0x0117C70E;
it is **one float ULP above2^-24**. ddgpu's existing D3D9 path implements
bias in the pixel shader because Vulkan hardware constant-bias units differ.
wgpu `DepthBiasState.constant` uses depth-format units, so plugging d1
directly into the native hardware field is not an established equivalent.
See [wgpu DepthBiasState](https://docs.rs/wgpu/latest/wgpu/struct.DepthBiasState.html).
L110's native descriptor must keep that distinction visible.

| DepthStencil descriptor / source0x0117C8E0 | D3D payload+0x18 | wgpu equivalent |
|---|---|---|
| d0[0],d0[1] | w0[0],w0[1] ZENABLE7,ZWRITEENABLE14 | depth_compare / depth_write_enabled; when depth disabled use Always and no writes |
| d0[2:5] | table0x01445A70 -> w0[2:5] ZFUNC23 | CompareFunction via mapping below |
| d0[6] | w0[6] STENCILENABLE52 | enable stencil faces/masks; disabled state is inert |
| d0[7:14],d0[15:22] | w0 bytes1/2 STENCILMASK58,STENCILWRITEMASK59 | StencilState read_mask/write_mask |
| d0[23:26],d0[27:30],d1[0:3],d1[4:7] | table -> front fail53,zfail54,pass55,func56 | front StencilFaceState (CW with FrontFace::Cw) |
| d1[8:11],d1[12:15],d1[16:19],d1[20:23] | table -> CCW186,187,188,189 | back StencilFaceState |
| raw descriptor front tuple differs from back tuple | w0[7] TWOSIDEDSTENCILMODE185 | if false use front tuple for both faces; if true use CCW tuple for back |
| fixed-binding byte+4 (not descriptor) | STENCILREF57 | RenderPass::set_stencil_reference |

Table0x01445A70 maps indices0..7 to D3D values1..8. As comparisons these
mean Never,Less,Equal,LessEqual,Greater,NotEqual,GreaterEqual,Always; as
stencil operations they mean Keep,Zero,Replace,IncrementClamp,
DecrementClamp,Invert,IncrementWrap,DecrementWrap. The constructor compares
raw face descriptors even if stencil is disabled. No guessed face swap
is performed by the depth apply when the raster cull variant changes.

| Blend descriptor / source0x0117CB70 | D3D payload+0x20 | wgpu equivalent |
|---|---|---|
| d0[1] | w0[0] ALPHABLENDENABLE27 | ColorTargetState.blend None/Some |
| d0[9:16],d0[17:24] | table0x01445AD0 -> SRCBLEND19,DESTBLEND20 | BlendComponent color factors |
| d0[25:31] | table0x01445B48 -> BLENDOP171 | color operation |
| d1 byte0,byte1,d1[16:22] | factor/op tables -> SRCBLENDALPHA207,DESTBLENDALPHA208,BLENDOPALPHA209 | independent alpha component |
| d2 bytes0..3 low nibbles | COLORWRITEENABLE168,190,191,192 | per-target ColorWrites RGBA bits |
| descriptor bit0, other unlisted bits, d3 | retained but no emitted state | no inferred alpha-to-coverage, independent blend, or extra pipeline feature |
| fixed-binding dword+0 RGBA bytes | red/blue byte swap -> BLENDFACTOR193 ARGB | dynamic set_blend_constant, normalized RGBA |

Factor indices0..12 map to D3D1..11,14,15 (table0x01445AD0): Zero,One,
Src,OneMinusSrc,SrcAlpha,OneMinusSrcAlpha,DstAlpha,OneMinusDstAlpha,
Dst,OneMinusDst,SrcAlphaSaturated,Constant,OneMinusConstant. Operation
indices0..4 map to D3D1..5: Add,Subtract,ReverseSubtract,Min,Max. Min/Max
must use wgpu One/One factors since D3D9 ignores factors for these ops.
Separate alpha is globally enabled: context init0x00DA6DA0 emits
RS25=1,24=0,15=0,206=1,52=0,206=1, in that order (206 twice).

Object apply0x00DAA5F0 first skips identical **binding** pointers, then
compares state identities/hashes. Distinct pointers with equal+0xC hashes
skip packed application. Blend factor is only checked inside the branch
where the BlendState pointer changes; a new fixed binding with only a
factor change does **not** issue193. Stencil-reference changes are checked
independently of the DepthStencilState pointer. Raster bit8 changes force
variant comparison even with the same RasterizerState pointer. All cache
writes and these suppression rules are covered by vectors. A host adapter
must model the actually applied state rather than blindly replacing all
dynamic values from the latest binding.

### Sampler descriptor and dynamic texture state

| Descriptor / source0x0117CD70 | D3D payload+0x30 / source0x00DA7250 | wgpu equivalent / limit |
|---|---|---|
| d0[8:10],[11:13],[14:16] | table0x01445B48 -> w0[0:2],[3:5],[6:8], ADDRESSU/V/W1/2/3 | indices0..4 become Repeat,MirrorRepeat,ClampToEdge,ClampToBorder,MirrorOnce; last has no direct wgpu address mode |
| float d4..d7 RGBA | SSE multiply255, CVTTSS2SI, low bytes -> w1 ARGB BORDERCOLOR4 | standard border colors supported; arbitrary color needs explicit emulation/fallback |
| d0[0:7] | w0 MAG5[9:12],MIN6[13:16],MIP7[17:20] | filters below |
| d0[17:21] | w0[21:25] MAXANISOTROPY10 | anisotropy_clamp, respecting device/linear-filter constraints |
| float d2 | x87 truncate-to-i64 then low4 bits -> w0[26:29] MAXMIPLEVEL9 | most-detailed mip allowed, normally lod_min_clamp; not lod_max_clamp |
| d1 | w2 MIPMAPLODBIAS8 unchanged | shader sampling bias; no field in wgpu SamplerDescriptor |
| d3 | preserved, not emitted | no inferred max-LOD clamp |
| renderer byte+0x31 and Texture+0x20 bit30 / +0x2C bit19 | pixel SRGBTEXTURE11=bit30 && !bit19, on every listed sampler when renderer byte is nonzero | linear/sRGB texture-view selection; not a sampler-object field |

Filters0..8 produce `(mag,min,mip)` of `(1,1,1)`, `(1,1,2)`, `(2,1,1)`,
`(2,1,2)`, `(1,2,1)`, `(1,2,2)`, `(2,2,1)`, `(2,2,2)`, `(2,3,2)`.
Filters9..17 repeat0..8; no comparison-sampler state is issued. D3D1 means
point,2 linear,3 anisotropic. Other filter selectors leave all three zero.
Those invalid/unsupported combinations should not silently become new
native behavior. CVTT invalid/overflow produces0x80000000, hence border
byte0; the mip FISTP invalid result also has low4 bits0.

Only the **non-null old payload** branch consults global0x018D1D20+0x34
when new MINFILTER is3. Option1 sets all three filters to2; options2..5
set anisotropy to2/4/8/16; other values leave it unchanged. Comparisons
use the old object's **unmodified** packed fields, not its previously
overridden effective values. This can emit calls even for equal raw
payloads, or retain a previous effective setting when raw comparison is
equal. Cache effective native sampler state after the original comparisons.

These sampler limitations follow the pinned wgpu API and the existing
ddgpu sampler path; see [wgpu sampler fields](https://docs.rs/wgpu/latest/wgpu/type.SamplerDescriptor.html)
and [D3D9 sampler states](https://learn.microsoft.com/en-us/windows/win32/direct3d9/d3dsamplerstatetype).
Do not add SetTextureStageState: none of the decoded paths issues slot67.
They use programmable shaders, sampler states and texture bindings.

### InputLayout and shader metadata

InputLayout+0x10 is a generated-element pointer, **not a descriptor inline
at+0x10**. Source records are at+0x18, count+0x1C. Constructor0x0112AAB0
marks a256-byte array with source second-dword type[6:10] for
count[11:17]*type_width bytes beginning at byte offset[22:30]. Later
records overwrite earlier bytes. It scans four-byte groups, taking the
first nonzero type and coalescing compatible groups, then0x01129750 emits
D3D elements. Source semantic dword0 is not used for these usages. Source
flag31 is tracked separately, copied into byte+0x24 from the last scan
group, but does not affect emitted declaration types in the PC emitter.

| Source type / width table0x01441BD8 | Generated D3DDECLTYPE | wgpu vertex format |
|---|---|---|
| 1 /4 bytes per component | FLOAT1..4 (0..3), groups at most4 | Float32/Float32x2/x3/x4 |
| 2 /2 | FLOAT16_2/4 (15/16), groups2/4 | Float16x2/x4 |
| 3,4 /2 | SHORT2/4 (6/7) | Sint16x2/x4 plus numeric conversion in shader |
| 5,6 /2 | SHORT2N/4N (9/10) | Snorm16x2/x4 |
| 7,8 /1 | UBYTE4 (5), groups4 | Uint8x4 plus numeric conversion |
| 9,10,13 /1 | UBYTE4N (8), groups4 | Unorm8x4 |
| 11,12,14 /4 | UBYTE4N (8), one declaration per component | Unorm8x4 (packed four-byte source) |

The emitter sets Method=0, stream=0, byte offset from the coalesced group,
and usages by **generated index**: POSITION0, NORMAL0, TANGENT0,
BINORMAL0, TEXCOORD0..11 (embedded table0x01441B38/3C). It appends
`{0x000000FF,0x00000011}`. The object retains generated count without
terminator and rounded source extent at+0x20. Inputs exceeding256 bytes
or16 generated entries are rejected by the semantic API; original local
arrays/tables do not establish safe behavior beyond those limits.
[D3D declaration types](https://learn.microsoft.com/en-us/windows/win32/direct3d9/d3ddecltype)
also require missing vector components to be expanded as0,0,1 by the shader
interface where applicable, rather than changing the raw buffer layout.

0x00E0FCC0 receives four source IDs; each nonzero ID resolves the layout
through `[sShader2+0x3C+(id&0xFFF)*8]`. It copies generated elements, assigns
the actual stream0..3 and **reassigns usages by combined index**, then
calls CreateVertexDeclaration(slot86). Its second argument is a nonzero
gate in this path, not the per-element semantic source. If either pointer/
gate is zero it returns0 without creating a declaration. `layout::declaration`
models the valid creation branch after IDs are resolved. Shader metadata
owns the returned COM handle, bound by SetVertexDeclaration(slot87).

| Shader binding / metadata field, source0x00DAA8B0 | D3D9 calls | Native use |
|---|---|---|
| binding+8 -> metadata+0x18/+0x10/+0x14 | declaration87,VS92,PS107 | vertex layout and linked shader module identities |
| binding d0 bits19 AND20, comparison[3:10], reference[11:18] | ALPHATESTENABLE15,ALPHAFUNC25,ALPHAREF24 | fragment shader discard using translated comparison/reference, no native fixed alpha test |
| metadata+4 descriptor array; +8/+0xC VS/PS summaries | section traversal (see section5) | persistent binding layout, not a copied giant constant buffer |
| sampler descriptor low12 stage,[12:21] sampler value index,[22:31] texture value index | sampler69; texture65 from Texture+0x34; VS stage+257, PS stage unchanged | stage bind group with effective sampler and texture view |
| ordinary descriptor bit0,[1:5],[6:15],byte2,byte3 | conversion flag,value index,source dword offset,destination register,count | stage-local register upload plan; float slots94/109, int96/111, bool98/113 |
| value pointer low4 tag (float only) | count=min(descriptor count,tag*16) if tag!=0; pointer always masked~15 | preserve upload bounds and offsets |
| bit0 int-to-float conversion | signed32->f32 for each component, one vec4 call per register | numeric conversion, not bit reinterpretation |
| derived descriptor [1:7],byte1 kind3/7 | texture dimensions reciprocal/direct: width+0x20 u16,height+0x22&0x3FFF,depth+0x26&0x7FF,zero fourth component | one derived constant vec4 at descriptor register |

The PS descriptor array begins at VS summary low8*dword size, even if
the number of used descriptors differs. Sections then advance in sampler,
float,int,bool,derived order. Equal VS handles allow comparison of old
binding values at+0xC; equal PS handles allow this only if VS also stayed
equal. Thus a VS change invalidates the PS value-comparison shortcut.
Unchanged value pointers suppress uploads, regardless of pointed-to bytes.
Mutable source memory must follow the engine's binding invalidation rules.
Unused VS stages with nonzero cached **sampler** pointer issue SetTexture0
and clear their cached texture pointer; sampler pointer is retained. There
is no analogous PS unused-stage clear loop in this function. The native
API must not invent one. No constant or texture bytes are embedded here.

### Targets, views, streams and dynamic state

| Object field / source | D3D9 operation | wgpu equivalent / condition |
|---|---|---|
| target binding+0..+0xC RTV pointers; +0x10 DSV | changed view identity -> SetRenderTarget37 / SetDepthStencilSurface39 using view+0x14 (or0) | retained color/depth attachment views and render-pass boundary |
| view+0x10 parent,+0x1C subresource,+0x24 format (only proven initialized paths) | identity/provenance behind surface | backing texture/subresource view, actual format/sample count in pipeline key |
| target+0x14 bit9 | select viewport at+0x18 vs+0x28, scissor at+0x38 vs+0x48 | select same dynamic rectangles |
| target+0x58/+0x5A u16 dimensions | clamp viewport left/top>=0,right<=width,bottom<=height; inverted axis becomes0,0 | SetViewport47 x,y,width=right-left,height=bottom-top,minZ0,maxZ1 |
| selected scissor rectangle | intersect with clipped viewport; inverted axis becomes0,0 | SetScissorRect75; native set_scissor_rect only when scissor test enabled |
| stream binding+0..+0xC VB pointers | VB+0x20 COM handle; null gives handle/offset/stride0 | set_vertex_buffer per slot, including explicit unbind semantics |
| stream binding bytes+0x10..13; dwords+0x14..20 | stride and byte offset -> SetStreamSource100 when tuple changes | VertexBufferLayout.array_stride and buffer slice offset |
| stream binding+0x28 IB pointer | IB+0x20 -> SetIndices104; null0 | set_index_buffer; original IB creation uses16-bit indices |
| VB+0x14 byte length, supplied offset/stride | ctx+0xE20=max(unsigned(length-offset)/stride) across non-null streams | preserve draw extent metadata; subtraction wraps, zero stride faults in original |

Any RT/DS change resets cached viewport coordinates to the four globals
at0x018D04E4..0x018D04F0 before viewport comparison, even when dimensions
appear unchanged. Scissor and viewport calls compare final rectangles,
not merely target-binding identity. Depth-view binding does not itself
enable depth/stencil tests. Vertex-instance frequency is set around draw
types2/3 by the executor, not0x00DAA7C0; incorporate it into native step mode
and draw instance count. Dynamic blend factor/reference/scissor/constants
stay outside immutable pipeline identity; cull variant, target formats,
layout/stride/step mode and shader alpha-test strategy can affect that key.

Native integration still requires lifetime/generation-safe resource handles,
the explicitly unresolved wgpu equivalents above, and head-run frame image
comparison. Passing the D3D9 call oracle does not establish GPU equivalence.

## Appendix A. Device call sites by owning function

216 identified instruction sites, 46 methods, 42 owners. This expands
L20's 213-site scan with assembly-verified `0x00DA7035`, `0x00DA7094`
and `0x00DAA878`. All addresses in this table are calls, not merely loads
of a method pointer. Numbers in parentheses are zero-based COM slots;
vtable byte offset is slot*4. Repeated calls on alternate branches are
listed separately, loops count once. Ownership is by function extent.

Scope: direct device flow from sRender+0x98 or the executor cache+4 in
the renderer, shader factories, texture/buffer/view/query classes. The
42 owner disassemblies were reviewed for indirect calls; surface, query,
swap-chain, resource and allocator vtables are not device calls. A whole-
program alias proof has not been performed. L20's broader offset scan
also produces false positives (e.g. `0x00602320` in `0x006019F0` loads
an unrelated object+0x98); offsets alone cannot establish device identity.

| Owner | Device method (slot) | Call instructions |
|---|---|---|
| `0x00DA6D00` | SetGammaRamp (21) | `0x00DA6D8B` |
| `0x00DA6DA0` | SetRenderState (57) | `0x00DA6E31`, `0x00DA6E43`, `0x00DA6E55`, `0x00DA6E6A`, `0x00DA6E7C`, `0x00DA6E91` |
| `0x00DA6EA0` | SetRenderTarget (37) | `0x00DA6EB6`, `0x00DA6EC5`, `0x00DA6ED4` |
| `0x00DA6EA0` | SetDepthStencilSurface (39) | `0x00DA6EE1` |
| `0x00DA6EA0` | SetTexture (65) | `0x00DA6F6C`, `0x00DA6F84`, `0x00DA6F96`, `0x00DA6FA8`, `0x00DA6FBA`, `0x00DA6FCC` |
| `0x00DA6EA0` | SetVertexShader (92) | `0x00DA6F47` |
| `0x00DA6EA0` | SetStreamSource (100) | `0x00DA6F01`, `0x00DA6F14`, `0x00DA6F27`, `0x00DA6F3A` |
| `0x00DA6EA0` | SetIndices (104) | `0x00DA6EEE` |
| `0x00DA6EA0` | SetPixelShader (107) | `0x00DA6F54` |
| `0x00DA6FE0` | SetRenderTarget (37) | `0x00DA7035` |
| `0x00DA6FE0` | SetDepthStencilSurface (39) | `0x00DA7094` |
| `0x00DA6FE0` | SetViewport (47) | `0x00DA7172` |
| `0x00DA6FE0` | SetScissorRect (75) | `0x00DA723D` |
| `0x00DA7250` | SetSamplerState (69) | `0x00DA728A`, `0x00DA72AD`, `0x00DA72D0`, `0x00DA72E7`, `0x00DA7370`, `0x00DA7393`, `0x00DA73B8`, `0x00DA73D7`, `0x00DA7403`, `0x00DA7422`, `0x00DA7440`, `0x00DA7457`, `0x00DA746E`, `0x00DA7480`, `0x00DA7497`, `0x00DA74AE`, `0x00DA74C5`, `0x00DA74DC`, `0x00DA74F3`, `0x00DA7505` |
| `0x00DA7510` | SetRenderState (57) | `0x00DA7544`, `0x00DA7569`, `0x00DA758C`, `0x00DA75AC`, `0x00DA75CE`, `0x00DA75F3`, `0x00DA7618`, `0x00DA763D`, `0x00DA765E`, `0x00DA7685`, `0x00DA76AC`, `0x00DA76C9`, `0x00DA76E2`, `0x00DA76FA`, `0x00DA770F`, `0x00DA7725`, `0x00DA773E`, `0x00DA7757`, `0x00DA7770`, `0x00DA7787`, `0x00DA77A1`, `0x00DA77BB` |
| `0x00DA77D0` | SetRenderState (57) | `0x00DA7804`, `0x00DA7824`, `0x00DA7846`, `0x00DA7868`, `0x00DA7881`, `0x00DA789A`, `0x00DA78BA`, `0x00DA78D8`, `0x00DA78F4`, `0x00DA7918`, `0x00DA793D`, `0x00DA7964`, `0x00DA7987`, `0x00DA79AE`, `0x00DA79D5`, `0x00DA79F2`, `0x00DA7A07`, `0x00DA7A1D`, `0x00DA7A33`, `0x00DA7A45`, `0x00DA7A57`, `0x00DA7A6C`, `0x00DA7A80`, `0x00DA7A93`, `0x00DA7AAA`, `0x00DA7AC3`, `0x00DA7ADD`, `0x00DA7AF5`, `0x00DA7B0F`, `0x00DA7B29` |
| `0x00DA7B30` | SetRenderState (57) | `0x00DA7B61`, `0x00DA7B7D`, `0x00DA7BA2`, `0x00DA7BC7`, `0x00DA7BEA`, `0x00DA7C0F`, `0x00DA7C28`, `0x00DA7C41`, `0x00DA7C5D`, `0x00DA7C6E`, `0x00DA7C87`, `0x00DA7CA0`, `0x00DA7CB8`, `0x00DA7CD1`, `0x00DA7CE5`, `0x00DA7CF9` |
| `0x00DA9B10` | TestCooperativeLevel (3) | `0x00DA9B1C` |
| `0x00DA9DD0` | Present (17) | `0x00DA9DEB` |
| `0x00DAA4B0` | Release (2) | `0x00DAA539` |
| `0x00DAA4B0` | GetDirect3D (6) | `0x00DAA4F5` |
| `0x00DAA4B0` | GetDeviceCaps (7) | `0x00DAA507`, `0x00DAA528` |
| `0x00DAA4B0` | CreateQuery (118) | `0x00DAA4DB` |
| `0x00DAA5F0` | SetRenderState (57) | `0x00DAA69D`, `0x00DAA722` |
| `0x00DAA7C0` | SetStreamSource (100) | `0x00DAA878` |
| `0x00DAA7C0` | SetIndices (104) | `0x00DAA7FF` |
| `0x00DAA8B0` | SetRenderState (57) | `0x00DAA942`, `0x00DAA998`, `0x00DAA9D3`, `0x00DAAA14` |
| `0x00DAA8B0` | SetTexture (65) | `0x00DAAB34`, `0x00DAAB8D`, `0x00DAAFFA` |
| `0x00DAA8B0` | SetSamplerState (69) | `0x00DAB02C`, `0x00DAB04E` |
| `0x00DAA8B0` | SetVertexDeclaration (87) | `0x00DAA8F2` |
| `0x00DAA8B0` | SetVertexShader (92) | `0x00DAA9FE` |
| `0x00DAA8B0` | SetVertexShaderConstantF (94) | `0x00DAACAA`, `0x00DAACCB`, `0x00DAAF1B` |
| `0x00DAA8B0` | SetVertexShaderConstantI (96) | `0x00DAAD58` |
| `0x00DAA8B0` | SetVertexShaderConstantB (98) | `0x00DAADCF` |
| `0x00DAA8B0` | SetPixelShader (107) | `0x00DAAA50` |
| `0x00DAA8B0` | SetPixelShaderConstantF (109) | `0x00DAB188`, `0x00DAB1A5`, `0x00DAB3EB` |
| `0x00DAA8B0` | SetPixelShaderConstantI (111) | `0x00DAB22C` |
| `0x00DAA8B0` | SetPixelShaderConstantB (113) | `0x00DAB2A3` |
| `0x00DAB480` | StretchRect (34) | `0x00DABF28`, `0x00DABF79` |
| `0x00DAB480` | BeginScene (41) | `0x00DAB4E8`, `0x00DABF8C` |
| `0x00DAB480` | EndScene (42) | `0x00DAB55D`, `0x00DABF4E` |
| `0x00DAB480` | Clear (43) | `0x00DABD00` |
| `0x00DAB480` | DrawPrimitive (81) | `0x00DAB928`, `0x00DABD8A` |
| `0x00DAB480` | DrawIndexedPrimitive (82) | `0x00DAB69C`, `0x00DAB78F`, `0x00DABA9C` |
| `0x00DAB480` | SetStreamSource (100) | `0x00DAB8C3`, `0x00DAB973`, `0x00DABA27`, `0x00DABAE3` |
| `0x00DAB480` | SetStreamSourceFreq (102) | `0x00DAB88F`, `0x00DAB94A`, `0x00DAB9F5`, `0x00DABABA` |
| `0x00DAC260` | TestCooperativeLevel (3) | `0x00DAC403` |
| `0x00DAE180` | TestCooperativeLevel (3) | `0x00DAE1C4`, `0x00DAE2AF`, `0x00DAE41C` |
| `0x00DAE570` | Present (17) | `0x00DAE5F4`, `0x00DAE750` |
| `0x00DAE570` | SetRenderTarget (37) | `0x00DAE5BD` |
| `0x00DAE570` | Clear (43) | `0x00DAE5E2` |
| `0x00DAE760` | GetBackBuffer (18) | `0x00DAE8A4` |
| `0x00DAE9C0` | TestCooperativeLevel (3) | `0x00DAE9FB`, `0x00DAEB67` |
| `0x00DAED10` | Present (17) | `0x00DAEE53` |
| `0x00DAF000` | TestCooperativeLevel (3) | `0x00DAF42A` |
| `0x00DAF7C0` | TestCooperativeLevel (3) | `0x00DAF838` |
| `0x00DAF7C0` | CreateAdditionalSwapChain (13) | `0x00DAFC05` |
| `0x00DAF7C0` | Reset (16) | `0x00DAF9FF`, `0x00DAFA3B`, `0x00DAFA90`, `0x00DAFAD4` |
| `0x00DB0A50` | Release (2) | `0x00DB0BD9` |
| `0x00DB0A50` | SetRenderTarget (37) | `0x00DB0B2F` |
| `0x00DB0A50` | SetDepthStencilSurface (39) | `0x00DB0B41` |
| `0x00DB1480` | GetAvailableTextureMem (4) | `0x00DB1976` |
| `0x00DB1480` | CreateAdditionalSwapChain (13) | `0x00DB19D8` |
| `0x00E0FCC0` | CreateVertexDeclaration (86) | `0x00E0FD9A` |
| `0x00E0FEC0` | CreateVertexShader (91) | `0x00E0FEF3` |
| `0x00E0FF00` | CreateVertexShader (91) | `0x00E0FF6E` |
| `0x00E0FFC0` | CreatePixelShader (106) | `0x00E0FFF3` |
| `0x00E10000` | CreatePixelShader (106) | `0x00E1006E` |
| `0x01103DD0` | CreateTexture (23) | `0x01103E79` |
| `0x01103DD0` | GetRenderTargetData (32) | `0x01103EAC` |
| `0x011042B0` | CreateRenderTarget (28) | `0x01104458` |
| `0x01104470` | CreateDepthStencilSurface (29) | `0x01104566` |
| `0x01105240` | CreateTexture (23) | `0x01105428`, `0x0110549C` |
| `0x01105240` | CreateVolumeTexture (24) | `0x011056D0` |
| `0x01105240` | CreateCubeTexture (25) | `0x0110558B` |
| `0x01129030` | CreateVertexBuffer (26) | `0x0112908C` |
| `0x01129200` | CreateVertexBuffer (26) | `0x01129286` |
| `0x01129460` | CreateIndexBuffer (27) | `0x011294B8` |
| `0x01129600` | CreateIndexBuffer (27) | `0x01129656` |
| `0x01129670` | CreateIndexBuffer (27) | `0x011296F9` |
| `0x01147AB0` | CreateAdditionalSwapChain (13) | `0x01147B6A` |
| `0x01147FE0` | CreateQuery (118) | `0x01148017`, `0x01148052` |
| `0x01148130` | CreateQuery (118) | `0x0114814D`, `0x0114817F` |
