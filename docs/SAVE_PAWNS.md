# Pawn save fields

`ddsave::pawns::PawnFields::read(Node)` adds a read-only view of one
`cSAVE_DATA_CMC`, alongside the existing `ddsave::pawn` API. It reads no
names, appearance or network identity. The caller selects the pawn record;
there is no global player. The original XML remains available for fields
outside this view.

## Schema evidence

The save XML identifies every field's type and fixed array length. The
same field names are reflected by `cSAVE_DATA_CMC::vf03` at `0x00787f30`
and `cSAVE_DATA_PARAM::vf03` at `0x00785790` in the 2017-12-13 Steam exe.
The private save copy contains 23 full CMC records: nine in each player
slot (three party, four close friends, two Kaiou records) and five in
`mSystemData` (last-clear pawn and four last-clear close friends).

| Typed field | Serialized storage |
|---|---|
| Knowledge | `mStudyFlag` and `mLocalStudyFlag`, 322 u32 words each |
| Study counters | `mStudyData.EncountFrame` 72 f32, `KillCnt` 72 u32, `UniqueCnt` 116 u8 |
| Equipment | `mParam/mEquipItem`, twelve `sItemManager::cITEM_PARAM_DATA` objects |
| Present item | `mParam/mPresentItem`, same item class |
| Weapon skills | twelve named `mWeaponSkill[nWeapon::...]` arrays, six s16 each |
| Learned skill words | `mSkillLv1`, `mSkillLv2`, fourteen u32 each |
| Augments | `mAbility`, six s8; `mAbilityInfo`, four u32 |
| Inclinations | nine `mInfo[ INFO_TYPE_* ]` groups: u32 type, f32 value, u32 status |
| Extra inclinations | two `mInfoEx[ INFO_TYPE_EX_* ]` groups: u32 type and f32 value |

Items preserve all ten serialized fields: signed quantity/item number,
u32 flags, u16 change counter and three day values, signed mutation-pool
and owner bytes, and u32 key. These names do not imply that their flag bits
or ownership semantics are decoded.

Missing fields return `None`, including individual item/inclination
members. Present arrays must have the expected declared count, actual
child count and element type. Scalars must fit their declared Rust width;
floats must be finite. Duplicate selected fields are rejected. Unknown
fields remain in the XML tree. Negative skill/item sentinels are retained.

Knowledge words preserve the quest, enemy and area knowledge storage;
the mapping from word/bit to a quest, enemy, area or star rating is **not
yet decoded**. Learned-skill/augment bit meanings and displayed inclination
ranking likewise remain unimplemented. This is a storage reader, not a
translation of the knowledge evaluation rules.

## Validation and use

Four synthetic tests cover absent versus zero fields, signed boundaries,
all item members, malformed lengths/types/values, duplicates and nonfinite
floats. The optional private-save test validates every full CMC record,
including system and inactive records, and skips when no copy is present.
Set `DDDA_SAVE` to a private copy; otherwise it finds the newest backup
under `E:/ddda-re/save-backups/*/367500/remote/DDDA.sav`.

`ddtool save-pawns <private save copy>` validates all CMC records and prints
only structural counts. It deliberately does not dump stored values, names,
account identifiers, file paths or raw XML. No save is written.

Implemented and tested: fixed typed storage, four synthetic tests and all
23 records in a private save written by the original game. Not compared:
UI knowledge stars, inclination labels/ranks or behavior after gameplay.
Next action: trace the knowledge evaluators to map stored words to their
quest/enemy/area identifiers, with a separate regression fixture.
