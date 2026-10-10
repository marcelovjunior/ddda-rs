# S10: Fix the elder's ear hole and hair in cutscenes

- **Branch:** `step/s10-ear-hair` (after S01 and S02)
- **Files:** most likely `crates/ddview/src/character.rs`, `stage.rs`
  (character mesh/material building), maybe `crates/ddlook` (part choice)
- **Kind:** diagnose and fix one visible defect

## The defect

st200 ev10 (`--event 22`, frames 800-1200): the elder has a dark hole
around his ear and his hair looks missing (maintainer screenshot at frame
1010; docs/tasks/cutscene-visuals.md Results describe the hole too).

## What is already ruled out (cutscene-visuals Results)

Back-face culling as authored, face morphs, skin-matrix determinants,
4-vs-8 weight skinning, mesh winding, transparent sort order, the NPC
texture override table 0x00BC1650. Disabling all culling *does* show the
ear, so the surface exists but faces the wrong way or is the wrong part.

## Leads

- S01's part log: is a hair or ear-covering mesh skipped (shadow-only,
  MaterialNull, LOD, group)? Is the right hair model chosen for this NPC?
- S02's group colours: which group should sit over the ear.
- The NPC's look record (hair/head parts) versus what the viewer loads.

## Do

Find the cause, fix it in the shared character path (it should help every
NPC, not just the elder), and add a regression (unit or installed test)
that would have caught it.

## Done when

A frame-1010 capture shows the elder with hair and no ear hole
(`E:\ddda-re\notes\steps\s10\`), the tests pass, and the Result names the
cause with its source (address or data).

## Result

Implemented in `ec01142` on `step/s10-ear-hair`; current-main integration
is `c420b39`. Only `crates/ddview/src/character.rs` and this Result differ
from main. Status: **review**.

The cause was **frustum culling with the parts' rest-pose AABBs**, while
the vertex shader drew their animated positions. Bevy 0.19.1 does not
automatically update skinned bounds: its `VisibilityPlugin` runs
`update_skinned_mesh_bounds` only for `DynamicSkinnedMeshBounds`, and
requires `Mesh::skinned_mesh_bounds()` generated from the skin attributes.
The existing character builder provided neither. Tight ear, hair and
face bounds could lie outside the camera even when their animated
triangles lay inside it. `ViewVisibility` alone did not isolate this:
it includes visibility to the shadow views as well as to the main camera.

Evidence from the installed data and bounded frame-1010 viewer captures:

- The elder's base `model\\pl\\m\\m_base\\m000\\m000` mesh 24, group 20,
  contains the ears (mainly semantic joints 221/225); all 464 rest
  triangles agree with their stored normals. All ten colour-pass
  `model\\pl\\hair\\hair104\\hair104` meshes were already admitted.
- Exact CPU skinning and projection found front-facing ear triangles
  covering pixels (1140,500) and (1140,550), while the baseline GPU frame
  showed the background there. The ear's authored material was opaque,
  `Cull::Back`, depth-test/write LessEqual, with no alpha test.
- Near clipping was excluded: the near plane was 16 cm, the elder's
  face mesh 27 lay at 85.576..97.449 cm view depth, and all hair vertices
  lay beyond 75.606 cm. No inspected character vertex was near-clipped.
- Static and dynamic-bounds captures used the same authored materials,
  face/body morphs, culling, meshes and camera. `static1010.png` retains
  the opening and sparse hair; `dynamic1010.png` restores the actual ear
  and full hair coverage. S11 independently localized the same stale
  bounds issue for Quina's brow overlay.

The shared `skinned_part` builder now generates joint bounds, adds
`DynamicSkinnedMeshBounds`, and retains `RenderAssetUsages::MAIN_WORLD`
on the mesh asset. Retention is necessary because Bevy's bounds updater
reads `Assets<Mesh>` on every animation frame, after GPU extraction.
It uses the final joint transforms, including the viewer's body/face
morphs. No culling override or NPC-specific geometry rule was added.

The new headless regression uses this same builder and Bevy's actual
visibility systems. A rest part starts outside a camera; its joint moves
it into view and back out on later frames. It checks the camera's
`VisibleEntities`, the updated AABB, and continued CPU asset retention.
It would fail with the previous static bounds.

Validation:

- Targeted animated-frustum regression: **1 passed**, 0 failed.
- Before main integration, full viewer tests: **53 passed, 2 ignored**;
  release build passed in 35.28 s.
- After integration, `cargo test --release -p ddview --target-dir
  E:/ddda-rs-wt/m5-cassardis/target`: **57 passed, 0 failed, 2 ignored**,
  8.26 s (compile 20.68 s).
- Integrated `cargo build --release -p ddview` with the same shared target:
  passed in 30.92 s; existing `Args` visibility and `forget_actor` warnings.
- Final ordinary event 22 / frame 1010 / hold / unattended capture,
  with all temporary diagnostics removed: exit 0 within its 60 s
  watchdog. `frame1010.png` visually confirms full hair and a complete
  ear, with the original authored materials. Executables, screenshots,
  projection exports and logs remain private in
  `E:/ddda-re/notes/steps/s10/`. The original game was not driven.

Temporary camera/material/depth/triangle diagnostics in `character.rs`
and the small related `event.rs` hook were removed before the fix was
committed; no event code change remains in this step.

This resolves the reported geometry omission; it is not a full original
game rendering comparison. Hair colour/shading approximations, Quina's
expression and the remaining eyebrow appearance checks are separate.
Next: finish S11's authored brow verification and S12's facial constraint
trace, with this shared bounds fix in place.
