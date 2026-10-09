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

## Colours

- **Hair and beard:** edit floats 8-10 × 0.5 + 0.2 (actor
  +0x13d0..+0x13d8). 0x008407f0 passes them to 0x008438f0, which sets a
  material constant on matching meshes.
- **In `ddview`:** the colour multiplies the albedo of hair and beard
  materials (*approximation*). This is done for NPCs.
- **Save characters:** `mColorHair` and the other `mColor*` fields are
  palette indices whose table was not found. Save characters are not
  tinted.
- **Not applied:** skin, eye and makeup colours.

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

Until those are traced, every character shows the unmorphed base head
and body.

## The save's Arisen and pawns (`ddlook::save`)

The slot is `mPlayerDataManual`. The Arisen is `mPl` and the party pawns
are `mCmc`, each with `mEdit` (`cSAVE_DATA_EDIT`) and `mParam`.

- **Sex:** `mGender`.
- **Pieces:** every equipped item (`mEquipItem`) maps to the armour id at
  item record +0x40. This is *inferred*: 640 records of the shipped
  `itemList` hold an `m_parts` armour-table id there, and the reader of
  the field was not found.
- **Hair:** `0x0f000000 | mHairNo`.
- **Beard:** `0x10000000 | mBeardNo`, for male characters only.
- **Evidence for hair and beard:** *inferred* from the h_parts ids and the
  NPC edits. The male-only beard is *guessed*: female records carry a
  beard number, and with it the female characters rendered with beards.

The maintainer's save: the Arisen and three pawns all render dressed.
They wear 7-10 models each, and the parts-off records hide 5-17 body
parts each (test `saved_arisen_and_pawns_are_dressed`, which prints no
personal data).

## Viewer

- `ddview <nativePC> --event 22 [--save DDDA.sav]`: event actors assembled
  as above. With `--save`, the player actor is the save's Arisen and pawn
  actors are its pawns.
- `ddview <nativePC> --lineup DDDA.sav [--shot out.png]`: the Arisen and
  pawns side by side in the bind pose.

**Screenshots** (2026-10-09, private save, not committed):
- **lineup:** four characters dressed in tunics, trousers, boots, a cloak
  and a mail coif. Hair is from the save. No skin shows through the
  covered parts. The heads are the unmorphed base heads.
- **ev10 frame 200:** Quina and the elder in their outfits, with
  hair-coloured hair and the elder's beard on the shared skeleton.
- **ev10 frame 1300 with `--save`:** the saved Arisen, dressed, stands
  behind them.

## Open

- Morphs: the `rJointEx` and `uEditModel` application.
- Colour palettes for the save's `mColor*`, and skin and eye colours.
- The item -> piece reader (+0x40).
- The runtime beard rule for female characters.
- Weapons (uWeaponPl).
- NPC mask bits 16/17 (0x00ba7150).
