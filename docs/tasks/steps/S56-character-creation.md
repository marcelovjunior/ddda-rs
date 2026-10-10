# S56: Character creation, first milestone (stage 801: name, sex, basic look)

- **Branch:** `step/s56-character-creation`
- **Files:** `crates/ddview` (an editor module; `--charaedit` exists),
  `crates/ddlook` (edit fields -> look), `crates/ddhost` / `ddprogress` /
  `ddstory` (the Arisen's name/edit record in host state and saves), tests
- **Kind:** research + one feature (milestone; the full editor comes later)

## Context

The original's new game goes prologue -> movie -> stage 801 character
creation -> Cassardis. The opening skips 801 today: the Arisen has a default
look and **no name** (S35's header, S43's notice show it empty). Readers
exist (docs/CHARA_EDIT.md, chara-edit; `--charaedit` / `--charaedit-female`
previews; ddlook edit fields from cSAVE_DATA_EDIT), the GUI is uGUIEdit.

## Do

1. Trace stage 801's flow: the uGUIEdit pages/order, the presets, what the
   first milestone needs (sex, name entry, a preset or the basic
   face/body controls), and how the result is stored (edit record + name
   in the Arisen's save data) and handed back (JumpArea to Cassardis).
2. Implement the milestone from the installed GUI/messages: choose sex,
   enter a name (keyboard; the original's character set/limits), pick a
   preset / basic sliders, confirm. Store it as host state (per profile,
   saved with checkpoints/saves, S55 keys) and apply it via ddlook.
3. Feed the name to the GUIs that show it (S35 vocation menu header, S43
   level-up notice) and the look to the Arisen body.
4. Tests: the edit record round-trips through save/load; the chosen look
   applies; the name appears in the level-up notice text.

## Done when

The opening can pass through a stage-801 character creation that sets the
Arisen's sex, name and basic look from the installed editor data, saved
with the profile, shown in the existing GUIs; the Result lists what of
uGUIEdit remains for later milestones.
