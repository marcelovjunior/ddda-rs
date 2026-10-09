# m1-gate: close the visible differences at ref001

- **Owner:** claude-3
- **Branch:** `task/m1-gate`
- **Board area:** `render/shading`
- **Status:** review

## Goal

A head side-by-side at `ref001` (`ddview --ref ref001.json`, after
m1-finish) shows these differences from the original:
1. **No distance haze.** The original's far terrain fades to blue
   (aerial fog). Ours is crisp to the horizon.
2. **Missing horizon mountains** on the left (the original's far
   backdrop).
3. **Floating far terrain chunks** at the top right, near the coast:
   a far-ring or landmark placement bug?
4. **White figure-like shapes** near the castle walls: placeholder meshes,
   NPC stand-ins or `MaterialNull`?
5. **Sky** when no `--light` is given: default to a light JSON of the
   nearest refs-lit capture, or to the reference's clock if known.

Fix them with traced rules: fog from the stage fog data (`uFog`, CBFog,
`mtweather`), and the backdrop from the stage's far models. Then produce
the M1 gate comparison: side-by-sides of refs 001/002/004/005 and two
refs-lit times, described in MILESTONES.md's M1 section (head allows
that edit for this brief).

## Scope

- `crates/ddview/src/stage.rs`, `main.rs`, `sky.wgsl` and the shaders
- `crates/mtweather/**`
- `docs/SHADING.md`
- `docs/MILESTONES.md`: the M1 section only

## Done when

- The five differences are fixed or explained with a traced reason.
- The gate comparison is written up.
- Fill in Results and report.

## Results

Commits on `task/m1-gate`: d401bbe (stage low models, primitive pass
mask and LOD), aae8b8a (lighting, fog and sky from the data by clock and
weather), 6fe72e2 (shadow-only primitives), plus the docs commit. Details
in `docs/SHADING.md` (Stage low models, Primitive pass mask and LOD,
Lighting from the data) and the gate in `docs/MILESTONES.md` (M1).

1. **Distance haze:** fixed. Our fog was already the original's linear
   fog (traced: `uColorFog` vf09, clip-w fog depth), but ref001 is fine
   weather while our lighting came from the cloudy refs-lit capture. The
   weather's fog file at the clock (blue `st100_fog_f`) now gives the
   haze: ref001's far mountain (106,142,186) / ours (104,140,185).
2. **Horizon mountains:** fixed: the `uStageLowMdl` backdrop
   (`st100_all00` and the area models) traced and drawn.
3. **Floating far chunks (top right):** the flat-topped cliffs were the
   missing low models' place; fixed with 2. Remaining at the top right:
   the Bluemoon Tower's low landmark is a flat fog silhouette (the
   original shows structure; not explained), and the sea plane ends
   before the horizon.
4. **White figures by the castle:** the cypresses' shadow cards (white
   texture, draw mode 0xE6C8 without the main pass bit). Fixed by the
   traced pass mask; they now cast the shadows only. The cypress
   imposters follow the traced LOD distances.
5. **Sky without --light:** lighting, fog and sky are built from the game
   data for `--clock` (default 12:00) and `--weather` (default fine); the
   sun colour reproduces the live reads within 1 % (test). Not applied
   to `--play` and `--character` (unchanged).

Gate comparison: see MILESTONES.md. ref001 within 3-4 % over the frame;
refs 002/004/005 have no recorded clock (rendered at 12:00 fine; 004's
shadows show a lower sun), refs-lit unchanged (within 1-3 levels).
Open: DOF, stars/moon/clouds/sky fog, water reflection, night lights.
