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
2. The player clears 17 and a pawn clears 16.
3. For every worn piece, each of its armour models (id % 1000) has an
   `rArmorPartsOff` record. It hides its parts either always (condition
   < 0) or when a model with id % 1000 = condition is also worn.

The masks start from all bits set (*assumed* model default).

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
- **In `ddview`:** each weapon keeps its own small skeleton (bone ids 0-2,
  which are not body joints) under an anchor on that joint.
- **Not applied:**
  - the second model of a dagger pair (0x00bc1c30);
  - the body-edit offset correction of rows on joints 11 / 13
    (0x0080bcb0).

## Morphs (not applied)

The game shapes heads and bodies with joint offsets, not separate models:
- **rFaceEdit** (0x00cb90e0): version 0x13, count (101), and that many
  vec3.
- **rBodyEdit** (0x00cad170): one vec3, a flag, a count and vec3s.
- **rHumanEdit** (0x00cbb0e0): nine floats, a flag and a list.
- **The save's parametric record** (`mFaceBase`, `mFaceEye`, ...,
  `mHumanArm`, `mBodySlender`) selects per-part presets:
  - `edit\parts\<base|eye|eyeblow|nose|mouth|ear>\..._NNN` (rFacePartsEdit:
    version, part type, count, vec3s);
  - `edit\parts\<arm|body|bodyall|head|leg>\..._NNN` (rHumanPartsEdit:
    version 0x43, type, count, six floats each).
- **The runtime edit** (cPlayerInfo +0x288..+0x290, made by 0x0077d6f0 /
  0x00785500) holds the three edit resources.
- **The application** goes through `uEditModel` (vf08 0x0087e6a0) and the
  model's `rJointEx` (`model\pl\m`). Which joint each entry moves, and
  how the presets combine, was not traced.

Traced so far (character-polish):
- **0x00780fa0 (save -> runtime):**
  - loads the six face presets and five human presets
    (`edit\parts\%s\%s_%03d`);
  - sets rBodyEdit +0x60 / +0x64 from `mBodySlender` / `mBodyMuscle`
    through constant tables;
  - fills around 20 scale and offset floats of the runtime edit (+0xc0..
    +0x110) from the other indices through stack tables.
- **0x0081be30 (rHumanEdit -> controller):** copies 13 slots (scale vec3
  and offset vec3) and nine floats into the human-edit controller.
- **0x00812a10 (rFaceEdit -> face controller):** copies 101 offset vec3.
- **Controller update (0x0081ba40):**
  - 0x0081c330 makes one joint-ex object per joint (ids 1-20 except 5 and
    9, plus 0x42, 0x44, 0x90, 0x96, 0x99);
  - 0x0081c6c0, 0x0081cff0 and 0x0081db50 compute their transforms.
- **Face counterpart:** 0x00812820 -> 0x00812fb0 (1,182 lines of
  decompile, mirrored left/right) and 0x00815c60.

Translating these joint computations is the remaining work. Until then,
every character shows the unmorphed base head and body.

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
- `ddview <nativePC> --lineup DDDA.sav[,npc:N...] [--shot out.png]`: the
  save's Arisen and pawns, and NPC looks, side by side in the bind pose.
- With both `--event` and `--lineup <save>`, the event runs and the
  lineup's save dresses the player.

**Screenshots** (character-polish, 2026-10-09, not committed):
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

## Open

- Morphs: the joint computations listed under Morphs.
- The beard colour formula, and eye, eyebrow and makeup colours, plus the
  skin colour's shader use.
- The item -> piece reader (+0x40); the slot -> actor field order.
- The dagger's second model; the weapon offset correction by body edit;
  shield placement checked against the original.
- NPC mask bits 16/17 (0x00ba7150).
