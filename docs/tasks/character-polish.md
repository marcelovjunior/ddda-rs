# character-polish: skin gaps, morphs, colours, weapons

- **Owner:** claude-7
- **Branch:** `task/character-polish`
- **Board area:** `render/characters`
- **Status:** open

## Goal

Follow-up to character-assembly. A head smoke run of
`ddview --event 22 --lineup <save>` at frame 295 showed:
- skin visible through gaps between clothing parts (shoulders and arms
  of the elder and Quina);
- the player stand-in naked (event without `--save`);
- `--lineup` apparently ignored when combined with `--event`.

Fix those, then:
- **Morphs:** `uEditModel` 0x0087E6A0 and `rJointEx`, so faces and bodies
  differ.
- **Colours:** skin, eye and hair palettes behind the save's `mColor*`.
- **Weapons:** models on their joints.

## Scope

- `crates/ddlook/**`
- `crates/ddview/src/character.rs`
- `crates/ddview/src/event.rs`
- `crates/mtmod/**`
- `docs/CHARACTERS.md`

## Done when

- No skin gaps on the dressed characters in the lineup or the event.
- Morphs and colours are applied from the save, traced.
- Weapons are attached.
- Screenshots are described.
- Fill in Results and report.

## Results

(filled in by the agent)
