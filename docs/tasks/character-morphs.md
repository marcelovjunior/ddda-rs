# character-morphs: faces and bodies from the edit data

- **Owner:** claude-7
- **Branch:** `task/character-morphs`
- **Board area:** `render/characters`
- **Status:** open

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

(filled in by the agent)
