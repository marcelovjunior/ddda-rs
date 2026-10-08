# Asset formats: implemented readers

Status per format. "Checked" = verified against every resource of that
type in the Steam install (`ddtool <fmt>-scan`); the private survey is
`E:\ddda-re\notes\formats-survey.md`. Unresolved points are labelled.

## ARC (`mtarc`)

See the crate docs. Gate: all 8,536 archives, 358,431 entries inflate to
their recorded sizes (`ddtool arc-scan --inflate`).

## TEX: `rTexture` v0x99 (`mttex`)

**Checked:** `ddtool tex-scan` parses all 73,706 textures (27 s). Each one
tiles its file exactly, with no gaps, overlap or trailing bytes.

**Header:**
- 16-byte header (see the crate docs).
- Cube maps only: 27 floats follow the header.
- Then an offset table, one entry per mip and face, in face-major order.
- Then tightly packed surfaces.

**Shapes in the data:**

| Shape | Count | Notes |
| --- | ---: | --- |
| Flat | 73,561 | |
| Cube | 131 | 6 faces, all format 0x14 |
| Volume | 14 | `system\filter\VolumeNoise_LIN`-style, A8R8G8B8, depth in the high 16 bits of word 3 |

**Format byte to GPU format.** The mapping comes from the exe, not from community lists. The format byte indexes a 48-entry `D3DFORMAT` table at `0x01440698`. It is read by `mov edx,[eax*4+0x01440698]` at `0x011051a2`, in `nDraw::Texture` code. The bytes per pixel I measured for every code that occurs agree with the table.

| Code | D3D9 | Count | Seen as |
| --- | --- | ---: | --- |
| 0x13 | DXT1 | 5,655 | `_CMM` and others |
| 0x14 | DXT1 | 22,110 | `_BM` base colour, `_NUKI` alpha-tested base colour (1-bit alpha), all cube maps |
| 0x18 | DXT5 | 9,387 | `_BM` with alpha |
| 0x19 | DXT1 | 6,956 | `_MM`/`_TM` masks (community lists call this BC4; the exe says DXT1) |
| 0x1F | DXT5 | 19,079 | `_NM` normal maps, DXT5nm layout (see below) |
| 0x25 | DXT5 | 468 | effect `_PM` maps |
| 0x27 | A8R8G8B8 | 14 | the 14 volume textures |
| 0x28 | A8R8G8B8 | 286 | sky, sun, UI |
| 0x2B | DXT5 | 111 | |
| 0x2F | DXT5 | 9,640 | effect textures (`_GM` and others) |

**Normal maps (0x1F).** Measured on `f_body_b000_NM_HQ`: R = B = 255 everywhere, and G and A carry the data. This is the standard DXT5nm swizzle, X = A and Y = G, with Z reconstructed. *Unresolved:* which of A and G is X has not yet been confirmed in the shader.

**Unresolved:**
- Which formats are sampled as sRGB. The exe has `SRGB`, `GPUFormat` and `MiscFlags` strings next to the table, so this is likely a per-texture flag.
- What the cube-map float block is used for.
- The meaning of the separate format codes that map to the same D3D format (e.g. 0x13 vs 0x14, 0x18 vs 0x1F). They likely select shader decoding.

Inspection: `ddtool tex-dds <arc> <entry> <out.dds>` writes a legacy DDS
(DXT1/3/5 and 32-bit formats) for viewing in external tools.
