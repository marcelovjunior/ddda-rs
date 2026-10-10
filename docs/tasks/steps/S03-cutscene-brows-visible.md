# S03: Force the eyebrows visible

- **Branch:** `step/s03-brows`
- **Files:** `crates/ddview/src/character.rs`
- **Kind:** diagnostic, no behaviour change

## Why

Brows don't render in cutscenes. Earlier work (docs/tasks/cutscene-visuals.md
Results) made the brow material use the authored `DSZTest` depth state (no
depth writes) and alpha blending, and applied the brow colour. We need to
know whether the brow *geometry* is in the right place and facing the
camera, separately from its shading.

## Do

1. Find the brow meshes: the ones whose material is the brow overlay
   (search `character.rs` and `stage.rs` for "brow"; the cutscene-visuals
   Results say Quina's brow is a separate group-6 mesh).
2. When `DDVIEW_BROW_DEBUG` is set, give only those meshes an unlit,
   opaque, bright red material with depth test on and a depth bias of +5
   (so they win over the skin). Everything else unchanged.

## Run

The cutscene command from README.md with `DDVIEW_BROW_DEBUG=1`, frame 1010,
shot to `E:\ddda-re\notes\steps\s03\brows-red.png`.

## Done when

The capture exists and the Result says, for each actor, whether red brows
appear above the eyes in the right shape (yes / no / wrong place). No
fixes.
