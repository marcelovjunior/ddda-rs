# Enemy spawns

The world's own enemies, from the stage data and the spawner `sSetManager`
(DTI 0x0198BE44), traced in the 2017-12-13 Steam exe. Runtime: `crates/ddspawn`
(readers, gate, respawn, save inputs) and `ddhost::spawns` (into the fight).
**T** traced, **G** guessed.

## Data

**Group lists (T).** Each stage has four `rLayoutGroupParamList` files,
`scr\st<S>\etc\st<S>_{e,n,p,t}` (enemies, NPCs and hostile humans, objects,
sensor targets). `rLayoutGroupParamList::vf10` (0x00CC6140):
- `"gpl\0"`, version 0x9E;
- a slot table: `u32 n` + n words; slot N holds a group when bit 31 is set
  (295 slots, 292 groups in `st100_e`);
- a second word array, a word, the group count;
- the groups (`cGroupParam`, 0x1E8 bytes, `cGroupParam::vf09` 0x00CC4690) in
  slot order.

A group: two words; the unit kinds (`cUnitData` 0x00CC5920: name such as
`em0100`, belong flag; at most 3 used); the cells it spans (`cID`
0x00CC59E0: stage and group, then `n` and `m` of the cell `<m>m<n>n`);
41 scalar words; three area lists (`AreaHitShape::vf05` 0x00744960 and the
`nZone::ShapeInfo*` loaders: area, sphere, cylinder, cone, AABB, OBB); a
tail. The parser is byte-exact on all 199 lists of the install. The
scalars the spawner reads (offsets in `cGroupParam`):

| Field | Read | Meaning |
|---|---|---|
| +0x20 bit 0, +0x24, +0x28 | scalars 12-14 | SceNo window [begin, end) (`mAppearBgn` / `mAppearEnd`, Riftstone's names) |
| +0x20 bit 1 / bit 3, +0x2C | 15, 16, 17 | layout flag (low u16) / second flag (high u16) must be set |
| +0x34 bit 0, +0x38 | 18, 19 | hour window: from the low u16 up to the high u16, wrapping at 24 |
| +0x34 bit 2 | 23 | never spawns |
| +0x54 bits 0-2 | 30-32 | despawn when the flag / SceNo / hour stops holding (0x0075F860) |
| +0x58 | 33-37 | respawn: type (bits 0-2, `mRspnType`), days (bits 3-10, at most 7 unless type 6), partial-respawn step and growth (bits 11-14) |
| +0x20 bits 4, 5; +0x140; +0x12C bit 17; +0x34 bit 7 | | further conditions (0x004A9040, 0x004A9080, sSetManager +0x14B7C, its +0x14B70 table, 0x004A52B0); not ported |

**Placements (T).** Group N places its units in the layouts
`scr\st<S>\etc\st<S>_<m>m<n>n_e<NN>` of its cells (format 0x01562268
`scr\st%03d\etc\st%03d_%02dm%02dn%s%02d`), read with `mtlot`. A unit's
bit in the kill record is its record id (the first s32 of the record, 0..n
in a group's layout; **G** that it is the value the spawn passes to
0x004A6830, the coordinate block's `mSetID` is -1 in these files).

## The spawn test (T)

`cLayoutSetCharaBase::vf12` (0x0075FAA0), for enemy and NPC groups, in
order: disabled (+0x34 bit 2) -> no; each enabled layout flag must be set
(`0x0075C110`: sGameSys +0xADEE8 + (stage index * 0x53 + n / 32) * 4,
MSB first; stage 100's flags 128..255 at +0xB3330); SceNo in the window;
the hour (sGameSys +0xB876C) in the hour window; then the unported
conditions above. The sensor-target lists use another test (0x004A9A70),
where the hour window excludes instead.

## Kill records and respawn (T)

`sSetManager` keeps a 16-byte record per group slot of the loaded stage
(+0x18A98, 295; saved for all stages as `cSaveWork.mGroupDataEnemy`, 445
u64): bit 0 in use, bits 1-7 stage index, 8-16 group, 17-19 respawn type,
20-22 (type 6: 20-27) days left, 23-24 / 25-26 partial-respawn step and
growth, 28-59 dead units.
- A kill (0x004A7AD0): the first opens the record and copies the group's
  respawn word (0x004A7C10: type 1 days and step fields, type 2 the layout
  flag byte, type 6 eight bits of days); every kill marks its unit
  (0x0049F5D0), except type 5.
- Spawning skips dead units (0x004A6830, 0x004A6510).
- Each day (`sSetManager::vf06` 0x004A40D0 compares the day counter
  sGameSys +0xB8760, then 0x004A8150): types 1, 5 and 6 count their days
  down; at 0 the record clears and the whole group returns. While type 1
  counts, its step (grown by the growth times the days, at most 3) picks a
  random mask, 75 / 50 / 25 % per unit (`rand % 100` above 24 / 49 / 74),
  and dead units outside it return early. Types 0 (spawn once), 2, 3 and 4
  do not age (type 2 holds a layout flag; 3 and 4 not traced).

## Cassardis outskirts (stage 100, cell 64m53n)

Cassardis' Ferrystone arrival (stage 100 start 56) is at x 41522, z 142168:
cell 64m54n. Its neighbour 64m53n (the road and the fields below the
village) holds ten enemy groups:

| Group | Units placed here | SceNo | Hours | Flag | Respawn |
|---|---|---|---|---|---|
| 250 | 3 goblins (em0100) | 0-7799 | 4-20 | | type 1, 1 day |
| 9 | 5 goblins | 0-7799 | 20-4 | | type 1, 1 day |
| 12 | 2 goblins | 0-7799 | 20-4 | | type 1, 1 day |
| 252 | 3 wolves (em0200) | 0-7799 | 20-4 | | type 1, 1 day |
| 13 | 4 em8000 | 0-7799 | 4-20 | | type 1, 1 day |
| 254 | 7 em8000 | 0-7799 | 4-20 | | type 0 |
| 23 | 3 goblins | any | any | 7 | type 0 |
| 24 | goblins, wolves | | 20-5 | 7 | disabled |
| 134 | goblins, wolves, em0101 | 4700-7799 | 4-20 | 150 | type 1, 3 days |
| 132 | em0202, em0102 | 7800- | any | | type 1, 1 day |

So before the dragon, three goblins (group 250) stand there by day and
seven goblins and three wolves (groups 9, 12, 252) by night; killed, they
return the next day. After the dragon (SceNo 7800) only group 132's
post-dragon variants (and 23 when flag 7 is set) remain. The maintainer's
save (SceNo 3400, 12:00) spawns group 250's goblins and the critters of 13
and 254 there. Tests: `crates/ddspawn/tests/install.rs`,
`crates/ddhost/tests/spawns.rs`.

## Runtime

- `ddspawn::gpl::GroupList::parse`, `ddspawn::gate`,
  `ddspawn::spawns(stage, list, cell, read_lot, world, records)`,
  `ddspawn::respawn::{kill, age, chance_masks}`,
  `ddspawn::save::SaveState` (SceNo, hour, day, layout flags and records
  from `DDDA.sav`).
- `ddhost::spawns::{group_list, cells_around, spawn_cells,
  spawn_from_save}`: goblins (`cSetInfoEnemy0100`) join `ddenemyrt`'s fight;
  other kinds come back as placeholders with class, name and position.
- *Placeholders*: the loaded cells are the 3x3 block around the position
  (the streaming radius is not traced); the facing is `mAngle` y; the
  save's values are the first of its two slots in document order.

## Open

- The unported conditions; respawn types 3 and 4; type 5's horde
  counter (`mSetCountMax`).
- When the spawner loads and unloads groups while the player moves (the
  lot managers' cell streaming), and the area lists (their use: life and
  kill areas).
- NPC (`_n`) and object (`_p`) lists use the same groups; not wired.
