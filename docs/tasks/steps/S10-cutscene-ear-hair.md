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
