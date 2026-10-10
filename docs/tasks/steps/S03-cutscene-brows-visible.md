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

## Result

codex-1, 2026-10-09: complete diagnostic. Implementation `5776ce2`
(saved as WIP during the coordinator's model-switch stop, then resumed).
`DDVIEW_BROW_DEBUG` replaces only material colour group 6 overlays with
unlit opaque red and depth bias +5. Mesh admission, skinning, position and
authored culling stay as before; without the variable the material path
is unchanged. Group 5's colour override is not mistaken for a brow.

Frame 1010 capture:
`E:/ddda-re/notes/steps/s03/brows-red.png` (1600x900), inspected after
resume. **Quina: no** visible red brows above the eyes. **Elder: no**;
his group-6 overlay is hidden by the authored part mask, so this run does
not test his brow shading or geometry placement. No wrongly placed red
brow was seen elsewhere in the closeup. This is a diagnostic outcome,
not a brow fix or proof that the geometry is misplaced.

The private log `frame1010.err.log` confirms the red diagnostic material
was constructed for Quina (`actor=3161v0`, female base) and the off-camera
player (`actor=2614v0`, male base); it was not constructed for the elder.
Both use material `XfBAN____01_m006_`. The existing installed regression
`npc_eyebrow_materials_are_selected_from_installed_mrl` confirms that the
elder's authored mask hides this overlay while Quina's admits it.

Validation in the shared Bevy release target:

- `cargo test --release -p ddview`: 52 passed, 2 existing ignored,
  0 failed (12.89 s compilation, 3.13 s tests).
- `cargo build --release -p ddview`: passed in 29.79 s, only the two
  existing warnings (`private_interfaces`, unused `audio::forget_actor`).
- Hidden `--unattended --event 22 --start-frame 1010 --hold` capture with
  a 60 s watchdog: exit 0; shot delay 15 s.
- `git diff --check`: passed. No rendering fix in this step.

Next: S11 should trace why Quina's admitted, opaque red overlay remains
occluded/invisible, including its depth/culling and animated placement.
The elder's actual brow surface may belong to the base model rather than
the hidden overlay; establish that from the authored material/part data.
