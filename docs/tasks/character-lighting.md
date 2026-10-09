# character-lighting: characters lit by the stage, as the original

- **Owner:** claude-3
- **Branch:** `task/character-lighting`
- **Board area:** `render/shading`
- **Status:** open

## Goal

In `--play --save` (and `--event`), characters are lit by Bevy's ambient,
not the stage's lighting, and the canyon at the saved spot looks very
dark (head screenshot after play-visuals). Light characters the way the
game's character shaders do:
- sun with shadows;
- the SH/hemisphere ambient from `mtweather::lighting`;
- the light buffer;
- any character-specific terms (rim, skin) traced from the character
  shader family.

Also check the overall darkness at the saved spot (st100 58m49n, 12:35,
fine weather) with the traced rules; a live comparison needs the
maintainer, so mark it.

## Scope

- `crates/ddview/src/stage.rs`, `character.rs` (materials only;
  coordinate with claude-7)
- the shaders
- `crates/mtweather/**`
- `docs/SHADING.md`

## Done when

- Characters use the stage lighting in `--play --save` and `--event`.
- Screenshots are described.
- Fill in Results and report.

## Results

(filled in by the agent)
