# play-visuals: fix what --play --save looks like in motion

- **Owner:** claude-7
- **Branch:** `task/play-visuals`
- **Board area:** `play/integration`
- **Status:** open

## Goal

A head screenshot of
`ddview --play --save <save> --goblin --demo --shot-delay 14` showed:
- **long stretched brown planes** coming off the characters during the
  fight: weapon, cloak or cloth meshes binding wrong in attack motions;
- the **camera low and clipping**, with terrain in front of it;
- the **saved spot dark at 12:35** in the canyon. Check it against the
  original at that spot through claude-4 if needed; it may be correct.
- the **female Arisen playing the male motion lists**: use the female
  banks (`motion\pl\f\...`);
- **weapons staying sheathed during attacks**: draw them as the game does.

Fix them with traced rules: skinning and joint mapping for parts and
weapons under the attack motions, the female motion banks, the draw and
sheathe attach switch, and a camera collision/offset like the original's
follow camera (`uCameraGame`, FRAME.md).

## Scope

- `crates/ddview/src/play.rs`, `character.rs`
- `crates/ddlook/**`
- `crates/mtlmt/**`: additive
- `docs/CHARACTERS.md`

## Done when

- Screenshots during the fight show no stretched meshes, the right
  female motions, weapons drawn while attacking, and the camera not
  clipping.
- Fill in Results and report.

## Results

(filled in by the agent)
