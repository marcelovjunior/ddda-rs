# play-visuals: fix what --play --save looks like in motion

- **Owner:** claude-7
- **Branch:** `task/play-visuals`
- **Board area:** `play/integration`
- **Status:** review

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

Commits on `task/play-visuals`: bdf4f13, 4ae90cd, a1a91e6, 4d3ff62 and the
results commit.

- **Stretched planes.** The cause was the blend, not the skinning.
  - At each motion change, `character::animate` froze the current local
    pose, which already had the body/face morph applied.
  - The morph pass then ran over the blend again, so every motion change
    compounded the morph.
  - Attack chains change motion every few frames, so joints blew up.
  - Now the blend freezes the unmorphed pose (`Morph::base`).
  - Fight screenshots at 6, 8, 11 and 14 s show no stretched meshes. The
    lineup posed in sword, staff and wand-shot motions was already clean.
- **Female motions.** No female banks exist, so female characters play the
  male lists, as the original does.
  - The exe names only `motion\pl\m\m00\...` player motion lists (110
    strings).
  - The install has no other player `rMotionList`.
- **Drawn weapons.** Each weapon now has a drawn joint
  (`ddlook::weapon::WeaponPart::hand`).
  - Primary 0x96, secondary 0x99 (0x00840180 / 0x008404b0 ->
    0x00c1b2d0), with the attach constraint reset (0x00c1b460, 0x00859b10).
    A shield stays on its row (0x0080be30).
  - Attack motions key those joints into the hand.
  - The viewer draws during attack states (*inferred*: the original's
    flags, uPlayer +0x273C bits 0x200 | 0x400 / 0x800 read by
    uPlayerBase::vf93 0x00B63F70, have writers that are not traced).
  - Screenshots show the pawn's sword in hand and the Arisen's staff in
    hand while attacking, sheathed otherwise.
- **Camera.**
  - Measured on the seven recorded reference poses of the original:
    - eye 230 cm from the target in every one;
    - target 127-144 cm above the feet (140 used);
    - fov 55 (+ CameraFov setting);
    - near plane 96 cm.
  - Pitch starts at 20 degrees looking down (the references show 12-30).
    Limits are 60 up / 30 down from `mAngLimitX*` in `param\cam\pl\*`.
  - A cast from the target pulls the eye in, 5 cm off the first surface
    (`mMargin`).
  - The camera no longer sits low or clips terrain.
  - The parameter list record that sets 230 / 140 was not identified.
    The player lists hold 410 / 125 etc., which are other camera modes.
- **Darkness.** Unverified.
  - The save spot is a canyon floor in the sun's shadow at 12:35. The lit
    rock above and the sky look right.
  - One known difference: ddview lights characters with Bevy's ambient,
    not the stage SH (main.rs / stage.rs, outside this scope).
  - Head declined a live load (it would discard the paused session).
  - The ORACLE.md entry was sent to claude-4 (#1192): "Canyon darkness at
    the save spot. Precondition: maintainer present; ok to load the save.
    Load DDDA.sav (manual slot: st100 58m49n, mPlPos (-5643, 40161,
    83345), 12:35, weather 0), take one watcher screenshot right after the
    load without moving, and compare floor and character brightness with
    `ddview --play --save`."
- **Tests:** ddlook `drawn_joints`; ddview
  `the_follow_camera_keeps_its_distance_and_stops_at_walls`; the ddlook and
  ddview suites pass.
