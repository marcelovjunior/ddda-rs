# S02: Colour each mesh group of the cutscene characters

- **Branch:** `step/s02-group-colours`
- **Files:** `crates/ddview/src/character.rs` (where character materials
  are created)
- **Kind:** diagnostic, no behaviour change

## Why

The elder has a hole around his ear and seems to miss hair. A capture
where every mesh `group` has its own flat colour shows which part should
cover the ear and whether it is there at all.

## Do

1. When the environment variable `DDVIEW_GROUP_COLOURS` is set, give each
   character mesh an unlit, opaque, double-sided `StandardMaterial` whose
   colour comes from its `group` number (for example hue = group x 0.13,
   wrapped to 0..1). Keep the mesh, skinning and position exactly as now.
2. Log one line per actor listing `group -> colour` so the picture can be
   read.
3. Without the variable nothing changes.

## Run

The cutscene command from README.md with `DDVIEW_GROUP_COLOURS=1`, at
frames 800, 1010 and 1200 (three runs), saved to
`E:\ddda-re\notes\steps\s02\`.

## Done when

The three captures exist, the group/colour lines are in this file's
Result, and you describe in one sentence per actor which colour sits where
the ear hole was and where the hair should be. No fixes.
