# S12: Fix Quina's distorted facial expression

- **Branch:** `step/s12-expression` (after S04)
- **Files:** most likely `crates/ddview/src/character.rs` (animation,
  morph), `crates/mtlmt` (motion decoding), `crates/ddlook` (face morph)
- **Kind:** diagnose and fix one visible defect

## The defect

At ev10 frame 1010 Quina's face looks distorted (maintainer screenshot):
the expression doesn't look like the original's. S04 lists the face
joints whose animated pose moves far from rest.

## Leads

- Face joints may get the event's facial motion and the face-edit morph
  (ddlook::morph) in the wrong order, or twice.
- Face tracks may be in a separate motion bank or layer in the event
  (lip-sync / expression layer) that should be blended, not replaced.
- Unkeyed channels keep the rest value (character.rs comment says
  *assumed*); for face joints this may be wrong.
- docs/ANIMATION.md and docs/EVENTS.md describe what is known.

## Do

Find which joints are wrong and why, fix it in the shared path, and add a
regression.

## Done when

A frame-1010 capture shows a natural expression (`E:\ddda-re\notes\steps\s12\`),
tests pass, and the Result names the cause with its source. If the cause
needs a live comparison with the original's joints, describe what to read
and ask head.
