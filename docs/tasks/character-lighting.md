# character-lighting: characters lit by the stage, as the original

- **Owner:** claude-3
- **Branch:** `task/character-lighting`
- **Board area:** `render/shading`
- **Status:** review

## Goal

In `--play --save` (and `--event`), characters are lit by Bevy's ambient,
not the stage's lighting, and the canyon at the saved spot looks very
dark (head screenshot after play-visuals). Light characters the way the
game's character shaders do:
- sun with shadows;
- the SH/hemisphere ambient from `mtweather::lighting`;
- the light buffer;
- any character-specific terms (rim, skin) traced from the character
  shader family.

Also check the overall darkness at the saved spot (st100 58m49n, 12:35,
fine weather) with the traced rules; a live comparison needs the
maintainer, so mark it.

## Scope

- `crates/ddview/src/stage.rs`, `character.rs` (materials only;
  coordinate with claude-7)
- the shaders
- `crates/mtweather/**`
- `docs/SHADING.md`

## Done when

- Characters use the stage lighting in `--play --save` and `--event`.
- Screenshots are described.
- Fill in Results and report.

## Results

Commits on `task/character-lighting`: 057ca03 (code), the docs commit after it.

- **Characters use the stage lighting.** `character.rs` builds its parts
  with `stage::Loader::character_material` (DdMaterial, skinned in the
  vertex stage of `dd_material.wgsl`), so in `--play --save` they get the
  stage's SH ambient (`mtweather::lighting`), the sun with its shadow map,
  the night spot light and the fog instead of Bevy's ambient.
- **Character terms traced** (character pixel shaders 0x31BEA8 / 0x40C494
  of `sc\dx9\root`; docs/SHADING.md, "Characters"): `fAlbedoColor` for
  `FAlbedoMapAdd` (skin tint (1.139, 1.035, 0.932) in 335 materials), the
  albedo-blend map on the view-normal UV (`fs003_BM`; adds nothing in most
  materials, whose `fAlbedoBlendColor.w` is 0), the sun's Blinn-Phong
  specular x `fSpecularColor` x specular map, the
  `FReflectCubeMapShadowLight` environment cube, Schlick Fresnel. The
  outline/rim term is traced as off in normal play (mask (1,0,0,0) in all
  `model\pl` materials and the identity table the exe writes). *Inferred:*
  the sun is the forward light DL0 for characters. *MFX defaults:*
  fAppReflectShadowDir, fAppShadowLightScale, fEnvMapMask. *Not done:* the
  light buffer's specular, vertex occlusion, normal maps.
  `mtmrl::shading` reads the new constants (regression test
  `describes_albedo_add`; `stage::tests::albedo_add_colour`).
- **`--event`** lights its stage and actors from the data at `--hour`, fine
  weather (`stage::data_lighting`; small edit in event.rs, claude-7 told).
  Fixed on the way: since m1-night-water `dd_material.wgsl` imports the
  sky scattering library, which only `SkyPlugin` loaded, so `--event` and
  `--lineup` drew no DD materials (no stage, and with this change no
  characters); the library now loads with `ShadingPlugin`.
- **Screenshots** (scratchpad, not committed):
  - `--play --save` (st100 58m49n, 12:35, fine): the Arisen stands in the
    canyon walls' shadow, lit only by the cool blue SH like the floor
    around him; the pawn further on is the same; one sunlit rock face upper
    right is bright. Before, the party was lit by Bevy's ambient
    regardless of shade.
  - `--play --clock 12:00` (default spawn, sunlit ridge): the bodies show
    sun on shoulders and heads with a specular sheen and cast shadows; the
    sides away from the sun take the blue-grey SH (main: uniformly orange,
    Bevy ambient).
  - `--event 22 --start-frame 700` (st200 at 08:00): Selene and the old
    man in the stone room, both lit by the data lighting; the walls are
    drawn again (black on main).
- **Canyon darkness** (traced rules): the floor is in shadow at 12:35 and
  fine weather's ambient (hemisphere/fill `DayColor_F` (0.055, 0.112,
  0.165)) is 2.4-3.8x weaker than cloudy; our shaded floor averages
  (9, 16, 19)/255 fine vs (18, 26, 26) cloudy. Our cloudy render matched
  the refs-lit 12:21 cloudy capture at the same canyon. **Not compared
  live in fine weather**: needs the maintainer's capture at the saved spot.
