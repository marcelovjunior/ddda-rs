# character-polish: skin gaps, morphs, colours, weapons

- **Owner:** claude-7
- **Branch:** `task/character-polish`
- **Board area:** `render/characters`
- **Status:** review

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

Details are in `docs/CHARACTERS.md`.

**Done.**
- **Skin gaps fixed.** Character materials now use the traced shading
  description: two-sided cloth, alpha test and blend, and albedo colours.
  The gaps were back-face culling and black alpha cut-outs.
- **`--event` with `--lineup <save>`** now uses the save for the player.
  `--lineup` also accepts `npc:N`.
- **The player is no longer naked without `--save`:** a stand-in Arisen
  is built from the game's first built-in pawn of that sex (romPawnInfo).
  This is a viewer choice.
- **Colours traced** (save -> runtime 0x00780fa0, editPattern pallets).
  - Applied: hair colour (× 0.5 + 0.2, 0x008407f0) and skin colour
    (approximation).
  - Hair and beard numbers now go through the editPattern
    `mHairId` / `mBeardId` tables.
  - The beard is male-only (traced, replacing the earlier guess).
- **Weapons:**
  - paths 0x00bc1a00;
  - attach rows 0x00c1bad0 and table 0x01872bc0 (joint, rotation,
    offset);
  - rotation order from uCnsAttach 0x00859bb0;
  - attached in `ddview` with their own skeletons.
  - Swords, daggers, a bow and a staff hang where expected.
- **Tests:** `ddlook` has 3 unit tests (variants, mask, weapon paths) and
  5 install tests (NPCs, armour paths, edit pattern, save looks with
  weapons, stand-in looks).

**Not done.**
- **Morphs.** The chain is traced down to the joint computations:
  0x0081c330 picks the joints, 0x0081c6c0, 0x0081cff0 and 0x0081db50
  compute the body, and 0x00812fb0 / 0x00815c60 the face, about 2,000
  lines of decompile. Those are not translated, so heads and bodies are
  unmorphed. This needs its own brief.
- **Colours:** the beard colour formula (left untinted), and eye,
  eyebrow and makeup colours.
- **Weapons:** the dagger's second model, the body-edit offset
  correction, and shield placement checked against the original.
