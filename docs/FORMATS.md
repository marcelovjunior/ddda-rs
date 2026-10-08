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

## MFX: shader package index (`mtshader`)

`nativePC\usershader\BbsShaderPackage.mfx` names every shader object:

- input layouts (kind 9), e.g. `IANonSkinTBN`
- textures (kind 1), e.g. `tAlbedoMap`
- feature switches (kind 2), e.g. `FAlbedo`
- samplers (kind 3)
- constant buffers (kind 0), e.g. `CBMaterial`

**Ids.** Other files refer to an object by `(JAMCRC(name) & 0xFFFFF) << 12 | entry index`.

- **Checked:** all 500,929 MRL shader-object ids and all 24 MOD vertex formats resolve this way.
- The layouts are read from the user's install at runtime; nothing is embedded in this repo.

**Element type codes.** These follow from the element offsets plus the checks noted against each code.

| Code | Meaning | Evidence |
| --- | --- | --- |
| 1 | f32 | positions lie inside the model bounds |
| 2 | f16 | UVs, some weights |
| 5 | s16 normalised (/32767) | |
| 8 | u8 | joints |
| 9 | u8 biased `x/255*2-1` | normals unit length to 0.008 |
| 10 | u8 normalised | |
| 11 | 4 × u8 biased, packed | normals unit length |
| 13 | u8 colour | |

*Unresolved:*
- Codes 3, 4, 7 and 14 are only used by particles, sprites and 1-weight skins.
- The exe's mapping to `D3DDECLTYPE` has not been traced yet.

## MOD: `rModel` v0xD4 (`mtmod`)

**Checked:** `ddtool mod-scan` passes on the full install.

- All 28,094 models (3,940 distinct) parse.
- All 215,267 meshes decode through their MFX layout: 70.2 M vertices, 71.3 M triangles.
- Every skinned vertex's weights sum to 1.
- There are no non-finite UVs.
- All 67,469 material names resolve in the same-path MRL by JAMCRC.

**Layout.** See the crate docs. Facts established on the data:

- The header is 0x84 bytes, the first table always starts at 0x84, and the mesh table plus weight bounds end exactly at the vertex buffer.
- Indices are u16 triangle strips (topology byte 4 everywhere), with no restart markers and with 4 zero bytes after the buffer.
- Indices lie in `[vertex_start, vertex_start + vertex_count)`.
- A mesh's vertices start at `vertex_byte_offset + (base_vertex + vertex_start) * stride`.
- Strip winding: even triangles are `(a,b,c)` and odd ones `(b,a,c)`. With that, the face normal `(b-a)x(c-a)` agrees with the stored normals on 98.4% of 1.0 M non-degenerate sample triangles.
- Quantised (s16) positions scale by one factor, the largest bbox extent, plus the bbox min. Checked: per-axis maxima equal `extent/max_extent`.
- Weights:
  - Semantic 0 is in the position's w.
  - Further semantics follow in order (u8 or f16).
  - The remainder up to 1.0 goes to the last joint.

**Unresolved:**
- 0.18% of normals deviate from unit length by more than 0.05.
- 25,811 of 30.7 M skinned vertices use a joint at or above the model's bone count. The 256-byte joint remap table is the likely explanation; it is not yet applied.
- The `IASkinBridge*` layouts have no UVs (51,651 meshes); their role is unknown.
- `draw_mode`, the LOD byte semantics (255 = all LODs?), and the meaning of the groups.

## MRL: `rMaterial` v0x20 (`mtmrl`)

**Checked:** `ddtool mrl-scan` parses all 28,152 lists (3,972 distinct). All 2,077,947 commands resolve in the MFX. Every texture index is in range (1-based; 0 = none).

**Material classes** are MtDTI names: `nDraw::MaterialStdEst` (most common), `DDMaterialStd`, `MaterialNull`, `DDMaterialWater`, `DDMaterialInner`, `MaterialStd`.

**What a material carries.** It is described entirely by MFX feature switches and texture slots:

- `FAlbedo` = `FAlbedoMap` / `FAlbedoMapAdd` / ...
- `FBump` = `FBumpNormalMap` / `FBumpDetailNormalMap` / `FBump` (none)
- `FSpecular` = `FSpecularMap` / `FSpecularDisable`
- `FTransparency` = `FTransparencyAlpha` / ...
- Textures: `tAlbedoMap`, `tNormalMap`, `tSpecularMap`, `tEnvMap`, `tEmissionMap`, `tDetailNormalMap`, and others.

`ddtool mod-info <nativePC> <arc> <entry>` lists a model's meshes, layouts and material bindings.

**Unresolved:**
- The constant-buffer layouts (MFX kind 0 entries are not parsed yet).
- The two state-bit words.
- The animation block.
- The blend, depth and rasterizer state ids.
