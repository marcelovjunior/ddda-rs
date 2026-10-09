# terrain-shading: terrain layer blending and material constants

- **Owner:** claude lane
- **Branch:** `task/terrain-shading`
- **Board area:** `render/shading`
- **Status:** open

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

(filled in by the agent)
