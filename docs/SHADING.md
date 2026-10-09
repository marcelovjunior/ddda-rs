# Shading: materials, constants and terrain layers

How the original draws a material, as far as it is traced, and what the
viewer does with it. Labels: **traced** (read from the exe or its compiled
shaders), **data** (checked on the game files), *assumed* (a guess, said
why). Format findings for `docs/FORMATS.md` are in the first sections.

Code: `mtshader` (MFX constant buffers and render states), `mtmrl`
(constant-buffer data, `mtmrl::shading`), `mttex::is_srgb`,
`crates/ddview/src/stage.rs` and `transparency_map.wgsl`.

## Sources

- `nativePC\usershader\BbsShaderPackage.mfx`: names and layouts of every
  shader object (FORMATS.md, MFX). Feature switches (kind 2) are small
  compiled expression graphs (e.g. `FTransparencyAlpha` returns a `float`
  "Transparency" from `mc` = `MATERIAL_CONTEXT` and reads `CBMaterial`);
  that IR is not decoded.
- `sc\dx9\<stage>` (`rShaderPackage`, 76 files, e.g. `sc\dx9\stage100`,
  `sc\dx9\root`, `sc\dx9\gamemain`): the compiled D3D9 shaders. Header
  `SPK\0`, the MFX hash `0xB46006D5`, version `0x22`; loaded by
  `rShaderPackage::vf10` (0x010082D0), which checks those three fields,
  fixes up offset tables and creates the vertex shaders
  (FUN_00E0FEC0) and pixel shaders (FUN_00E0FFC0).
  `stage100` holds 764 vertex and 4,224 pixel shaders (shader model 3,
  each with its constant table), from 0x101D10 on. They disassemble with
  `D3DDisassemble` from `D3DCompiler_47.dll`.
  *Not traced:* the permutation lookup (which shader a feature set
  selects). The rules below are what every variant with the named inputs
  computes, not one proven permutation.

## MFX constant buffers (kind 0) — data

207 entries (`CBMaterial`, `$Globals`, `CBFog`, ...). From the entry
start (offsets are absolute file offsets):

```text
+0x18 u32  size in floats (low 16), parameter count (high 16)
+0x20 u32  parameter table, +0x24 u32 default values (size floats)
parameter, 28 bytes:
  u32 name (string offset), u32 bits (unresolved),
  u32 type word = array << 20 | rows << 8 | columns << 4 | base type,
  u32 (unresolved),
  u32 offset word: offset in floats = (w >> 8) & 0xFFFF, top byte 0..5 (unresolved),
  u32 annotation table (SasUiControl, SasUiMin, ...), u32
```

Base types, from the parameter names: 1 float (`f...`), 3 bool (`b...`),
4 int (`i...`), 0xB/0xC/0xD structures (`sLightParam...`,
`sShadowReceiveParam...`, `sOutlineDetector`; no member list).
Check: in all 186 buffers without structure parameters the last parameter
ends in the buffer's last 4-float register.

`CBMaterial` (32 floats): `fDiffuseColor` float3 @0, `fTransparency` @3,
`fReflectiveColor` float3 @4, `fTransparencyVolume` @7, `fUVTransform`,
`fUVTransform2`, `fUVTransform3` (float4x2) @8, @16, @24. Defaults 1, 1, 10
and identity transforms. `$Globals` (80 floats) holds `fAlbedoColor` @0
and 32 more (`fShininess`, `fSpecularColor`, `fFresnelSchlick`, ...).

## MFX render states (kinds 4, 5, 6) — data

The MRL material header's blend, depth-stencil and rasterizer ids resolve
to these entries. Words from entry +0x18; each field below is confirmed by
the named states, the rest is unresolved.

- **Blend (kind 4, 136 states).** Word 0: bit 1 enable, bits 9-13 source
  factor, bits 17-21 destination factor, bits 25-27 operation. Factor codes
  0 Zero, 1 One, 2 SrcColor, 3 InvSrcColor, 4 SrcAlpha, 5 InvSrcAlpha
  (confirmed: `BSBlendAlpha` 4/5, `BSAdd` 1/1, `BSMul` 0/2, `BSComposite`
  1/5, `BSAddAlpha` 4/1); 6-10 *assumed* in D3D order; 11/12 *assumed*
  BlendFactor/InvBlendFactor (`BSBlendFactor`, `BSBlendFactorAlpha`).
  Operation 2 RevSubtract (`BSRevSub*`), 4 Max (`BSMax`). Byte 0 of word 2
  is the write mask (`BSRGBWrite` 7, `BSRWrite` 1, `BSNoWrite` 0). Word 1
  (alpha channel) unresolved.
- **Depth-stencil (kind 5, 31).** Word 0: bit 0 depth test, bit 1 depth
  write, bits 2-4 comparison in D3D order from Never (`DSZTestWriteGT` 4
  Greater, `DSZWrite` 7 Always, `DSZTest*` 3 LessEqual). Stencil fields
  unresolved.
- **Rasterizer (kind 6, 20).** Word 0: bit 0 solid (`RSWireFrame` clears
  it), bits 3-4 cull (`RSMeshCN` 0 none, `RSMeshCF` 1 front, `RSMesh` 2
  back), bit 8 scissor (`RSScissor*`). Word 1: signed depth bias
  (`RSMeshBias1` = -32, -32 per step).

## MRL constant-buffer data — data

A `ConstantBuffer` command's value is the byte offset of the buffer's data
from the command-list start. The data follows the command list, each
buffer 16-byte aligned, `size * 4` bytes in the MFX layout. Check: all
48,114 buffer commands of the 17,787 materials (one MRL per resource
path) resolve to a kind-0 entry, and together they tile each material's
command block exactly.

Values that differ from the MFX defaults (materials, all MRLs):
`fReflectiveColor` 3,321, `fShininess` 4,172, `fSpecularColor` 2,937,
`fFresnelSchlick` 2,224, `fAlbedoBlendColor` 1,929, `fAlbedoColor` 975,
`fEmissionColor` 888, `fDiffuseColor` 263, `fUVTransform` 176,
`fTransparency` 153, `fTransparencyVolume` 111, plus `CBBurn*` and others.
**Every terrain cell material of stage 100 (2,979) holds the defaults.**

Material header word `bits[1]`: byte 1 holds 0x78/0x80/0x46/0x7E/... on
alpha-tested materials (`8c90xx00`), possibly the alpha-test reference
(lead, not traced; opaque materials carry such values too).

## Pixel shader rules — traced from the compiled shaders

The engine is a light pre-pass renderer: the material pass reads the
light buffer (`tLightAccumulationTexture0`, sampled at the pixel,
light = `min(-log2(sample), 8)`). In the material-pass pixel shaders that
sample `tAlbedoMap` (e.g. the `sc\dx9\stage100` pixel shaders at file
offsets 0x26F938, 0x3F0490, 0x3B4B9C, 0x2A1BE8, 0x624A80, 0x32C18C):

```text
N        = vFace ? -n : n                       (two-sided normal flip)
light    = SH(fSHCoef[7], N) + light buffer + max(0, N.L) * sun colour
colour   = albedo.rgb * fAlbedoColor * light * fDiffuseColor
           * occlusion * (1 - max(ambient mask))
colour   = lerp(colour, fog colour, fog factor) * HDR factor
alpha    = albedo.a * vertex alpha * fTransparency [* fGlobalTransparency]
alpha   *= tTransparencyMap.g        (FTransparencyMap variants, own UV)
alpha test (some variants): discard if alpha <= CBROPTest.x
```

`occlusion` and `vertex alpha` come from the vertex shader. In the
non-skinned vertex shaders with a colour input (e.g. the one at file
offset 0x10D084):
`o2.w = colour.w` (VertexAlpha), `o3.xyz` = world normal and
`o3.w = normal.w` (the layout's `Occlusion` byte), and
`o2.xyz = (c / (1.001 - c))^2.2` of the vertex colour, which the
`FDiffuse` pixel shaders do not read. The UV is
`u' = row0.x*u + row0.y*v + row0.w` with `fUVTransform` (likewise `v'`).
The vertex-element order of these shaders is position, normal, tangent,
"binormal" = the TexCoord pair, "texcoord" = colour + alpha.

The transparent variants convert their output to sRGB in the shader
(`0.0031308`, `12.92`, `1/2.4`, `1.055`) before blending, so **the
original blends in sRGB space**.

## Terrain layers — data + traced

A stage cell model (`scr\st100\model\m<A/10*10>\st100_<A>m<B>n`) is opaque
base meshes plus overlay meshes:

- Overlays use the layouts with vertex colour (`IANonSkinTBC`,
  `IANonSkinTBNC`) and materials named `..._W_...`, with `BSBlendAlpha`,
  `DSZTest` (LessEqual, no write), `RSMesh`, `FTransparencyAlpha`.
- **Every overlay triangle coincides with a base triangle** (bit-identical
  positions; checked on 58m50n, 58m49n, 57m50n, 59m50n: 2,858 of 2,858
  triangles). Overlays almost never overlap each other (1 triangle in
  those four cells), so their draw order does not matter.
- Vertex alpha is 0 or 1 per vertex; the blend comes from interpolating it
  across the shared triangles. Vertex colour RGB is a constant 110/255
  grey on most overlays (unused by `FDiffuse`).
- Some layers (`..._AY_...`, layout `IANonSkinTBA`, alpha priority 1) use
  `FTransparencyMap`: `tTransparencyMap` = `scr\st100\model\textures\
  f00_alpha50_AM` (DXT1 code 0x19, R = G = B soft mask) on the `UV_Extend`
  set (the second TexCoord), `FChannelTransparencyMap` (G: 351 of the 362
  transparency-map reads in the `stage100` pixel shaders use G, 11 use R).
  126 of the stage-100 cell materials, 112 in the m50-m59 rows.
- Cell materials' states: 2,007 `BSSolid/DSZTestWrite` opaque, 846
  `BSBlendAlpha/DSZTest` + `FTransparencyAlpha`, 126 + `FTransparencyMap`.

So a layer's coverage is `albedo.a * vertex alpha [* map.g] * fTransparency`
blended over the base with SrcAlpha / InvSrcAlpha.

## Texture colour space — traced

`mttex::is_srgb`, translated from 0x01104F40: the texture setup sets bit 30
of the texture's format word for format codes 0x09, 0x14, 0x16, 0x18,
0x23, 0x24, 0x28, 0x2F. Those are the colour formats (`_BM`, sky, UI,
effects); masks (0x19), normal maps (0x1F) and `_CMM` (0x13) are not
flagged. *The flag's consumer (sampler sRGB state) is not traced.*

## What the viewer does

`stage.rs` builds each material from `mtmrl::shading::describe`:

| Original | Viewer | Status |
| --- | --- | --- |
| `fAlbedoColor` (FAlbedoMap) x `fDiffuseColor` | base colour (linear) | traced |
| alpha = albedo.a x vertex alpha x `fTransparency` | base alpha x `ATTRIBUTE_COLOR` = (1,1,1,vertex alpha) | traced |
| `BSBlendAlpha` | `AlphaMode::Blend` (no depth write) | traced |
| `FTransparencyMap` | `ExtendedMaterial` multiplying the map's G on UV 1 | traced, map space linear per is_srgb |
| alpha test | `AlphaMode::Mask(0.5)` | 0.5 is a placeholder (CBROPTest at run time) |
| rasterizer cull | `cull_mode` / `double_sided` | traced; alpha-tested foliage forced two-sided, see below |
| `FUVTransformOffset` + `fUVTransform` | `uv_transform` | *assumed*: 156 of 176 non-identity transforms have this variant; the default variant is taken to ignore the transform |
| `FSpecularDisable` | `reflectance = 0` | derived (no specular term) |
| sRGB-space blending | linear-space blending | **difference** (Bevy) |
| lighting (light buffer, SH, sun, fog) | Bevy PBR, placeholder sun/ambient | not done |

Alpha-tested leaf crowns (`*_NUKI`, e.g. `om0500`, `om0501`, `om0531`,
`om0532`) are `RSMesh` (cull back) but every card of a crown lies in a
plane z = const facing +Z: the original turns them to the camera (how is
not traced). Culled as stored they vanish from most views, so the viewer
keeps drawing alpha-tested materials two-sided.

## Open

- The SPK permutation lookup (feature set -> shader), to pin each rule to
  the exact shader of a material.
- The alpha-test reference (CBROPTest; lead: MRL `bits[1]` byte 1).
- Foliage billboarding (what turns the leaf cards; candidate
  `CBSwingBillboard`).
- `FDiffuseVertexColor*`, `FTransparencyVolume`, `FAlbedoMapBlend` and the
  other variants: not traced.
- Lighting inputs: who fills `fSHCoef`, `sLightParamDL0`, `CBFog`
  (codex-6 / codex-11 trace the weather and light data side).
