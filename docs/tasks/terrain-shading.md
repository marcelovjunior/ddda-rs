# terrain-shading: terrain layer blending and material constants

- **Owner:** claude lane
- **Branch:** `task/terrain-shading`
- **Board area:** `render/shading`
- **Status:** review

## Goal

Make the world look like the original in the reference screenshots.
Today the terrain draws only its base albedo: no layer blending, and the
sun and ambient light are placeholders. Find how the original blends
terrain layers and what its materials' constants are, then draw that.

## Evidence

- `docs/FORMATS.md`, MRL and MFX sections:
  - materials are MFX feature switches plus texture slots;
  - the constant-buffer layouts (MFX kind-0 entries) are not parsed;
  - the state words and the animation block are unknown.
  - `ddtool mod-info <nativePC> <arc> <entry>` lists a model's meshes,
    layouts and material bindings.
- Terrain models: `scr\st100\model\m<A/10*10>\st100_<A>m<B>n`. Which
  textures and feature switches do their materials use? Look for blend
  maps or vertex-colour weights.
- Reference screenshots of the original with the exact camera pose
  (private, never commit): `E:\ddda-re\shots\refs\ref00N.jpg` and `.json`.
  - Usable: 001, 002, 004, 005. 003 is invalid; 006 and 007 are uncertain.
  - `ref00N-ours.png` / `-side.jpg` are our earlier renders and
    side-by-sides.
  - `ddview <nativePC> --ref <json> --shot out.png` renders our frame from
    the same pose (see `crates/ddview/src/main.rs` for options).
- Exe: the material classes (`nDraw::MaterialStdEst`, `DDMaterialStd`,
  `DDMaterialWater`, ...) in `dti-labels.csv` and the code that fills
  their constants. Stage lighting and fog may be XFS data under `scr\`.

## Scope

- `crates/mtmrl/**`, `crates/mtshader/**`
- `crates/ddview/src/stage.rs`, plus new shader asset files for the viewer
- `docs/SHADING.md` (new). Do not edit `docs/FORMATS.md`: put format
  findings in SHADING.md and head folds them in.
- **Not** `crates/ddview/src/character.rs` or `play.rs` (claude-2 owns them).

## Deliverables

- Terrain layer blending as the original does it (traced or derived from
  the data, never tuned by eye), with the material constants parsed.
- Before and after side-by-sides for refs 001, 002, 004 and 005, kept
  private. Describe them in Results.
- SHADING.md: the formats and the rules, marked traced or guessed.

## Done when

`cargo test -p mtmrl -p mtshader` and `cargo build --release -p ddview`
pass, and the reference comparisons show the terrain layers. Fill in
Results and report to `head`.

## Results

claude-3, branch `task/terrain-shading`. Details and evidence:
`docs/SHADING.md`.

**How the original blends terrain layers (data + compiled shaders).**
A cell model is opaque base meshes plus overlay meshes that sit on exactly
the same triangles (bit-identical positions: 2,858 of 2,858 overlay
triangles in 58m50n, 58m49n, 57m50n, 59m50n). Overlays use the
vertex-colour layouts (`IANonSkinTBC`/`TBNC`), `BSBlendAlpha`
(SrcAlpha/InvSrcAlpha), `DSZTest` (LessEqual, no depth write), `RSMesh`
and `FTransparencyAlpha`; per-vertex alpha is 0 or 1 and the soft edge
comes from interpolating it. A second kind (`FTransparencyMap`, layout
`IANonSkinTBA`, 126 stage-100 cell materials) also multiplies in the G
channel of `f00_alpha50_AM` on the second UV set. The rule
`alpha = albedo.a x vertex alpha [x map.g] x fTransparency` and
`colour = albedo x fAlbedoColor x light x fDiffuseColor` is read from the
compiled D3D9 shaders in `sc\dx9\stage100` (`rShaderPackage`, 764 VS +
4,224 PS, disassembled with D3DCompiler): every material-pass variant with
those inputs computes it. Not traced: the package's permutation lookup,
so no single shader is proven to be a given material's.

**Built.**
- `mtshader`: MFX constant buffers (all 207: parameters, type, offset,
  defaults) and blend / depth-stencil / rasterizer state decoding, with
  field meanings confirmed by the named states.
- `mtmrl`: constant-buffer data location (16-byte aligned after the
  commands; all 48,114 buffer commands of 17,787 materials tile their
  blocks exactly) and `mtmrl::shading::describe` (features, textures,
  constants, states, alpha source). `mtmrl` now depends on `mtshader`
  (Cargo.lock).
- `mttex::is_srgb`, translated from 0x01104F40 (colour formats 0x09, 0x14,
  0x16, 0x18, 0x23, 0x24, 0x28, 0x2F are sRGB; masks 0x19, normal maps
  0x1F, `_CMM` 0x13 are not). Out-of-brief file, approved by head (#363).
- `ddview` (`stage.rs`, new `transparency_map.wgsl`, one line in
  `main.rs` registering `stage::ShadingPlugin`, approved #363): blended
  overlays with vertex alpha, `FTransparencyMap` layers through an
  `ExtendedMaterial`, albedo/diffuse colour constants, UV transform
  (`FUVTransformOffset`, assumed), cull from the rasterizer state,
  reflectance 0 for `FSpecularDisable`, sRGB per format.

**Tested.** `cargo test -p mtmrl -p mtshader -p mttex` pass (15 tests;
`real_package_material_layouts` also runs against the install when
`DDDA_NATIVE_PC` is set, and passes). `cargo build --release -p ddview`
passes; clippy is clean for the touched crates.

**Compared with the original (refs 001, 002, 004, 005; before = the
`main` viewer, after = this branch, same pose).**
- 001 (hill above Cassardis): before, the grass slope below shows
  rectangular dirt and rock patches with hard polygon edges and a hard grey
  quad. After, the patches fade into the grass and the dirt path up the
  hill appears as a continuous soft-edged track, as in the reference.
- 002 (rock outcrop with the chest): the grass hillside below had grey
  rock strips with hard polygon edges; after, they are soft rock and dirt
  patches fading into the grass, closer to the reference's hillside. The
  outcrop itself changes little.
- 004 (tree by the coast): before, a dirt patch at lower left ends in a
  hard straight edge; after it blends into the grass, matching the
  reference's soft ground variation.
- 005 (road to the capital): before, road and grass meet along one hard
  polygon edge; after the road fades into the grass over a metre or two,
  as in the reference.
- Unchanged and still different from the original: lighting (placeholder
  sun and ambient, no SH, fog or light buffer), sky and water, missing
  props, and the framing (our renders show a narrower view than the
  reference at the same pose; not investigated). Blending happens in
  linear space here, in sRGB in the original.
- Note: refs 004/005 need `--cell 61,52` (the viewer does not take the
  origin cell from the JSON); `apply_ref` could read `origin_cell`.

**Not resolved / next.**
- Leaf crowns of alpha-tested foliage (`*_NUKI`, `RSMesh`) are cards in
  parallel planes facing +Z that the original turns to the camera; drawn
  two-sided here (with the rasterizer cull they vanish). Billboarding not
  traced (candidate `CBSwingBillboard`).
- Alpha-test reference: placeholder 0.5 (CBROPTest at run time; lead:
  MRL `bits[1]` byte 1 = 0x78/0x80/0x46... on alpha-tested materials).
- The SPK permutation lookup (`rShaderPackage::vf10` 0x010082D0 loads it).
- Lighting inputs (`fSHCoef`, `sLightParamDL0`, `CBFog`, the light
  buffer): next step for the look gate; codex-6/codex-11 trace the data.
