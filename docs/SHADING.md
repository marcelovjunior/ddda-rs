# Shading: materials, constants and terrain layers

How the original draws a material, as far as it is traced, and what the
viewer does with it. Labels: **traced** (read from the exe or its compiled
shaders), **data** (checked on the game files), *assumed* (a guess, said
why). Format findings for `docs/FORMATS.md` are in the first sections.

Code: `mtshader` (MFX constant buffers and render states), `mtmrl`
(constant-buffer data, `mtmrl::shading`, alpha test), `mttex::is_srgb`,
`mtswm` (swing models), `mtweather::lighting` (SH),
`crates/ddview/src/stage.rs` and `dd_material.wgsl`.

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

Material header word `bits[1]` holds the alpha test (see "Alpha test").

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

## Alpha test — traced

`rMaterial::vf09` (0x0112E4D0) builds each draw material from the MRL
header word `bits[1]`: enable = bit 20, comparison = bits 21-23 (forced to
7, Always, when disabled), reference = byte 1. The draw code (FUN_00FEE320)
writes `reference / 255` to `CBROPTest.fAlphaRef` and
`1.1 / (1 - reference / 255)` to `fAlphaToCoverageScale`; the pixel shaders
discard when `alpha <= fAlphaRef`. Every enabled material uses comparison 4
(Greater). Common references: 0x78 (1,467 materials), 0x80 (966), 0x46
(125), 0 (358, also the blended terrain overlays). `mtmrl::Material::alpha_test`.

## Swing billboards — data + traced shaders

Foliage has an `rSwingModel` (`mtswm`; `...\etc\<model>_s00`, or the model's
own path for `fmfore_*`): one node per mesh with a per-vertex record of a
quantised pivot (`u8 xyz / 255 * scale + offset`, model space) and a wind
weight. Nodes with flag 0x200 are billboards (133 of 137 have every vertex
in its pivot's local XY plane). The stage swing vertex shaders (e.g. file
offset 0x1A9500 in `sc\dx9\stage100`) compute

```text
pivot_world = fWorld * pivot
world       = pivot_world + d.x * ViewI.row0 + d.y * ViewI.row1 + d.z * ViewI.row2,  d = position - pivot
```

(the offset is not transformed by the model matrix), rotate the normal the
same way, then add a wind sway along `fSwingConstraint` scaled by the
weight while keeping `|world - pivot_world|`. The viewer does the
billboard rotation (pivot in the vertex colour, w = -1, `dd_material.wgsl`)
and not the wind. 8-byte records (`fmfore_*`, flag 0xCA08) are not decoded;
those meshes are drawn two-sided. Which flag bits select the cylindrical
(`FSwingBillboardTransformFixedY`) variant is not traced; all billboards
use the spherical form above.

`nDraw::MaterialNull` meshes (`Scene_Material` in scenery models) are not
drawn (whether they cast shadows is not traced).

## Projection and placement — traced / measured

- The game renders with `uCamera.mFov + CameraFov`: `uCameraBase::vf21`
  (0x007D3E00) adds the float at `*0x018D1D20 + 0x24`, a settings object
  filled from `[GRAPHICS] CameraFov` in the player's
  `%LOCALAPPDATA%\CAPCOM\DRAGONS DOGMA DARK ARISEN\config.ini` (15 on this
  machine). Live check: uCameraGame/uCameraCtrl `mFov` 55, while sCamera
  viewport 0 holds m11 = 1.42815 (70.0 degrees) and m00/m11 = 9/16. The
  recorded `uCameraGame.mAspect` (1.0) is not the viewport's. The reference
  poses now line up with the screenshots (props, rocks, bones in place).
- Layout `mAngle` is applied about world X, then Y, then Z (R = Rz Ry Rx):
  measured (the only order of six that puts log om0529 in st100_58m49n
  where the live capture has it; also consistent with refs 001/002/005).
  Not traced in the exe.

## Lighting — traced shaders, live values

The stage is a light pre-pass renderer:

- **Light buffer:** deferred light shaders (`sc\dx9\root`, e.g. file offsets
  0x33FD5C, 0x82DFF0) add, per light, `colour * intensity * max(N.L, 0)`
  [x shadow] (point lights also attenuate), stored as `exp2(-light)` and
  decoded by the material pass as `min(-log2(sample), 8)`.
- **SH:** hemisphere lights have no deferred shader; they and the
  unshadowed infinite lights go into `fSHCoef` (`mtweather::lighting`:
  projections translated from 0x0130D6D0 and 0x0130D420, Sloan packing
  *assumed*).
- **Light units** (live, sky scheduler): `uSkyInfiniteLight` colour +0x50,
  direction to the light +0x230 (several: the sun, a fill light from
  above, a weaker group-0x10 light); `uSkyHemiSphereLight` sky colour
  +0x50, ground colour +0x230, axis +0x240; `uSkyColorFog` start +0x188,
  end +0x18C, density +0x194, colour +0x1A0 (e.g. 1000 / 110000 / 0.9).
- **Light groups:** `uLight.mGroup` (+0x34) is a bit mask; stage cells and
  scenery have MOD light group 1. The sun (0x10C7), the fill light
  (0x10C5) and one hemisphere (0x10C7) share bit 0; a grey hemisphere and
  the third infinite light are group 0x10 only. Including the group-0x10
  lights doubles the shadowed ground's brightness against the captures,
  excluding them matches: the rule `mGroup & model.light_group != 0` is
  *assumed* from that, not traced.

The viewer (`--light <json>`) computes `fSHCoef` from the hemisphere and
non-sun infinite lights, lights the stage with
`albedo * (SH(N) + sun * max(N.L, 0) * shadow)` using Bevy's shadow map for
the sun, applies the linear depth fog, and turns off Bevy's tone mapping.

**Compared** (claude-4's read-only day series, private
`E:\ddda-re\shots\refs-lit`, same camera pose as the capture): the sunlit
wedge and the shadow edge of the 09:46 capture line up. Mean colour of a
ground region (8-bit sRGB, box 400..1200 x 450..700 of 1600 x 900) ref vs
ours, before the post chain: 09:46 (79,80,68)/(100,102,88), 12:21
(30,39,40)/(36,47,48), 15:00 (26,34,34)/(43,55,56), 17:45
(26,34,34)/(30,40,41), 20:21 (12,16,17)/(14,21,22), 23:04 (5,9,11)/(3,6,7).
With the post chain (below): 09:46 (79,80,68)/(90,91,78), 12:21
(30,39,40)/(31,40,41), 15:00 (26,34,34)/(37,48,48), 17:45
(26,34,34)/(25,34,35), 20:21 (12,16,17)/(12,17,18), 23:04 (5,9,11)/(1,4,5).
A second, character-free ground box (100..500 x 100..300) gives ours / ref
sums of 1.17, 1.00, 1.39, 1.00, 0.98 for 09:46 .. 20:21: three of the five
day captures match within 2 %, 09:46 is 13-17 % bright, 15:00 (all ground in
the rock's shadow) 39 % bright; night is too dark (no moon or point lights).

## Post chain — data + traced shaders

Stage 100's filter scheduler `scr\st100\sdl\st100_f00` creates three
filters (no bloom filter in it):
- `ImagePlaneFilter` (priority 1): `scr\sky\camera_shadow_NOMIP_BM`, black
  with a vignette in alpha, times `mColor` (1, 1, 1, 0.5), blended over the
  frame (blend type 1, *assumed* SrcAlpha / InvSrcAlpha): a vignette in the
  sRGB frame buffer.
- `DOFFilter` (priority 16, `mType` 1 "Light", traced: vf11 0x01013320,
  shaders 0x89E474 / 0x775888 / 0x611C08 / 0x466ADC of `sc\dx9\root`):
  the frame point-sampled down to 640 x 360 in sRGB, a separable 5-tap
  Gaussian (sigma 1, step `mBlurSize`, point sampled, `mBlurCount`
  times), blended over the frame with alpha min(0.385, (z - zf) / (zfar -
  zf)) behind the focus and min(0.22, (zf - z) / (zf - zn)) in front; z,
  zf, zn, zfar are post-projection depths (z/w) of the pixel, `mFocal`
  1000 cm, `mNear` 50 (clamped to the near plane) and `mFar` 200000 cm.
  Active only with the player's DofFilter=ON (0x0050adc0). The viewer's
  frame is float and carries the view depth in alpha for it; the post
  pass clamps the colour to 0..1 as the 8-bit frame did. *Assumed:* the
  composite blends SrcAlpha / InvSrcAlpha; one fullscreen pass evaluates
  the low-res image and blur per pixel (same result, more taps).
- `ColorCorrectFilter` (priority 26000): `ToneCurveRGB` is an
  MtHermiteCurve (8 x, 8 y; here x 0, 0.2, 1 / y 0, 0.155738, 1) rendered
  into a table by the pixel shader at file offset 0x47800C of
  `sc\dx9\root` (`mtweather::post::tone_curve`, translated, regression
  test against the shader), raised to `mGamma` (1) and applied per channel
  to the linear colour; the colour matrix (`mCorrector`) is empty.

The viewer (`post.rs`, `dd_post.wgsl`) runs both as one fullscreen pass
under `--light`. *Assumed:* ascending priority order; the image plane
covers the screen (its UV transform is not read); 256 table entries.

## Sky — traced shaders, live sun

The weather manager creates a `uSkyExt` with 256 x 256 optical depth maps
(0x004c19f0). Each frame the sky renders the Rayleigh and Mie depth maps
(when the atmosphere changes), Rayleigh and Mie scatter maps over the screen
(`mDivX` x `mDivY` = 32 x 32, loop count by `mLoopCount` = 10) and the dome,
which lights the scatter maps:
`sun colour x (Rayleigh (1 + cos^2) + Mie (1 + g^2 - 2 g cos)^-1.5)`
plus stars.
- `CBSky` (44 floats, `mtweather::sky::constants`, translated from
  0x00f33d50 and the tail of 0x00f36d60): Rayleigh scattering from the
  `rSky` wavelengths (665 / 555 / 455 nm) and +0x90 (0x00e2c920, `l^-4`),
  Mie from `mMieScattering` / `mMieDensity`, heights and earth radius from
  `rSky` +0xA8..+0xBC (km), eccentricity 0.4 (constructor default),
  `mHeightOffset` 30000, sun colour `mBaseSunColor`, which the weather
  manager overwrites with the weather's `SunColor_F` (0x004c3d40).
- Depth maps (`optical_depth_map`, translated from the pixel shaders at file
  offsets 0x59745C and 0x38E804, regression test against them).
- Scatter and dome: `sky.wgsl`, generated instruction for instruction from
  the 10-step shaders at file offsets 0x5F9134 (Rayleigh) and 0x657A54
  (Mie) and the dome shader 0x5B8D54. **Difference:** the scatter runs per
  pixel on the pixel's own ray instead of on the 32 x 32 screen grid with
  its horizon-weighted rows.
- Sun direction (`sun_direction`): *reconstructed*, not translated: the
  standard solar position for the resource's latitude (47 deg), tilt
  (23.4 deg) and season angle (90 deg, declination `tilt x sin` assumed)
  with noon at 12:00; it matches the live light direction at 09:46, 12:21,
  15:00 and 17:45 within 0.006. At night the captured infinite light points
  straight up, so the sky uses the clock.

- Cloud domes (`uSkyCloudModel`, 0x00c64620): the scheduler's
  `SkyCloudModel_*` whose `mWeatherType` is the weather (`sky_all_F` and
  the belt by fine weather), at the sky's height offset following the
  camera, hidden by night when `mDayOnly`. Their light is the unit's sun
  colour (the sun at `mSunColorHeight` x `mSunColorScale`, saturation x
  `mSaturationScale`, x `mIntensityScale`) plus its ambient colour
  (0x00f330a0: the in-scatter at `mAmbColorHeight`), both translated into
  `mtweather::sky`; *assumed:* the cloud shader adds the two as a constant
  light (it is not identified). Under the stage fog (beyond the fog end)
  they turn the horizon from white to the fog's blue.

*Assumed:* each track's first key. Not drawn: stars (the `ddstar` catalog
billboards with `hoshi2_BM`, the `star_all` / `kosumo_BM` model), the
starry sky texture (no `mpStarrySkyColorTexture` is set by the stage), the
moon (`uSkyAstralBody`, the `rSky` moon texture); no night sky reference
exists to check them against.

## Water — traced shaders, partly inferred

The sea (`st100_sea00`) and the rivers and lakes (`DDMaterialWater`) keep
their MRL materials.
- **Unlit forward materials:** every stage100 pixel shader with the sea's
  sampler set (albedo, normal, detail normal, specular, environment cube;
  e.g. file offset 0x4A0980) and every blended water shader reads no light
  buffer. Their colour (0x4A0980) is albedo x `fAlbedoColor` x
  `fDiffuseColor` x v1.rgb x v2.w + reflection x mean(v1.rgb) x v2.w. No
  vertex shader outside the grass reads `fSHCoef` or a light, so v1 and
  v2.w are *inferred* to be the layout's VertexColor and Occlusion: the sea
  is unlit, with baked vertex colours (0.47 grey on the sea).
- **Environment cubes:** `FReflectCubeMap` on `scr\sky\DDCube<i>_CM` (a
  run-time target) is `DDBaseCube<i>_CM` (a 32 x 32 cube texture in the
  data) times `mEnvMapBaseColor[i]` x lerp(1, `mEnvMapColor[i]`.rgb, .a)
  of the weather manager (sEnvMapManager's SystemEnvCube pass 0x00434be0,
  shader 0x912060; for cube 0: 0.3 x 0.25). The reflection is env x
  `fReflectiveColor` x specular G x mean(vertex colour) x Fresnel-Schlick
  (`fFresnelSchlick` 0.6). *Assumed:* the weather scales of the cube
  colours are not applied, the blend factor is 0.
- `FReflectWaterMap` (planar water reflection): the stage sky along the
  reflected ray stands in for the mirrored scene (shared
  `sky_scatter.wgsl`).
- `FTransparencyVolume` blends with `fTransparency`. Not done: normal and
  detail normal maps, the volume's depth fade, the coast foam (an effect
  render target `effect\rtt\e_st100_10_BM`; those meshes are left out).

## Distant terrain and landmarks — traced

- Far ring: each far cell draws its terrain model
  (`scr\st{stage}\model\m{A/10*10}\st{stage}_{a}m{b}n`) under its `fmfore`
  scenery, so far trees stand on ground.
- Landmarks (`uFmSetBase`): 15 entries placed from the exe table at
  0x01873550 (radius, position; FUN_00c61bd0), each loading
  `fm_index<i>_high.arc` within its radius and `_low.arc` beyond; entries 5
  and 7 swap on story progress 0x1E78 (`--progress`). The castle in ref001
  is now drawn.

## Stage low models — traced

The split controller (`uStageSplitCtrl`, 0x00c5c2d0) creates
`uStageLowMdl` units: one for `scr\st100\model\st100_all00` (the whole
map, 30 km wide, up to 1.8 km high: the horizon mountains) and, per area
slot (0x00c5d7a0), `st100_area<NN>_h` or `_l` with NN from the table at
0x014e9228 (0, 3, 5, 10, 20, 30). The table at 0x014e8d00 gives each cell
(row A-40, column B-30) an area value; the player's cell's value marks two
slots high (3: slots 0/1, 10: 3/0, 12: 3/4, 13: 3/5, 0x23: 1/2), the rest
are low, and ViewRange FAR or FARTHEST (config object +0x38 >= 2) makes
all high (0x00c58d60). The viewer places them at the stage origin
(*assumed*: the creator sets no position) without the streaming fade.

## Primitive pass mask and LOD — traced

The model draw (0x00ea5320) draws a primitive when (1) its group is
enabled, (2) its LOD byte has the model's current level bit, and (3) its
draw mode (the primitive's first u16) shares a bit with the pass mask
(render-pass begin 0x00fedb80: 1 the scene, 2 reflections, 8 / 0x200 the
shadow casters, 0x10 environment cubes). The LOD level is 1 within the
model's middle distance, 2 within its far distance, 4 beyond (both
doubled for ViewRange FAR; FARTHEST keeps 1). The viewer picks the level
at spawn from the start eye, skips primitives without bit 1, draws those
with only a shadow bit into the shadow map (e.g. the white cypress shadow
cards, draw mode 0xE6C8) and marks those without a shadow bit as not
casting.

## Lighting from the data — traced, checked live

Without `--light` the viewer builds the lighting for `--clock` and
`--weather` (fine, cloudy, very cloudy):
- Sun direction: `mtweather::sky::sun_direction` (see Sky).
- Sun colour (`uSkyInfiniteLight`, 0x00c688a0): the sky's sun colour
  (the weather's `SunColor`) through the atmosphere to 100 cm above the
  sky's height offset (Rayleigh, Mie and cloud-slab depth, 0x00f32840
  with the 50-step integral 0x00f2ffc0), times `mLightScale` (0.1), HLS
  saturation times `SaturationScale`, times `IntensityScale` (fine 3.0,
  cloudy 3.6), times one minus the day/night blend (0x004c18c0: sun
  height clamped to +-0.2, mapped to 0..1). It reproduces the four live
  day reads within 1 % (regression test).
- Fill light and hemisphere light: their `DayColor` / `DayRevColor` of the
  weather blended to `NightColor` / `NightRevColor` by the day/night blend
  (*assumed* linear; the 20:21 capture agrees within the sun-direction
  error).
- Fog: the weather's `scr\st<S>\etc\st<S>_fog_<w>` sampled at the clock
  (0x00cead20). `uColorFog` (vf09 0x00e47f80) writes the colour, density,
  start and `1 / (end - start)` for distance type 1 (linear); the stage
  vertex shaders pass clip w as the fog depth, so the fog is linear in view
  depth, as the viewer does it.
- With `--light`, the weather is the one whose fog at the capture's clock
  equals the captured fog (the refs-lit series is cloudy: `st100_fog_e`;
  the weather manager's +0x268 holds 1).

## Night: the sky spot light — traced

The sky scheduler's `SkySpotLight` (`uSkySpotLight`) is a deferred spot
light at `LightOffset` (0, 2000, 0.1) cm above the player (*assumed*
anchor; the unit attaches to a tracked position, 0x00c92d10), pointing
down. Its colour is `LightColor` (0.022, 0.057, 0.078) x clamp((night blend
- 0.8) x 10, 0, 1) (0x00c69460), so it is lit only deep at night. The spot
shader (file offset 0x9B1A70 of `sc\dx9\root`) adds colour x (1 -
sat((d - mStart) / (mEnd - mStart)))^2 x sat((dot(-dir, L) - cos(cone/2)) /
(cos((cone - spread)/2) - cos(cone/2)))^falloff x max(N.L, 0); its
parameters come from 0x00e19850 (start 2000, end 3000, cone 90, spread 60).
The `--light` captures did not read spot units, so it is added from the data
there too. It lights the ground around the party at 23:04 as in the
capture.

## Characters — traced shaders, partly inferred

Character models carry light group 64 (bit 6). The sun (`uSkyInfiniteLight`,
group 0x10C7), the fill light (0x10C5), the hemisphere light (0x10C7) and
the sky spot light (0x10C5) all include that bit, so characters get the
same lights as the stage; the viewer gives them the stage's SH, sun, shadow,
spot light and fog (`stage::Loader::character_material`, skinned in the
vertex stage of `dd_material.wgsl`).

Of the 1946 materials of the `model\pl` lists, 1673 use `FAlbedoMapAdd`,
1681 `FUVAlbedoBlendMap = FUVViewNormal` (on `model\textures\fs003_BM`),
1655 `FReflectCubeMapShadowLight` (1585 on `scr\sky\DDCube2_CM`), 1918
`FSpecularMap`, and all `FAmbientSH`, `FLighting`, `FBRDF`,
`FIntegratedOutlineColor`. Their pixel shaders (file offsets 0x31BEA8 and
0x40C494 of `sc\dx9\root`; 0x4BB2DC is the rare `FAlbedoMapBlendMaxAlpha`
variant) compute:

```
A     = albedo * fAlbedoColor + blend.rgb * blend.a * fAlbedoBlendColor.rgb * fAlbedoBlendColor.w
        blend = tAlbedoBlendMap at the view-normal UV:
        vn = normalize(fView3x3 * N) ; uv = (0.5 + 0.5 vn.x, 0.5 - 0.5 vn.y)
diff  = (SH(N) + light buffer diffuse + DL0 diffuse) * fDiffuseColor * occlusion
spec  = ((light buffer specular + DL0 specular) * fSpecularColor + env) * specular map.rgb * occlusion
F     = lerp(fFresnelSchlick, 1, (1 - N.V)^5)
lit   = A * diff + spec * F
out   = outline select (below), fog, * fHDRCompressEncode
DL0   = the forward infinite light sLightParamDL0 (uInfiniteLight 0x00e15c70):
        colour * (1 - max(mask a * ShadowGroup0, mask b * ShadowGroup1)) *
        (max(N.L, 0) for diffuse, (N.normalize(L + V))^fShininess for specular)
env   = cube(reflect(V, N)) * fReflectiveColor *
        (1 + 7 fAppShadowLightScale (N.normalize(fAppReflectShadowDir + V))^fShininess
           - fEnvMapMask * shadow mask)
```

- `fAlbedoColor` is (1.139, 1.035, 0.932) in 335 of the materials (skin);
  `fAlbedoBlendColor` is (0,0,0,0) in 1298 and (1,1,1,0) in 268, so the
  blend map adds nothing there; it is (1,1,1,1) in 273.
- The outline term is off in normal play: every `model\pl` material and
  the MFX default hold `fOutlineBlendMask` (1,0,0,0) (keep the lit colour);
  the exe writes the mask from the identity table 0x01874640 by
  `mOutlineBlendType` (0x00c87c30 / 0x00c87ca0, called from the
  material-effect callbacks of uArmor, uWeapon, uObjModel and
  uDDMCActorModel); modes 1 and 2 (`cOutlineParam`, `param\outline`,
  timed) turn on a view-angle rim lerp(inner, outer,
  sat(pow(1 - N.V, 5))).
- DL0 is bound only for lights with a forward technique (`uLight::vf20`
  0x00e135d0). That the sun is DL0 for characters is *inferred* (light
  groups, the shadowed infinite light), not traced at run time.

The viewer applies `fAlbedoColor` for `FAlbedoMapAdd` /
`FAlbedoMapBlendMaxAlpha`, the blend map on the view-normal UV, the sun as
DL0 (diffuse with its shadow, Blinn-Phong specular), the environment cube
(`DDBaseCube<i>_CM` times the weather's cube colour, as for the stage) with
the shadow-light factor at the MFX defaults (fAppShadowLightScale 1,
fAppReflectShadowDir (0, 1, 0), fEnvMapMask 1; their run-time writers are
not found), and Fresnel. *Not done:* the light buffer's specular, the
vertex occlusion (1), normal maps, the burn/damage/edit albedo variants
and the eye materials' `FUVSecondary` blend map.

`--event` lights its stage and actors the same way from the data at
`--hour` in fine weather (`stage::data_lighting`), when the stage has a sky
scheduler.

### The canyon at the saved spot (st100 58m49n, 12:35, fine)

The canyon floor there lies in the walls' shadow at 12:35, so it gets only
the SH. In fine weather that is small: the hemisphere light's
`DayColor_F` (0.055, 0.112, 0.165) over `DayRevColor_F` (0.03, 0.061,
0.09) and the fill light's `DayColor_F` (0.055, 0.112, 0.165), against
(0.21, 0.336, 0.39) for the cloudy weather's colours, 2.4 to 3.8 times
weaker by channel. The direct sun is strong (fine `IntensityScale` 3.0), so
sunlit rock is bright and shade is dark. In our render (`--play --save`)
the shaded floor averages (9, 16, 19) of 255 after the post chain and the
lit wall (62, 63, 57); with `--weather e` the floor is (18, 26, 26). The
refs-lit capture at 12:21 in cloudy weather at the same canyon matched our
cloudy render (30, 39, 40 there), so the rules hold for cloudy weather;
the fine-weather darkness follows the same traced rules but is **not
compared live** (needs a capture by the maintainer at the saved spot).

## What the viewer does

`stage.rs` builds each material from `mtmrl::shading::describe`:

| Original | Viewer | Status |
| --- | --- | --- |
| `fAlbedoColor` (FAlbedoMap) x `fDiffuseColor` | base colour (linear) | traced |
| vertex `Occlusion` x all light | folded into the vertex colour RGB | traced (VS passes it, PS multiplies the light) |
| alpha = albedo.a x vertex alpha x `fTransparency` | base alpha x vertex colour alpha | traced |
| `BSBlendAlpha` | `AlphaMode::Blend` (no depth write) | traced |
| `FTransparencyMap` | map G on UV 1 in `dd_material.wgsl` | traced, map space linear per is_srgb |
| alpha test | `AlphaMode::Mask(reference + 0.5/255)` | traced |
| rasterizer cull | `cull_mode` / `double_sided` | traced |
| swing billboards | vertex stage of `dd_material.wgsl` | traced (no wind) |
| `FUVTransformOffset` + `fUVTransform` | `uv_transform` | *assumed* (156 of 176 non-identity transforms have this variant) |
| `FSpecularDisable` | `reflectance = 0` (Bevy fallback only) | derived |
| SH + light buffer + fog | `--light`: own lighting in `dd_material.wgsl` | traced shaders; SH packing and light-group rule assumed |
| sRGB-space blending | linear-space blending | **difference** (Bevy) |
| post chain: vignette + tone curve | `--light`: `post.rs` | traced (order and blend assumed) |
| DOF | `post.rs`, depth in the frame's alpha | traced (blend assumed) |
| sky dome | `--light`: `sky.rs` | traced shaders, per-pixel; sun reconstructed |
| water | unlit forward + environment cubes | traced shaders, vertex inputs inferred |
| cloud domes | `spawn_sky_model` | traced unit, shading assumed |
| night spot light | `SpotLight` in `dd_material.wgsl` | traced |
| stage low models, primitive pass/LOD | `spawn_low_models`, `spawn_parts` | traced |
| lighting/fog by clock and weather | without `--light` | traced, sun checked live |
| character shaders: albedo add, sun specular, env cube, Fresnel | `character_material` + `FLAG_ALBEDO_ADD` / `FLAG_CHARACTER` | traced shaders; DL0 = sun inferred; env constants at MFX defaults |
| character outline (`FIntegratedOutlineColor`) | not drawn | traced: off in normal play |

Without `--light` the stage keeps Bevy's PBR with the placeholder sun and
ambient.

## Open

- The SPK permutation lookup (feature set -> shader).
- The SH packer and the light-group test in the exe (both assumed here).
- Post chain: the material pass's `fHDRCompressEncode` (0.5) and the
  chain order are not traced. 09:46 is 13-17 % and 15:00 39 % brighter than
  the captures.
- Sky: stars, the moon and the cloud shader; the upper sky is about 12 %
  darker than ref004's; the uSky sun matrix code (0x00f36d60) is not
  translated.
- Landmarks beyond the fog end (the Bluemoon Tower at ref001) are a flat
  fog silhouette in ours but show structure in the original: not
  explained (both use the low model and linear fog).
- Water: the normal maps, the volume depth fade, the foam effect, the
  planar reflection's scene; the sea is 9-20 % darker than refs 001-005.
- Wind sway of swing models; 8-byte swing records; the cylindrical
  billboard variant.
- Night: the moon light (the group-0x10 lights) and point lights of lamps
  and the lantern are not used; the 23:04 ground matches in brightness
  with the spot light, the far rock wall is darker in ours.
- Characters: which light is DL0 at run time; the writers of
  `fAppReflectShadowDir`, `fLightMaskSelector` and the shadow groups; the
  light buffer's specular; the outline modes 1 and 2; a live comparison of
  the fine-weather canyon at the saved spot.
- `FDiffuseVertexColor*`, `FTransparencyVolume`, `FAlbedoMapBlend` and the
  other variants: not traced.

### Primary stage normal maps (render-fidelity checkpoint)

The ordinary stage forward shader now supports `FBumpNormalMap` with
primary normal/albedo UVs. Source: installed `sc/DX9/Stage200` PS file
0x6171f8 (confirmed by 0x318e0c and 0x5f3000), VS file 0x1381d4.
The linear DXT5 map uses alpha/green: XY = 2 * (AG - 127/255),
Z = sqrt(1 - dot(XY, XY)) when the radicand is greater than 0.0001,
otherwise 0.01. The decoded biased-byte tangent W multiplies
cross(transformed tangent, transformed normal). Only N is normalized
before interpolation; T and B retain their source lengths. The final
weighted sum is normalized. UVs use the material transform.

The implementation preserves geometric-normal UVs for the separate
character albedo blend term. Character bump is disabled pending a traced
skinned basis; the viewer unit conversion in skin matrices cannot be
blindly applied to this raw ordinary-world basis. Detail normals,
secondary/procedural UV variants and a normal prepass are not implemented
by this checkpoint. Existing face orientation is retained pending the
same-pose comparison. AO and shadow matching remain unresolved.

Decoder/basis and material-selection regressions pass. A release build
and bounded field capture (480 meshes, 397 instances) passed without WGSL
pipeline errors. This does not establish visual agreement with the
original; see the render-fidelity brief for the comparison status.

### Ambient-occlusion filter source audit

The executable contains a separate `uAmbientOcclusionFilter`; this is
not accounted for by the already supported mesh Occlusion byte.
Constructor **0x010460f0** and reflection **0x01046250** identify:

| Offset | Property | Constructor default |
| --- | --- | --- |
| +0x280 | mLocalSpaceOccRadius | 10 |
| +0x284 | mLocalSpaceFallOff | 1 |
| +0x288 | mGlobalSpaceOccRadius | 20 |
| +0x28c | mGlobalSpaceFallOff | 1 |
| +0x290 | mDistanceDispersion | 2 |
| +0x294 | mIntensityDispersion | 5 |
| +0x298 | mIntensity | 1 |
| +0x2a0 | mBounceIntensity | 1 |
| +0x2ac | mOccDecayRatio | 2 |
| +0x2b0 | mBlurTapNum | 5 |
| +0x2b8 | mMaxRadius | 0.1 |
| +0x2bc | mAmbientMaskGroup | 0 |

These are constructor observations, not values to hard-code for a stage.
The authored/live settings and enable conditions still need to be read.
Draw callback **0x0104a870** binds +0x40/+0x44 as `tSSAONarrowMap` and
`tSSAOWideMap` (MFX indices 0x387/0x388). It uploads intensity and bounce
intensity to `CBSSAOFFilterIntensity` (0x29b), selects
`FSSAOApplyAmbientOcclusion` (0x7c8), and chooses one of four
`BSAmbientMaskGroup0..3` states (0x265..0x268) using +0x2bc. Optional
bounce follows through **0x0104a320**; its formula is not translated yet.
The underlying map generation/blur callbacks, active Cassardis settings
and final material mask consumption need comparison before implementing
this pass. The existence of this filter alone does not prove it was
active in a particular screenshot.

An additional ordinary material shader, `sc/DX9/Stage200` PS file offset
**0x242be4**, confirms that the decoded perturbed normal feeds the SH
lighting evaluation and that the same alpha/green midpoint and Z floor
apply in a material pass, not just the normal buffer. Vertex occlusion
then multiplies the resulting light.

The stage-100 and stage-200 `scr/st*/sdl/st*_f00` schedulers were also
read directly from the installed archives. Their spawned filter units
are DOFFilter, ImagePlaneFilter and ColorCorrectFilter; neither contains
an AO unit. This narrows the activation investigation to another owner
or scheduler; it does not establish that AO is absent at run time.
