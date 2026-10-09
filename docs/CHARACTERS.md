# Characters: how a look is assembled

How the game builds a character's appearance, and what `ddlook` (headless)
and `ddview` (`character::spawn_assembled`) do with it. Traced in the
2017-12-13 Steam exe (read-only Ghidra). Labels: **traced**, *inferred*
(from data, with the evidence named), *guessed*.

## The builder: `uActorModelPl::vf05` 0x0083e010

`uActorModelPl` (event actors) shares its model code with the player and
pawns. The unit type (+0x1290) is 0 for the player, 1 for a pawn and 2 for
an NPC. The look fields are:

| Actor field | Meaning | NPC source (rEditPawn) |
|---|---|---|
| +0x1324 | sex: 1 female, else male (0x0083fa60) | +0x60 |
| +0x139c | NPC armour set, -1 none | +0x64 |
| +0x1330..+0x1338 | weapons (uWeaponPl, 0x00840180..) | +0x84.. |
| +0x1358, +0x135c x2, +0x1364 x5 | equipment pieces | +0x90.. |
| +0x1378 / +0x137c | hair / beard | +0xb0 / +0xb4 |
| +0x13b0..+0x1408 | colours and scales (22 floats) | +0xc0.. |
| +0x1458 / +0x145c / +0x1460 | rHumanEdit / rBodyEdit / rFaceEdit | paths in the edit |

- **NPC (type 2):** the NPC's home stage comes from the NPC ledger
  (lookup 0x004769c0, field +6). Then `edit\stSSS\nNNN\stSSS_nNNN`
  (rEditPawn, loader 0x00cb5ab0) is copied in by 0x008442d0.
- **Player:** 0x00843ed0 copies the player's runtime edit (the same field
  order).
- **Variants** of the NPC armour set: 0x00ba6dd0, for unit numbers 999,
  0x3e3 and 0x3e4 only, and 0x00ba6950.

### Models

**Base body** (0x0083fa60): `model\pl\f\f_base\f000\f000` for sex 1,
otherwise the male `m000`. Its skeleton (224 joints) is the character's
skeleton.

**Pieces, hair, beard** (0x008407f0):
1. The id (category << 24 | number) is looked up in an `rArmorTable`:
   `m_parts` for sexes 0 and 1 (sGameSys +0xbe238), `h_parts` for hair
   and beard (+0xbe23c).
2. Its model ids go into `rArmorModel` by sex: `m_parts`, `f_parts`,
   `h_parts` (+0xbe22c[sex]).
3. Model kind 0 or 1 selects one of the actor's two model slot lists
   (0x00843820 / 0x008436c0).

**NPC armour set** (0x00841960): `rArmorTableNpc` (+0xbe24c[sex]) record
-> up to two `rArmorModelNpc` ids (+0xbe244[sex]). These are
`model\pl\npc\...` outfits.

The per-sex slots holding the `m`, `f` and `h` files in that order is
*assumed*: the loader that fills them was not traced.

### Formats (readers in `ddlook`)

| Class | Magic, version | Layout | Loader |
|---|---|---|---|
| rEditPawn | `edp`, 0x20 | 4 i16, 2 f32, 3 strings, 3+8 i32, hair, beard, 22 f32, 3 u32, 2 bytes, u16 | 0x00cb5ab0 |
| rArmorTable | `atr`, 1 | count, size; {id, a, b, n, n x model id} | 0x00cabbc0 |
| rArmorModel | `amr`, 0x10 | infos, blends; {id, kind, path} | 0x00caab30 |
| rArmorModelNpc | `nmr`, 0x10 | count; {id, kind, path} | 0x00cab1c0 |
| rArmorTableNpc | `NTR`, 0 | size, count; {id, u32, models, colours (2 u32 + RGBA), on u16s, off u16s} | 0x00cac350 |
| rArmorPartsOff | `aor`, 0x10 | count, size; {id, n, n x (s16 condition, u16 part)} | read from data and the consumer 0x00bc1060 |

Every armour model path that `m_parts` names exists in the install, except
one empty path, which means no model (test `every_armour_piece_resolves`).

## One skeleton

- **Joint ids:** every part model (outfit, hair, beard, equipment)
  carries bones with the same joint ids as the base body. Hair has 68
  bones, a beard 59, an outfit all 224.
- **What `spawn_assembled` does:**
  - binds each part bone to the base joint with the same id;
  - adds joints that a part has and the base lacks under their parent;
  - skins each part with its own rest-pose inverse binds.
- **Animation:** one `Playback` animates all of them.
- **Status:** *inferred* to match the game's sharing of joint matrices.
  The exe's skinning of slot models was not traced.

## Part masks (no skin bleed)

**Groups.** A mesh's part group (`mtmod::Mesh::group`, u16) selects a bit
of the character's 512-bit part mask (+0x110). A mesh draws only when its
bit is set. Groups seen:
- base body 0-31;
- hair 50;
- beard 66;
- NPC outfits 300 and 301.

**NPC with an armour set** (0x00841e20):
1. Clear parts 200-511.
2. Set the set's `on` parts.
3. Clear its `off` parts.

For st200 ev10:
- NPC 1: on 300, off 24.
- NPC 2: on 300 and 301, off 1, 8, 9, 10, 16, 17, 18, 30.

**Dressed characters** (0x008449b0 with 0x00bc1060):
1. Set parts 0-199.
2. The player ANDs the first word with 0x7ffdffff (clears 17 and 31), a
   pawn with 0x7ffeffff (clears 16 and 31).
3. For every worn piece, each of its armour models (id % 1000) has an
   `rArmorPartsOff` record. It hides its parts either always (condition
   < 0) or when a model with id % 1000 = condition is also worn.

The masks start from all bits set (*assumed* model default).

**Render passes** (uModel::vf26 0x00ea5320). A mesh draws in a pass only
when, besides its part bit:
- its disp flag (flags bit 0) is set;
- its LOD byte has the current LOD's bit;
- `draw_mode & pass mask` is non-zero. The mask is the u16 that
  0x00fedb80 stores at +0x16a when a pass begins. The scene pass
  "Common" pushes 1 (0x00daf0a8); ShadowCast pushes 8 or 0x200
  (0x00ede86c); Velocity 0x20, Outline 0x1000.

So in the colour pass, `draw_mode` bit 0 decides. The base bodies and
outfits carry a low-poly full-body mesh with `draw_mode` 0xe6c8 (a
shadow caster) and shells with 0x1020 (outline and velocity). Drawing
them in colour was the skin showing through sleeves and leggings, and
the white patches on the elder's robe.

## Materials

Character materials use the traced material description
(`mtmrl::shading::describe`, SHADING.md), as the stage does:
- albedo map with fAlbedoColor × fDiffuseColor;
- culling from the rasterizer state (cloth is mostly two-sided);
- the material header's alpha test, or alpha blending.

Before this, cloth was culled from behind and its cut-outs drew black.
That showed the body through the gaps head saw at ev10 frame 295, on the
shoulders and arms of the elder and Quina.

## Colours (traced: 0x00780fa0)

The save -> runtime edit conversion 0x00780fa0 reads
`etc\editPattern\editPattern` (rEditPatternData, sGameSys +0xbe254;
`ddlook::pattern`):

| Runtime edit field | Source | Pallet |
|---|---|---|
| +0x50 / +0x60 eye R / L | `mColorEyeR` / `mColorEyeL` | 2 |
| +0x70 hair | `mColorHair` | 0 |
| +0x80 skin | `mColorBody` | 1 |
| +0x90 beard | `mColorBeard` | 0 |
| +0xa0 eyebrow | `mColorEyebrow` | 0 |
| +0xb0 makeup | `mColorMakeup` | 2 |

- **Index rule:** an index of 63 or more takes colour 0.
- **NPCs:** their edit floats carry the same layout (eye R, eye L, hair,
  skin, beard, ...).
- **Hair colour as the game uses it** (0x008407f0 -> 0x008438f0): the hair
  value × 0.5 + 0.2. It is applied in `ddview` as an albedo multiplier on
  hair models (*approximation* of the material constant).
- **Skin colour:** applied raw to the base body's materials. How the
  shader uses it was not traced (*approximation*).
- **Beard colour: not applied.** The beard branch of 0x008407f0 sets no
  colour, and using the hair formula on the elder's beard turned it
  green.
- **Not applied:** eye, eyebrow and makeup colours.

## Weapons (traced)

- **Ids:** the save's equipment slots 0 and 1 hold the primary and
  secondary weapons; slots 2-11 hold armour. An item record's +0x40 holds
  the weapon id or armour id.
- **Model** (0x00bc1a00): `model\wp\<kind>\<kind>NNN[_x]\...`:
  - number = id % 100, with 20 read as 999;
  - `_a`, `_b`... per hundred;
  - kinds by category: 1 one, 2 mac, 3 two, 4 dag, 5 stk, 6 wan, 7 war,
    8 sld, 9 bld, 10 bow, 11 bbw, 12 mbw, 13/14 qui;
  - 15/16 qui with `arr` files.
- **Sheathed attachment:** the row is picked by category (0x00c1bad0),
  from table 0x01872bc0. Each row has a joint id, a rotation in degrees
  and an offset in cm (0x0080bcb0).
- **Hang-up:** uCnsAttach (0x00859bb0, mode 1) builds the rotation as Rx ×
  Ry × Rz for row vectors, so X is applied first.
- **Normalised joint:** uCnsAttach::vf24 normalises the joint's world
  rows before it applies the attach matrix (+0xac clear), so weapons keep
  their size on a morphed arm.
- **In `ddview`:** each weapon keeps its own small skeleton (bone ids 0-2,
  which are not body joints) under an anchor. The anchor is placed every
  frame from the joint's normalised world matrix.
- **Shields:** row 3 is joint 11, the left forearm (ids 9-12 run clavicle,
  upper arm, forearm, hand on +x). In the bind pose the forearm's outer
  side faces up, so the shield lies flat above it. With an idle motion
  (`--pose`), it hangs on the outside of the forearm.
- **Offset correction** (0x0080bcb0): rows on joint 0xb add
  `(f84 - 1) x 15` to offset x; rows on 0xd add `±(f78 - 1) x 15`, with
  the sign of x. f84 and f78 are human-edit controller floats from the
  untranslated skeleton measurement (1.0 here), so the correction is
  zero.
- **Row 23** replaces row 3 in some action states (0x0080c110); not used
  at rest.
- **Not applied:** the second model of a dagger pair (0x00bc1c30).

## Morphs (`ddlook::morph`, `ddlook::face`)

The game shapes heads and bodies with constraints on the shared skeleton's
joints (uCnsEdit), not with separate models. Each character model has
three edit controllers, fed by three edit resources:
- **rHumanEdit** (0x00cbb0e0): nine floats, a flag and 13 (scale, offset)
  slots.
- **rFaceEdit** (0x00cb90e0): version 0x13, count (101), and that many
  vec3.
- **rBodyEdit** (0x00cad170): not translated (see Open).

NPCs ship all three (edp paths). For a save character, 0x00780fa0 builds
them from the parametric record:
- `edit\parts\<bodyall|body|head|arm|leg>\..._NNN` (rHumanPartsEdit)
  from `mHumanBodyAll`, `mHumanBody`, `mHumanHead`, `mHumanArm`,
  `mHumanLeg`;
- `edit\parts\<base|eyeblow|eye|nose|mouth|ear>\..._NNN`
  (rFacePartsEdit: version 0x13, part, count, vec3s) from `mFaceBase`,
  `mFaceEyebrow`, `mFaceEye`, `mFaceNose`, `mFaceMouth`, `mFaceEar`.

NNN is the field plus 100 for a female, unless `mHumanBodyAll` is 200 or
more (EDI in 0x00780fa0); the female presets are the 1NN files. A missing
preset resets its slots (0x00cbc810) or zeroes its components
(0x00cb9320).

### Body (`ddlook::morph`)

- **Controller** (0x0081b0d0 set-up, 0x0081be30 copy):
  - 13 slots at +0x250, with rest values from table 0x01832aa0;
  - 33 entries at +0x720 (table 0x017ed8f8);
  - the nine floats at +0xf70..+0xf90.
- **Computation:** the slot clamps 0x0081c6c0 (skipped when the record's
  flag is set), the entries 0x0081cff0, then the joints 0x0081db50.
- **Joints** (0x0081c330): ids 1-20 except 5 and 9, and 0x42, 0x44, 0x90,
  0x96, 0x99. Types:
  - 0xb for 2, 3, 4, 6, 10 (0x0086d180): an absolute size, scale x child
    over the world axis lengths;
  - 9 for 8 and 12;
  - 0xc for 0x96 and 0x99 (approximated as a plain scale);
  - 0 for the rest.
- **Model scale:** entry 0's scale is uModel +0x60, the skeleton root.
- **Not translated:** the nine floats of a save character come from a
  skeleton measurement (0x00cbbf00), so they are 1.0 here.

### Face (`ddlook::face`)

- **Controller** (cFaceEdit, 0x00811780 / 0x00811a90):
  - 101 markers at +0x280 (table 0x01831df0): kind, rest position, edit
    value, adjustment. A marker reads as an offset (kind 0), a scale
    (kind 1, around 1) or a rotation (kind 2): default + value +
    adjustment.
  - 116 controllers at +0x2210 (table 0x017edc70): joint id, offset,
    scale, rotation, and a link to the parent joint's controller.
- **Parameters:** ten floats at +0x240..+0x274. 0x0080bb10 fills them
  from the save's sliders `mOfsXFaceEye`, `mOfsYFaceEye`, `mOfsYFaceNose`,
  `mOfsYFaceMouth`, `mOfsYFaceEar`, `mOfsZFaceEar`, `mSclFaceEye` and
  `mSclFaceNose`, through five-step tables of 0x00780fa0. NPCs keep zeros.
- **Update** (0x00812820):
  1. 0x00812fb0 computes marker adjustments.
  2. 0x00815c60 computes the controllers.
  3. 0x0081a370 sums each controller with its parents (offsets and
     rotations add, scales multiply) and sets the joint's uCnsEdit. Each
     part is set only away from its default; rotation is stored in
     degrees.
- **Joints:** face uCnsEdit records exist for ids 0x50-0x95 and 0xd2-0xfe
  (0x00812c80). The eye joints get other types (0x00812330), which are not
  translated, and the viewer leaves them alone:
  - 0x51 and 0x58: type 0x10;
  - 0x52-0x57 and 0x59-0x5e: type 0xd;
  - 0xe2-0xe3: type 0xf;
  - 0xe4-0xe9: type 0xe.
- **How the update was translated:** `face_controllers.rs` is generated
  from the instructions of 0x00812fb0 and 0x00815c60. Ghidra's decompile
  loses the register arguments of their helpers, so the functions were
  executed symbolically instead: integers concrete, floats as
  expressions.
  - Marker kinds and controller joint ids are static, so their branches
    are resolved.
  - Float compares become `if`s.
  - The f32 operation order is kept.
- **Tested** (`matches_the_emulated_exe`) against the exe itself:
  constructor, set-up and update ran in an x86 emulator on DDDA.exe's
  code with pseudo-random inputs, and the outputs agree bit for bit. A
  neutral edit changes no joint.
- **Assumed:** the bss vector 0x018cfe80 is the zero vector.

### Applying them (uCnsEdit::vf24 0x00860ac0)

1. Type 0 builds `C = diag(scale) x Rx x Ry x Rz` (row vectors, angles from
   +0x70 in degrees, X applied first), with the offset as the translation
   row.
2. It replaces the joint's world matrix W with `C x W`.
3. Constraint weight +0x44 is 1 (0x0112ebb0).

In `ddview` (`character::apply_morph`):
- Each frame, after the motion, the world matrices are recomputed parents
  first, with each joint's edits applied in column form:
  `W x T(offset) x R x S`.
- The model scale is the root.
- The result is written back as local transforms. Shear from non-uniform
  parent scales is lost in that TRS decomposition.
- Joint 0x90 has both a body and a face edit; applying the body edit
  first is *assumed*.

### Results

Save characters from the maintainer's save:

| Character | Model scale | Face joints changed |
|---|---|---|
| Arisen (female) | (1.0, 0.859, 1.0) | 92 |
| pawn0 | (0.9, 0.989, 0.9) | 100 |
| pawn1 | (1.0, 0.918, 1.0) | 99 |
| pawn2 | (1.0, 0.960, 1.0) | 100 |

NPC 1 and NPC 2 change 100 and 99 face joints. The four save faces all
differ (test `face_morphs_differ`).

## The save's Arisen and pawns (`ddlook::save`)

The slot is `mPlayerDataManual`. The Arisen is `mPl` and the party pawns
are `mCmc`, each with `mEdit` (`cSAVE_DATA_EDIT`) and `mParam`.

- **Sex:** `mGender`.
- **Pieces:** every equipped item (`mEquipItem`) maps to the armour id at
  item record +0x40. This is *inferred*: 640 records of the shipped
  `itemList` hold an `m_parts` armour-table id there, and the reader of
  the field was not found.
- **Hair** (traced, 0x00780fa0): `0x0f000000 | mHairId[mHairNo]` from
  the editPattern table.
- **Beard** (traced): `0x10000000 | mBeardId[mBeardNo]`, for male
  characters only.
- **Equipment slots:** 0 and 1 are weapons, 2-11 are armour.
- **Stand-in without a save** (`ddlook::save::template_look`): the first
  built-in pawn of `etc\editPattern\romPawnInfo` of the configured sex.
  This is a viewer choice, not a game rule; the original's player actor
  copies the real player.

The maintainer's save: the Arisen and three pawns all render dressed.
They wear 7-10 models each, and the parts-off records hide 5-17 body
parts each (test `saved_arisen_and_pawns_are_dressed`, which prints no
personal data).

## Viewer

- `ddview <nativePC> --event 22 [--save DDDA.sav]`: event actors assembled
  as above. With `--save`, the player actor is the save's Arisen and pawn
  actors are its pawns.
- `ddview <nativePC> --lineup DDDA.sav[,npc:N...] [--pose <rMotionList>[:slot]]
  [--start-frame F] [--shot out.png]`: the save's Arisen and pawns, and NPC
  looks, side by side, in the bind pose or held at frame F of a motion
  (e.g. `motion\pl\m\m00\m0000_co\m0000_co:0`, the idle).
- With both `--event` and `--lineup <save>`, the event runs and the
  lineup's save dresses the player.
- `ddview <nativePC> --play --save DDDA.sav [--goblin]` (play.rs):
  - **Party:** the save's Arisen (seat 0) and main pawn (`mCmc[0]`, sim
    seat 4) are built as above, with morphs and sheathed weapons.
  - **Place:** they stand at `mPlPos` / `mCmcPos[0]` with angles
    `mPlAng` / `mCmcAng[0]`, from slot `mPlayerDataManual`.
  - **Movement:** the shared skeleton plays ddsim's states. Common motions
    come from bank 0 / 6; attacks come from the weapon's `_co` / `_at`
    lists (sword `m0001`, staff `m0007`).
  - **Goblins** play `ddenemyrt`'s motion banks.
  - **Motions:** female characters play the male lists, as in the
    original. The exe names only `motion\pl\m\m00\...` player lists
    (110 strings), and the install has no other player motion lists.
  - **Drawn weapons:** each weapon has a drawn joint (`WeaponPart::hand`).
    - The primary goes to joint 0x96 and the secondary to 0x99
      (0x00840180 / 0x008404b0 -> 0x00c1b2d0), with no offset (0x00c1b460
      resets the constraint, 0x00859bb0).
    - A shield keeps its sheathed row (0x0080be30, category 8).
    - The attack motions key 0x96 / 0x99 into the hand.
    - The original draws while uPlayer +0x273C has bits 0x200 and
      0x400 / 0x800 (uPlayerBase::vf93 0x00B63F70). Those bits' writers are
      not traced, so the viewer draws during attack states (*inferred*).
  - **Blending:** a motion change freezes the unmorphed pose. Freezing the
    morphed one re-applied the morph at every change, so attack chains
    stretched meshes.
  - **Follow camera:**
    - Distance 230 cm, target 140 cm above the feet, near plane 96 cm,
      fov 55 + the CameraFov setting. These are measured on the seven
      recorded reference poses, not traced.
    - The eye is pulled in to the first surface between target and eye,
      5 cm off it (`mMargin` of `param\cam\pl\*`).
    - Pitch is limited to 60 degrees above and 30 below
      (`mAngLimitXMin` / `Max`, sign inferred).
  - **Viewer choices:**
    - enemy materials' 8x8 `d_` placeholder maps are swapped for the full
      map of the same name (the game's swap is not traced).

**Screenshots** (character-morphs, 2026-10-09, not committed):
- **lineup of the save:**
  - The four party members have their own heights and proportions.
  - No skin shows through the sleeves or leggings.
  - Shields lie flat on the left forearm in the bind pose. With
    `--pose` they hang on the outside of the forearm.
- **lineup of the save plus `npc:1,npc:2`:** six distinct faces. The
  elder's robe has no white patches.
- **Close-ups:** a pale band shows over every brow (eyebrow colour, see
  Open).

**Earlier screenshots** (character-polish):
- **lineup of the save plus `npc:1,npc:2`:** six characters dressed, with
  no gaps.
  - Swords and a dagger hang at the hip.
  - A bow sits on the back.
  - Shields lie on the left upper arm (row 3, joint 11), not checked
    against the original.
  - The Arisen's staff sits on the back (joint 2).
  - Hair is in the save's colours. Quina's hair is alpha-tested.
- **ev10 frame 295 with `--lineup <save>`:** the saved Arisen dressed
  behind Quina and the elder, with no skin gaps.
- **ev10 frame 1300 without a save:** the stand-in Arisen dressed.

**Earlier screenshots** (character-assembly):
- **lineup:** four characters dressed in tunics, trousers, boots, a cloak
  and a mail coif. Hair is from the save. No skin shows through the
  covered parts. The heads are the unmorphed base heads.
- **ev10 frame 200:** Quina and the elder in their outfits, with
  hair-coloured hair and the elder's beard on the shared skeleton.
- **ev10 frame 1300 with `--save`:** the saved Arisen, dressed, stands
  behind them.

## Eyebrow appearance (cutscene-visuals)

Translated 0x00843940: material groups 5/6 receive eyebrow RGB multiplied
by edit alpha, with output alpha one; group 6 also receives texture/UV
controls. `rMaterial::vf09` 0x0112e4d0 preserves MRL bits 23..30 as the
runtime group. Existing blend/alpha-test/culling state is retained.
0x01067570 and 0x010675f0 replace only translation and diagonal UV matrix
elements. Texture indices 1..3 select installed pl_eyebrowNN_BM textures;
zero retains the material texture and 4 uses the original UV hide path.
Normal-map replacement remains unimplemented, along with general
character normal mapping.

NPC apply 0x008442d0 copies raw edit hair RGBA (floats 8..11) to eyebrow
RGBA, offsets/scales from floats 18..21, and texture from f118[0]. Saved
characters use mColorEyebrow palette zero and mFaceEyebrow selection from
editPattern (0x00780fa0 / 0x00cb5740). Bulk palette and 100 ordinary /
8 fixed preset records are read at runtime; save conversion selects the
first five fixed records. Regressions cover color multiplication, UV
hide, slider/fixed offsets, installed presets and texture resolution.

MRL depth writes and blending are independent. In particular hair104's
first material uses `BSBlendAlpha` with `DSZTestWrite`, while the base
eyebrow overlay uses `BSBlendAlpha` with `DSZTest`. The material pipeline
now keys on the authored depth-state bits and converts the comparison
to reverse Z (LessEqual becomes GreaterEqual), preserving writes even
for blended materials. Blended alpha tests also retain the MRL reference
and comparison (0x0112e4d0 / 0x00fee320). This corrects a material-state
mismatch; it is not proof that all facial visibility defects are solved.

## Open

- Morphs not translated:
  - rBodyEdit (0x0080add0 / 0x0080aed0; `mBodySlender`, `mBodyMuscle`,
    bust scale);
  - the nine human-edit floats of a save character (0x00cbbf00);
  - the eye constraint types 0xd, 0xe, 0xf and 0x10;
  - uCnsEdit types 9 and 0xc (approximated).
- The beard colour formula, and eye and makeup colours, plus the
  skin colour's shader use.
- The item -> piece reader (+0x40); the slot -> actor field order.
- The dagger's second model.
- NPC mask bits 16/17 (0x00ba7150).
