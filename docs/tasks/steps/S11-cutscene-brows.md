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
