# character-morphs: faces and bodies from the edit data

- **Owner:** claude-7
- **Branch:** `task/character-morphs`
- **Board area:** `render/characters`
- **Status:** review

## Goal

1. **Morphs.** Translate the joint-morph chain claude-7 traced:
   - joint selection `0x0081c330`;
   - body `0x0081c6c0`, `0x0081cff0`, `0x0081db50`;
   - face `0x00812fb0`, `0x00815c60`.

   Do it in `ddlook`, headless with tests, then apply it in the viewer, so
   each saved character and NPC has its own face and body.
2. **Two fixes** seen in the head lineup screenshot (`--lineup <save>`):
   - skin-texture patches still show through sleeves and leggings
     (Arisen 2 and 3 in the lineup): part masks or the material
     alpha/order;
   - shields float off the left upper arm: check the attach joint and
     offset against the original. claude-4's oracle is paused; ask head
     if you need a live read.

## Scope

- `crates/ddlook/**`
- `crates/ddview/src/character.rs`
- `crates/ddview/src/event.rs`
- `crates/mtmod/**`
- `docs/CHARACTERS.md`

## Done when

- The lineup shows four distinct faces and bodies matching their edit
  data, with no skin patches and shields on the arm.
- The morph math is tested against values computed from the exe's rules.
- Fill in Results and report.

## Results

Commits on `task/character-morphs`: 36514ed (body morphs in ddlook), 7aa64e8
(viewer morphs, pass-mask fix, weapon anchors, `--pose`), 7076252 (face
morphs), plus docs. Details are in docs/CHARACTERS.md, under Morphs, Part
masks and Weapons.

1. **Body** (`ddlook::morph`): rHumanEdit and the five human presets.
   - Translated: controller set-up 0x0081b0d0, copy 0x0081be30, clamps
     0x0081c6c0, entries 0x0081cff0, joints 0x0081db50, joint choice
     0x0081c330.
   - Tests: neutral is identity; a +18 cm height gives model y 1.1; the
     clamps; the arm-length chain; preset slots.
   - Fix: a female's preset number is the field + 100 (0x00780fa0), which
     the first version missed.
2. **Face** (`ddlook::face`): rFaceEdit, the six rFacePartsEdit presets,
   the slider parameters, and 0x00812fb0 / 0x00815c60 / 0x0081a370.
   - Method: the two big functions lose register arguments in the
     decompile, so they were translated from the instructions by symbolic
     execution (`face_controllers.rs`, generated).
   - Test: `matches_the_emulated_exe` compares, bit for bit, against
     DDDA.exe's own code run in an x86 emulator (constructor, set-up and
     update) on pseudo-random edits.
   - Not translated: the eye joints' constraint types (0xd-0x10); the
     viewer leaves those joints alone.
3. **Viewer:** `apply_morph` recomputes world matrices each frame with
   uCnsEdit's `C x W` (type 0 with rotation, type 0xb absolute size), body
   then face.
4. **Skin patches:** these were not part masks. Every base body and outfit
   has a low-poly shadow mesh (`draw_mode` 0xe6c8) and outline/velocity
   shells (0x1020).
   - uModel::vf26 0x00ea5320 draws a mesh only when `draw_mode` & the
     pass mask, and the scene pass "Common" pushes mask 1 (0x00daf0a8).
   - The viewer now filters on `draw_mode & 1` and the disp flag. The
     patches on pawns 0 and 1 and the elder's white patches are gone.
   - Also: the player and pawn masks clear part 31 too (0x008449b0).
5. **Shields:**
   - Row 3 is joint 11, the left forearm, and the rotation order matches
     uCnsAttach. In the bind pose the forearm's outer side faces up, so the
     shield lies flat above it, which looked like floating.
   - With `--pose motion\pl\m\m00\m0000_co\m0000_co:0` they hang on
     the outside of the forearm.
   - Weapons now sit on the joint's normalised world matrix (as
     uCnsAttach does), so arm morphs do not scale them.
   - No live read was needed.

**Screenshots** (`--lineup <save>`, with and without `--pose`, and with
`npc:1,npc:2`):
- four party members of distinct build, and six distinct faces;
- no skin patches;
- shields on the forearm.

**Open:**
- rBodyEdit (slender, muscle, bust);
- the nine skeleton-measurement floats of save characters (0x00cbbf00);
- the eye constraint types;
- the eyebrow colour (eyebrows show as a pale band).
