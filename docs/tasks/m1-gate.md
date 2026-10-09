# m1-gate: close the visible differences at ref001

- **Owner:** claude-3
- **Branch:** `task/m1-gate`
- **Board area:** `render/shading`
- **Status:** open

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

(filled in by the agent)
