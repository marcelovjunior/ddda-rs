# S11: Fix the eyebrows

- **Branch:** `step/s11-brows` (after S01 and S03)
- **Files:** most likely `crates/ddview/src/character.rs` and the
  character material code in `stage.rs` / `dd_material.wgsl`
- **Kind:** diagnose and fix one visible defect

## The defect

Brows don't show on cutscene characters (and likely in play too).
cutscene-visuals Results: the brow is a separate overlay mesh (Quina's is
group 6) with depth state `DSZTest` (test, no write) and alpha blending;
the colour comes from the edit palette (0x00843940, 0x008442D0,
0x00780FA0). S03 tells you whether the geometry is there and in place.

## Do

- If S03 showed the geometry in place: the problem is shading or blending.
  Check the brow texture's alpha (which channel holds the brow shape),
  the colour multiplication, the blend state and the draw order against
  the skin. Read the original's brow material in the MRL and the shader it
  uses.
- If the geometry is missing or misplaced: fix the mesh/skinning path.

Fix it in the shared character path and add a regression.

## Done when

A frame-1010 capture shows both actors' brows with their colour
(`E:\ddda-re\notes\steps\s11\`), the tests pass, and the Result names the
cause with its source.

## Result

codex-49, 2026-10-09: the missing brow geometry was rejected by the
viewer’s **rest-pose bounds**, rather than by its alpha or depth state.
The shared character fix is `ec01142` on `step/s10-ear-hair`; this step
records the independent brow diagnosis and verification instead of
duplicating that implementation.

Quina’s female-base mesh 36 (part group 24, material colour group 6,
`XfBAN____01_m006_`) has 222 vertices and four skin influences. Replaying
the actual frame-1010 camera and posed triangles from S10’s private export
finds front-facing overlay triangles at pixels (570,355) and (635,370),
respectively 0.0765 cm and 0.1403 cm nearer than the skin. Every overlay
vertex is beyond the 16 cm near plane. Its rest-pose box, however, projects
entirely above the view: NDC Y 1.0068 through 1.4984. The larger skin box
still intersects the view. The root-transform fit used for that box replay
differs from the exported posed vertices by at most 0.0022 cm. S03’s red
material could not repair this visibility rejection.

The installed MRL specifies `BSBlendAlpha`, `DSZTest` without depth writes,
`RSMesh`, `FAlbedoMap`, `FTransparencyAlpha`, and `FUVPrimary`. The shape is
in `pl_eyebrow00_BM`’s alpha channel (64x64 DXT5, alpha range 0..176).
Colour and UV controls already follow 0x00843940, NPC conversion
0x008442d0, and UV setters 0x01067570 / 0x010675f0. No new colour, bias,
blend, culling or skin displacement rule was needed.

The elder’s authored mask hides the base overlay. His brow-containing
surface is in `hair104`, particularly mesh 8 (98 vertices, brow-joint
influences including IDs 126/127, 136..143 and 210/211), also included in
the combined alpha-mask mesh 10. Its MRL uses the group-5 hair material and
`hair104_BM` alpha. The fix restores those hair parts as well.

`character::skinned_part` now generates Bevy’s per-joint mesh bounds and
adds `DynamicSkinnedMeshBounds` to every shared character part. It retains
the mesh in `MAIN_WORLD`, which Bevy 0.19.1’s
`VisibilityPlugin::update_skinned_mesh_bounds` needs when updating bounds
from the current joints each frame. This is a viewer visibility fix; it
does not claim to translate the original engine’s culling algorithm.

Validation:

- S10’s shared implementation regression moves a skinned part into and
  out of a real camera frustum and checks camera-specific visibility,
  updated bounds, and continued CPU mesh availability. Its integrated
  `cargo test --release -p ddview` passed **57 tests, 2 existing ignored**
  (8.26 s). The integrated `cargo build --release -p ddview` passed
  (30.92 s). Both used the shared `m5-cassardis/target` directory.
- Normal frame-1010 capture, **without red/group-colour/depth diagnostic
  overrides**, is `E:/ddda-re/notes/steps/s11/frame1010.png`, copied from
  S10’s approved ordinary capture (exit 0, unattended, 60 s watchdog).
  It was visually inspected. RGB pixels exactly match S10’s dynamic-bounds
  diagnostic capture. Quina’s dark brows and the elder’s hair brow surface
  are now drawn; the ear and hair restoration is reported in S10.
- Projecting the authored texture alpha independently onto Quina’s posed
  overlay gives 2,195 pixels above alpha 20/255, of which 2,061 darken
  between static and dynamic captures. The elder’s visible profile portion
  of hair mesh 8 gives 2,209 such pixels, of which 2,060 darken. These
  measurements support restored coverage; they are not original-game
  colour acceptance. Scratch scripts, textures, geometry exports and
  measurements remain under `E:/ddda-re/notes/steps/s11/`.

An additional hidden red-overlay-plus-dynamic-bounds launch was rejected
by automatic approval review with only “blocked by policy” stated. No
process started, and it was not retried. The existing ordinary capture
and geometry/texture checks supplied the verification instead.

Open: Quina’s distorted expression is S12. General character normal
mapping and the existing approximation of the hair tint remain unchanged.
No fresh original-game comparison was made, so exact brow shape/colour
matching beyond restored authored coverage remains unverified. Next:
resolve the expression in S12, then compare the resulting closeup with the
original.
