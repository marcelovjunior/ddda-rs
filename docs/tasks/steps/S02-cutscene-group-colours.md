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


## Result

Implemented in `70cdcf6` on `step/s02-group-colours` (diagnostic only).
When `DDVIEW_GROUP_COLOURS` exists, every otherwise-drawn character mesh
gets an unlit, opaque, double-sided `StandardMaterial`, with no textures,
using the suggested hue = fract(group * 0.13) palette. The mesh generation,
visibility filters, skeletons, transforms and morphs remain unchanged.
Without the variable the original material construction/cache path runs.
Materials in diagnostic mode are cached by mesh group rather than material
index, so groups sharing an authored material still receive their palette
colour. One sorted/deduplicated legend is emitted per assembled actor;
the event motion path identifies the actor as well as its Bevy root.

Validation:

- `cargo test --release -p ddview --target-dir E:/ddda-rs-wt/m5-cassardis/target`:
  **52 passed, 0 failed, 2 ignored**, 3.41 s (compile 25.19 s).
- `cargo build --release -p ddview --target-dir E:/ddda-rs-wt/m5-cassardis/target`:
  passed, 32.01 s; existing `Args` visibility and `forget_actor` warnings.
- All three prescribed event 22 / hold / unattended / shot-delay 15 runs,
  with the variable set to 1, exited 0 within individual 60 s watchdogs.
  Captures `frame800.png`, `frame1010.png`, `frame1200.png`, corresponding
  `.out.log`/`.err.log` files, and the executable are private under
  `E:/ddda-re/notes/steps/s02/`. All three images visually inspected; their
  HUDs confirm the requested frames. No game process or install changes.

Visual observations (viewer diagnostic, not original-game acceptance):

- **Elder (`npc001_00`, root `2885v0`):** frame 1010 shows a red ear surface
  where the dark opening was, bounded above/back by cyan group 50 hair and
  below/front by blue group 66 beard, with violet group 21 neck underneath;
  cyan hair covers the crown and back in all three captures.
- **Quina (`npc002_00`, root `3161v0`):** cyan group 50 hair covers both ear
  regions and the crown, falling to the shoulders in frames 1010/1200;
  no exposed ear can be assigned a separate colour in these views (frame
  800 predominantly shows the back of her hair).
- **Arisen (`pl_00`, root `2614v0`):** assembled and logged but off-screen
  in all three requested cuts; no ear/hair visual claim is possible.

The palette wraps: group 0 and 300 are both red, and 1 and 301 both yellow.
Therefore colour alone does not distinguish base-body red from clothing
red. This diagnostic also combines opaque alpha and two-sided rendering,
so it cannot isolate alpha/material versus culling as the cause. It shows
that ear/hair geometry is present; no rendering fix or missing-part claim
is made. Next: S10 should correlate S01's model/mesh list with these
regions and isolate the authored material/culling state.

All three runs printed the same complete actor legends:

```text
[group-colours] actor=2614v0 model=model\pl\m\m_base\m000\m000 motion=event\st200\ev10\list\motion\pl_00 0 -> #FF0000, 6 -> #AD00FF, 7 -> #FF008A, 11 -> #00FF94, 20 -> #0066FF, 21 -> #6100FF, 22 -> #FF00D6, 23 -> #FF000F, 24 -> #FFB800, 50 -> #00FFFF, 146 -> #FF001F, 161 -> #FF006B
[group-colours] actor=2885v0 model=model\pl\m\m_base\m000\m000 motion=event\st200\ev10\list\motion\npc001_00 0 -> #FF0000, 1 -> #FFC700, 2 -> #70FF00, 3 -> #00FF57, 4 -> #00E0FF, 5 -> #001AFF, 6 -> #AD00FF, 7 -> #FF008A, 8 -> #FF3D00, 9 -> #FAFF00, 10 -> #33FF00, 11 -> #00FF94, 12 -> #00A3FF, 13 -> #2400FF, 14 -> #EB00FF, 15 -> #FF004D, 16 -> #FF7A00, 17 -> #BDFF00, 20 -> #0066FF, 21 -> #6100FF, 22 -> #FF00D6, 23 -> #FF000F, 50 -> #00FFFF, 66 -> #0085FF, 300 -> #FF0000
[group-colours] actor=3161v0 model=model\pl\f\f_base\f000\f000 motion=event\st200\ev10\list\motion\npc002_00 0 -> #FF0000, 2 -> #70FF00, 3 -> #00FF57, 4 -> #00E0FF, 5 -> #001AFF, 6 -> #AD00FF, 7 -> #FF008A, 11 -> #00FF94, 12 -> #00A3FF, 13 -> #2400FF, 14 -> #EB00FF, 15 -> #FF004D, 20 -> #0066FF, 21 -> #6100FF, 22 -> #FF00D6, 23 -> #FF000F, 24 -> #FFB800, 31 -> #FF2E00, 50 -> #00FFFF, 300 -> #FF0000, 301 -> #FFC700
```
