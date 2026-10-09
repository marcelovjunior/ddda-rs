# Vocations, skills, augments and growth

Evidence: DDDA.exe 2017-12-13 Steam (identity in `METHODOLOGY.md`), read-only
Ghidra decompilation and disassembly, and the installed resources read with
`ddvfs`, `ddparam`, `mtgmd` and the new `ddskill` crate. Names are the game's
own English text. Addresses are this build's.

Status legend: **traced** (read in the exe), **data** (shipped resource
values), **guessed** (an inference, not a fact), **live** (compared with the
running game). **Live** so far (claude-4 oracle, board #722, read-only
reads and pure calls on the maintainer's save): the experience curve
(0x00b5c080, 0x00b5c150), the experience-to-discipline conversion
(0x00782f30), the level-7 stats of a Mage Arisen and a Fighter pawn, and the
vocation-rank thresholds. Everything else is static analysis.

## Summary

- Nine vocations, ids 1..9 (`cPlayerInfo.mJob`, +0x10). The main pawn may
  only be Fighter, Strider, Mage, Warrior, Ranger or Sorcerer.
- Learning costs live in one resource, `etc\skill\skill` (`rSkillList`,
  magic `skl\0`): 18 core skills, 131 weapon skills x 2 levels, 78 augments.
  Which vocation and weapon a skill belongs to, its action id and its name
  come from static tables in the exe; `ddskill::catalogue` reads them at run
  time from the player's DDDA.exe and resolves every skill to its action
  class and motion list.
- Level growth: each level adds the `rPlLevel` row of the vocation held *at
  that moment*; changing vocation never recomputes stats. The experience
  curve and the discipline conversion are translated and match the running
  game on every value checked.
- Changing vocation (`uGUIJobChange`): needs level 10 for anything beyond the
  three basic vocations, costs 1000 or 1500 discipline the first time per
  vocation, clears the six active skill slots, keeps level, stats, ranks,
  learned skills and augments.

## Vocations

| Id | Name (`job_N`, common_message) | `rPlLevel` / `rPlJobLevel` file | Weapon categories kept (0x0163e784) | Pawn |
|---:|---|---|---|---|
| 1 | Fighter | LvFighter / JobLvFighter | Sword, Shield | yes |
| 2 | Strider | LvStrider | Dagger, Bow | yes |
| 3 | Mage | LvMagicUser | Staff | yes |
| 4 | Mystic Knight | LvPaladin | Sword, Mace, Staff, Magick Shield | no |
| 5 | Assassin | LvAssassin | Dagger, Sword, Shield, Bow | no |
| 6 | Magick Archer | LvTrickster | Dagger, Staff, Magick Bow | no |
| 7 | Warrior | LvBattler | Longsword, Warhammer | yes |
| 8 | Ranger | LvHunter | Dagger, Longbow | yes |
| 9 | Sorcerer | LvWizard | Archistaff | yes |

**Traced.** Names: labels `job_1`..`job_9` in
`id\DDN\message\common\common_message_eng` (install test). Internal file
names: loader 0x0047e2b0 (`rPlLevel` at sPlayerManager+0x86c + id*4,
`rPlJobLevel` at +0x894 + id*4; index 0 repeats Fighter). Pawn restriction:
the vocation list for a pawn at 0x01517a3c is `1,2,3,7,8,9`, and the pawn's
unlock-flag row 0x01517a54 has -1 for ids 4..6. The random-pawn generator
(0x00796160, claude-5's area) loads only the six `rPlLevel` tables of those
vocations.

The weapon categories are the high byte of a weapon item id
(`mMainWpnId >> 24`, 0x00780590): 0 unarmed, 1 sword, 2 mace, 3 longsword,
4 dagger, 5 staff, 6 archistaff, 7 warhammer, 8 shield, 9 magick shield,
10 bow, 11 longbow, 12 magick bow. Which items a vocation may *equip* is
item data (claude-6); the list above is the skill-set filter (0x00780840).

### Weapon tables per category (traced)

| Cat | Weapon | Skill ids (0x014f6c18 / 4c) | Weapon-skill ids (0x014f6c80 / b4) | Stamina table (0x0163e62c) | Motion banks (0x0163df8c) |
|---:|---|---|---|---|---|
| 0 | Unarmed | 0..9 | none | PlStamina_Unarm | m0004_co, m0004_at |
| 1 | Sword | 10..69 | 40..59 | PlStamina_Sword | m0001_co, m0001_at |
| 2 | Mace | 10..69 | 40..59 | PlStamina_Sword | m0001_co, m0001_at |
| 3 | Longsword | 70..119 | 100..109 | PlStamina_GSword | m0003_co, m0003_at |
| 4 | Dagger | 120..179 | 150..169 | PlStamina_Dagger | m0005_co, m0005_at |
| 5 | Staff | 180..249 | 210..239 | PlStamina_Wand | m0007_co, m0007_at |
| 6 | Archistaff | 180..249 | 210..239 | PlStamina_Wand | m0007_co, m0007_at |
| 7 | Warhammer | 70..119 | 100..109 | PlStamina_GSword | m0003_co, m0003_at |
| 8 | Shield | 250..289 | 270..278 | PlStamina_Shield | m0002_at |
| 9 | Magick Shield | 290..329 | 310..324 | PlStamina_ShieldMG | m0009_at |
| 10 | Bow | 330..379 | 350..358 (+ magick bow) | PlStamina_Bow | m0006_at |
| 11 | Longbow | 380..419 | 400..408 | PlStamina_BowL | m0008_at |
| 12 | Magick Bow | 330..379 | 350..367 | PlStamina_Bow | m0010_at |

Bank keys: 0x600 / 0x700 for primary weapons, 0x900 for secondary ones
(see ANIMATION.md for how banks are loaded). Note: this corrects the guess in
ANIMATION.md that `m0004_co` is the dagger set; the table gives `m0004` to
category 0 (unarmed) and `m0005` to daggers.

## Skill data

### `rSkillList` (data + traced)

One resource, `etc\skill\skill` (two archive copies, identical after
re-encoding). Loader `rSkillList` vtable slot 10 = 0x00cdec00, writer slot
11 = 0x00cdee30. Little-endian:

```
u32 magic 0x006c6b73 ("skl\0")  u32 version 0x200
u32 n; n x (skill_id, cost, rank)            core skills (18)
u32 n; n x (skill_id, level, cost, rank)     weapon skills (262 = 131 x levels 1, 2)
u32 n; n x (augment_id, cost, rank)          augments (78)
```

`cost` is discipline (`cPlayerInfo.mJobPoint`, +0x344; getter 0x0077c340),
`rank` the vocation rank required (`mJobLevel[mJob]`, +0x2cc + id*4). The
meanings come from the consumers 0x006fd940 / 0x006fd7e0 / 0x006fe050, which
compare `rank` with the character's rank and show `cost`. The MtDTI field
names (`mJobLevel`, `mSkillId`, `mSkillLv`, `mJobPoint`) exist, but the DTI
offsets list `mJobLevel` first while the loader stores the rank last; the
order above follows the code. The saved learn state is per character
(below); the "new" markers use bitsets indexed by rSkillList *entry*, at
sGameSys+0xacf84.. (core), +0xacf8c / +0xacfb0 (weapon), +0xacfe0 (augments).

### Skill table in the exe (traced)

A static table of 430 records (ids 0..429, `0x1ae` is the bound used by
every skill bitset), 0x20 bytes each, at 0x014f3658 + id*0x20:

| Offset | Meaning | Evidence |
|---|---|---|
| +0x00 | skill id (unused rows hold copy-paste ids) | layout |
| +0x04 | action id: `0x01wwnnnn` primary weapon, `0x02wwnnnn` secondary, -1 none | 0x00b60e20 selects primary/secondary stamina table by the high byte |
| +0x08 | vocation mask, bit `1 << mJob` (0 = any vocation using the weapon) | 0x006fdb40, 0x00780840 |
| +0x0c | weapon mask, bit `1 << category` | 0x006fdb40, 0x006fd540 |
| +0x10 | 0 action only, 1 core skill learned with discipline, 2 weapon skill | 0x006fdb40 (== 2), 0x006f9e40 (== 1) |
| +0x14 | 0..5, unresolved (spells: 1 fire, 2 ice, 3 thunder pattern, **guessed**) | - |
| +0x18 | flags, unresolved | - |
| +0x1c | index into the text table below, -1 none | 0x005ddcf0 |

Text table, 149 records of 12 bytes at 0x01520278 (index from +0x1c):
`s16 name`, `s16 command[3]`, `s16 unknown`, `u16 flags`. Flag 0x8000 marks
a levelled weapon skill. Name message = `name - 1 + level` in
`id\DDN\message\skill\skillName` for weapon skills, `name` for core skills
(0x005ddcf0). `command[level-1]` indexes `id\message\skill\skill_command`
(the input help, e.g. "Press <SET>."); -1 means the level does not exist,
which is how level 3 is absent for some skills (the name file has " ").

Stamina: for skills whose weapon mask has a bit of 0x19e (sword, mace,
longsword, dagger, warhammer, shield) the cost is row `id - first` of the
weapon's `PlStamina_*` table (0x00b60e20). 0x00b60f70 charges `mStamina`
at once when `mFrame <= 1`, else drains `mStamina / mFrame` per unit of the
player's delta time (`uPlayer`+0x14) for `mFrame` units; every shipped
weapon row has `mFrame` = 1, so all charges are immediate. For the skills listed in 0x00b79500 (40-49, 55-59,
100-109, 150-159, 164-166) the cost is multiplied by
`(100 - v) * 0.01`, `v` = augment 62 (Proficiency) value: 25 when equipped,
**1 when not** (rPlAbilityParam row 62 = 25 / 1, read by 0x00783340). The
literal reading is a 1% discount without the augment; to be checked live.
Staff, magick shield and bow skills have no row (0); their costs are elsewhere
(not traced; Conservation, augment 82, is the magick counterpart).

### Learning rules (traced, 0x006fd540 / 0x006fdb40 / 0x006fdc30)

- Core skill: shown when its weapon mask matches a weapon the vocation can
  use (`0x01516738` per-vocation list of learn-screen tabs mapped to masks by
  0x01516bd8) and cost != 0; learnable when `rank <= mJobLevel[mJob]`.
- Weapon skill: shown when type == 2, cost != 0, the weapon mask matches the
  tab, and the vocation bit is set; learnable at the rank. Level 2 also needs
  level 1 learned (0x00780ba0 >= 1). Skills with cost 0 at rank 0 (40, 104,
  152, 210, 220, 234, 272, 310, 350, 359, 402) are the starting skills.
- Augment under 100: shown only to its vocation (`id / 10 == mJob - 1`).
  Augments 100..114: shown to every vocation when the save byte
  sGameSys+0xbe1b4 + (id - 100) is set (0x0044cac0; copied from the save at
  0x004965a0; what sets it is **not traced**). Augment 106 has cost 0 and
  is never shown.
- Per-character learn state, `cPlayerInfo` +0x72c (0x00780ba0, 0x00442af0):
  +0x730 bitset by skill id = core skill / level 1 learned, +0x768 = level 2
  learned, +0x7a0 = level 3 available. Augments learned: bitset +0x7dc by
  augment id (< 115). Level returned by 0x00780ba0: 3 when the +0x7a0 bit is
  set and level 1 or 2 is learned, else 2, 1 or 0.
- Level 3 ("Grand" names, Dark Arisen) is **granted by equipment**:
  0x00442af0 clears every +0x7a0 bit, then for each of the 12 equipment
  slots 0x00442d50 sets the bit for the two skill ids an item carries at
  item data +8 / +10 when the item has flag bit 29 at +0x58. Which items
  carry which skills is item data (claude-6).

### Active skills and augment slots (traced)

- Six augment slots, `cPlayerInfo.mAbility[6]` (+0x258). An augment's value
  is `rPlAbilityParam[id].mFVal0` when equipped, `mFVal1` otherwise
  (0x00783340; id 10 Leg-Strength returns -1.0 / 0, id 53 also returns
  `mSVal0`).
- Per weapon category a skill set of six ids at +0x120 + cat*0x18
  (`mWeaponSkill[nWeapon]`), and the six active slots `mMainSkill[3]`
  (+0x270) and `mSubSkill[3]` (+0x27c). 0x00780590 empties active slots that
  are not in the equipped weapons' ranges: slots 0..2 take the primary
  weapon's skills, 3..5 the secondary's; a staff or archistaff with no
  secondary uses all six. 0x00780840 empties per-weapon sets the vocation
  cannot use and, except for Mage and Sorcerer, staff slots 3..5 ("Mystic
  Knights and Magick Archers may only enable three skills.",
  `shop_guild_eng` 105).

### Action ids, action classes and motions (traced)

An action id is `group << 24 | weapon << 16 | index`. The player's action
controller (`uPlayer`+0x2db8) keeps one record table per group; the record
is selected by `index` alone (0x007f19c0 stores pointers `table + i * 0x44`,
0x007f2020 overrides single entries from `-1`-terminated lists). The
`weapon` byte only labels the id.

- Group 0, common actions: 0x01866c48, 0x103 records (`uPlayerBase::vf197`
  0x00b60870). Per-weapon override lists (0x0186f488, 0x0186f140,
  0x0186c330, 0x01870118) only replace ids 0x2b..0x2e (holding and
  sheathing the weapon while jumping or falling).
- Group 1, primary weapon (0x00b610f0, jump table 0x00b611bc by weapon
  category): unarmed 0x0186f2a8 (7), sword and mace 0x0186e3f8 (50),
  longsword and warhammer 0x0186c498 (59), dagger 0x0186b1e8 (65), staff
  and archistaff 0x0186f5f0 (42).
- Group 2, secondary weapon (0x00b611e0): shield 0x0186dd10 (26), magick
  shield 0x0186d448 (33), bow, longbow and magick bow 0x0186b118 (3).

A record (0x44 bytes) is the `MtDTI*` of the action class followed by 16
parameter words. The class name comes from the class's `MtDTI` constructor
call (`push name; mov ecx, dti; call 0x00cf5780`). For a weapon skill word 0
is the skill id: the action's vf05 copies it to action+0x54 (e.g.
`cPlActWpnDaggerCstmHyakuretsu::vf05` 0x00b23d50) and passes it to
0x00ac63c0 (level effects) and 0x00ac65a0, which plays the skill's motion
list on layer 7 (main slots) or 9 (sub slots). Exceptions: Gouge (58) runs
`cPlActEnemyClimb` (word 0 = 0xd) and Arc of Might (109)
`cPlActWpnGSwordChargeAtckWait`; all bow skills share `cPlActWpnBow` (word 0
= 0 for the bow, 1 for the longbow) or `cPlActWpnMagicBow` (word 0 = 2). For
core-skill and normal-attack records word 0 is often a value shaped like a
motion id (bank << 8 | slot, e.g. 0x70d), **guessed**.

Motions of weapon skills: `uPlayer`+0x4c2c points to 0x0163dfc0 (13
pointers, one per weapon category; 0x00b5876b). Each list holds
`(bank key, path)` pairs indexed by `skill id - first weapon skill`;
0x00b5b400 loads the `rMotionList` of each of the six active skills into
`uPlayer`+0x4c38 / +0x4c44 when weapons or skills change. Every path exists
in the install (test `motion_lists_exist`). Several skills share a list
(the Mystic Knight sigils 50, 52, 54 share `m0001_at_10`; 161 and 163
share `m0005_at_11`; 220-221 share `m0007_at_10`, the boons 222-226
`m0007_at_12`; magick-shield skills 310-314 and 315-319 share one list
each; all bow, longbow and magick-bow skills use the weapon's single `_at`
list).

`ddskill::catalogue` reads all of this from the player's DDDA.exe; the
table below is its output for the English text (`catalogue_dump`).

| Id | Skill (level 1) | Action id | Action class | Param | Motion list (bank) |
|---:|---|---|---|---|---|
| 30 | Dire Onslaught | 0x0101000a | `cPlActWpnSwordAtckLandS1_2_1` | 0x0 | - |
| 31 | Takedown | 0x0101000d | `cPlActWpnSwordAtckLandL_1` | 0x0 | - |
| 90 | Devastate | 0x01030013 | `cPlActWpnGSwordAtckLandL1_2` | 0x0 | - |
| 91 | Eviscerate | 0x01030014 | `cPlActWpnGSwordAtckLandS1_2_1` | 0x0 | - |
| 140 | Engrave | 0x01050011 | `cPlActWpnDaggerAtckLandS` | 0x70d | - |
| 141 | Roundelay | 0x01050015 | `cPlActWpnDaggerAtckLandL2` | 0x0 | - |
| 142 | Double Vault | 0x01050016 | `cPlActWpnDaggerJumpSecond` | 0x0 | - |
| 40 | Blink Strike | 0x0101001e | `cPlActWpnSwordCstmStinger` | 0x28 | `m0001_at_00` (0x700) |
| 41 | Broad Cut | 0x0101001f | `cPlActWpnSwordCstmIchidoriKiri` | 0x29 | `m0001_at_01` (0x700) |
| 42 | Downthrust | 0x01010020 | `cPlActWpnSwordChokkaIttenTsuki` | 0x2a | `m0001_at_02` (0x700) |
| 43 | Tusk Toss | 0x01010021 | `cPlActWpnSwordCstmKiriAge` | 0x2b | `m0001_at_03` (0x700) |
| 44 | Compass Slash | 0x01010022 | `cPlActWpnSwordCstmEscAtck` | 0x2c | `m0001_at_04` (0x700) |
| 45 | Skyward Lash | 0x01010023 | `cPlActWpnSwordCstmTsuigekiKiri` | 0x2d | `m0001_at_05` (0x700) |
| 46 | Flesh Skewer | 0x01010024 | `cPlActWpnSwordCstmStab` | 0x2e | `m0001_at_06` (0x700) |
| 47 | Hindsight Slash | 0x01010025 | `cPlActWpnSwordCstmKaihiKiri` | 0x2f | `m0001_at_07` (0x700) |
| 48 | Stone Will | 0x01010026 | `cPlActWpnSwordCstmGoutaiJutsu` | 0x30 | `m0001_at_08` (0x700) |
| 49 | Legion's Bite | 0x01010027 | `cPlActWpnSwordCstmGokuRanbuKiri` | 0x31 | `m0001_at_09` (0x700) |
| 50 | Perilous Sigil | 0x01010028 | `cPlActWpnSwordCstmMadouBase` | 0x32 | `m0001_at_10` (0x700) |
| 51 | Magick Cannon | 0x01010029 | `cPlActWpnSwordCstmMadouBase` | 0x33 | `m0001_at_11` (0x700) |
| 52 | Funnel Sigil | 0x0101002a | `cPlActWpnSwordCstmMadouBase` | 0x34 | `m0001_at_10` (0x700) |
| 53 | Sky Dance | 0x0101002b | `cPlActWpnSwordCstmMadouKuujinKiri` | 0x35 | `m0001_at_13` (0x700) |
| 54 | Stone Grove | 0x0101002c | `cPlActWpnSwordCstmMadouBase` | 0x36 | `m0001_at_10` (0x700) |
| 55 | Intimate Strike | 0x0101002d | `cPlActWpnSwordCstmHensokuNidanTsuki` | 0x37 | `m0001_at_15` (0x700) |
| 56 | Windmill Slash | 0x0101002e | `cPlActWpnSwordCstmBrandish` | 0x38 | `m0001_at_16` (0x700) |
| 57 | Powder Charge | 0x0101002f | `cPlActWpnSwordCstmKaenKibakuJutsu` | 0x39 | `m0001_at_17` (0x700) |
| 58 | Gouge | 0x01010030 | `cPlActEnemyClimb` | 0xd | `m0001_at_18` (0x700) |
| 59 | Clarity | 0x01010031 | `cPlActWpnSwordCstmZangekiAnsatsu` | 0x3b | `m0001_at_19` (0x700) |
| 100 | Upward Strike | 0x01030028 | `cPlActWpnGSwordHangetsu` | 0x64 | `m0003_at_00` (0x700) |
| 101 | Pommel Strike | 0x01030029 | `cPlActWpnGSwordCstmTsukaTsuki` | 0x65 | `m0003_at_01` (0x700) |
| 102 | Savage Lunge | 0x0103002a | `cPlActWpnGSwordCstmOshikomiTosshin` | 0x66 | `m0003_at_02` (0x700) |
| 103 | Escape Slash | 0x0103002b | `cPlActWpnGSwordCstmKaihiKiri` | 0x67 | `m0003_at_03` (0x700) |
| 104 | Savage Lash | 0x0103002c | `cPlActWpnGSwordCstmHakaiKiri` | 0x68 | `m0003_at_04` (0x700) |
| 105 | Ladder Blade | 0x0103002d | `cPlActWpnGSwordCstmDaikenToujinJutsu` | 0x69 | `m0003_at_05` (0x700) |
| 106 | Spark Slash | 0x0103002e | `cPlActWpnGSwordCstmHanniSukuiKiri` | 0x6a | `m0003_at_06` (0x700) |
| 107 | Act Of Atonement | 0x0103002f | `cPlActWpnGSwordCstmUramiNoKen` | 0x6b | `m0003_at_07` (0x700) |
| 108 | Battle Cry | 0x01030030 | `cPlActWpnGSwordCstmBanseiChouhatsu` | 0x6c | `m0003_at_08` (0x700) |
| 109 | Arc Of Might | 0x01030031 | `cPlActWpnGSwordChargeAtckWait` | 0x0 | `m0003_at_09` (0x700) |
| 150 | Biting Wind | 0x01050028 | `cPlActWpnDaggerRensaKiriNige` | 0x96 | `m0005_at_00` (0x700) |
| 151 | Toss And Trigger | 0x01050029 | `cPlActWpnDaggerCstmRensaAgoKudaki` | 0x97 | `m0005_at_01` (0x700) |
| 152 | Scarlet Kisses | 0x0105002a | `cPlActWpnDaggerCstmHyakuretsu` | 0x98 | `m0005_at_02` (0x700) |
| 153 | Dazzle Hold | 0x0105002b | `cPlActWpnDaggerFuuatsu` | 0x99 | `m0005_at_03` (0x700) |
| 154 | Sprint | 0x0105002c | `cPlActWpnDaggerCstmShissou` | 0x9a | `m0005_at_04` (0x700) |
| 155 | Helm Splitter | 0x0105002d | `cPlActWpnDaggerCstmKaitenRakkaKiri` | 0x9b | `m0005_at_05` (0x700) |
| 156 | Ensnare | 0x0105002e | `cPlActWpnDaggerCstmHikiyoseKiri` | 0x9c | `m0005_at_06` (0x700) |
| 157 | Pilfer | 0x0105002f | `cPlActWpnDaggerCstmShundatsu` | 0x9d | `m0005_at_07` (0x700) |
| 158 | Reset | 0x01050030 | `cPlActWpnDaggerCstmKyouseiFootwork` | 0x9e | `m0005_at_08` (0x700) |
| 159 | Stepping Stone | 0x01050031 | `cPlActWpnDaggerCstmKeriTobi` | 0x9f | `m0005_at_09` (0x700) |
| 160 | Sunburst | 0x01050032 | `cPlActWpnDaggerCstmMadouKyuushuuKiri` | 0xa0 | `m0005_at_10` (0x700) |
| 161 | Shadowpin | 0x01050033 | `cPlActWpnDaggerCstmMadouBase` | 0xa1 | `m0005_at_11` (0x700) |
| 162 | Scension | 0x01050034 | `cPlActWpnDaggerCstmMadouBase` | 0xa2 | `m0005_at_12` (0x700) |
| 163 | Magick Rebuffer | 0x01050035 | `cPlActWpnDaggerCstmMadouBase` | 0xa3 | `m0005_at_11` (0x700) |
| 164 | Wind Harness | 0x01050036 | `cPlActWpnDaggerCstmMadouKakusei` | 0xa4 | `m0005_at_14` (0x700) |
| 165 | Back Kick | 0x01050037 | `cPlActWpnDaggerCstmShuntouKiri` | 0xa5 | `m0005_at_15` (0x700) |
| 166 | Spiderbite | 0x01050038 | `cPlActWpnDaggerCstmDokubariTsuki` | 0xa6 | `m0005_at_16` (0x700) |
| 167 | Backfire | 0x01050039 | `cPlActWpnDaggerCstmMadouNensyou` | 0xa7 | `m0005_at_17` (0x700) |
| 168 | Stealth | 0x0105003a | `cPlActWpnDaggerCstmToumei` | 0xa8 | `m0005_at_18` (0x700) |
| 169 | Easy Kill | 0x0105003b | `cPlActAssassinStart` | 0xa9 | `m0005_at_19` (0x700) |
| 210 | Ingle | 0x01070000 | `cPlActWpnWandBase` | 0xd2 | `m0007_at_00` (0x700) |
| 211 | Frazil | 0x01070001 | `cPlActWpnWandBase` | 0xd3 | `m0007_at_01` (0x700) |
| 212 | Levin | 0x01070002 | `cPlActWpnWandBase` | 0xd4 | `m0007_at_02` (0x700) |
| 213 | Comestion | 0x01070003 | `cPlActWpnWandBase` | 0xd5 | `m0007_at_03` (0x700) |
| 214 | Frigor | 0x01070004 | `cPlActWpnWandBase` | 0xd6 | `m0007_at_04` (0x700) |
| 215 | Brontide | 0x01070005 | `cPlActWpnWandWhip` | 0xd7 | `m0007_at_05` (0x700) |
| 216 | Grapnel | 0x01070006 | `cPlActWpnWandBase` | 0xd8 | `m0007_at_06` (0x700) |
| 217 | Silentium | 0x01070007 | `cPlActWpnWandBase` | 0xd9 | `m0007_at_07` (0x700) |
| 218 | Blearing | 0x01070008 | `cPlActWpnWandBase` | 0xda | `m0007_at_08` (0x700) |
| 219 | Lassitude | 0x01070009 | `cPlActWpnWandBase` | 0xdb | `m0007_at_09` (0x700) |
| 220 | Anodyne | 0x0107000a | `cPlActWpnWandBase` | 0xdc | `m0007_at_10` (0x700) |
| 221 | Halidom | 0x0107000b | `cPlActWpnWandBase` | 0xdd | `m0007_at_10` (0x700) |
| 222 | Fire Boon | 0x0107000c | `cPlActWpnWandBase` | 0xde | `m0007_at_12` (0x700) |
| 223 | Ice Boon | 0x0107000d | `cPlActWpnWandBase` | 0xdf | `m0007_at_12` (0x700) |
| 224 | Thunder Boon | 0x0107000e | `cPlActWpnWandBase` | 0xe0 | `m0007_at_12` (0x700) |
| 225 | Holy Boon | 0x0107000f | `cPlActWpnWandBase` | 0xe1 | `m0007_at_12` (0x700) |
| 226 | Dark Boon | 0x01070010 | `cPlActWpnWandBase` | 0xe2 | `m0007_at_12` (0x700) |
| 227 | Bolide | 0x01070011 | `cPlActWpnWandBase` | 0xe3 | `m0007_at_17` (0x700) |
| 228 | Gicel | 0x01070012 | `cPlActWpnWandBase` | 0xe4 | `m0007_at_18` (0x700) |
| 229 | Fulmination | 0x01070013 | `cPlActWpnWandThunderSpark` | 0xe5 | `m0007_at_19` (0x700) |
| 230 | Seism | 0x01070014 | `cPlActWpnWandBase` | 0xe6 | `m0007_at_20` (0x700) |
| 231 | Maelstrom | 0x01070015 | `cPlActWpnWandBase` | 0xe7 | `m0007_at_21` (0x700) |
| 232 | Exequy | 0x01070016 | `cPlActWpnWandBase` | 0xe8 | `m0007_at_22` (0x700) |
| 233 | Petrifaction | 0x01070017 | `cPlActWpnWandBase` | 0xe9 | `m0007_at_23` (0x700) |
| 234 | Miasma | 0x01070018 | `cPlActWpnWandBase` | 0xea | `m0007_at_24` (0x700) |
| 235 | Perdition | 0x01070019 | `cPlActWpnWandBase` | 0xeb | `m0007_at_25` (0x700) |
| 236 | Sopor | 0x0107001a | `cPlActWpnWandBase` | 0xec | `m0007_at_26` (0x700) |
| 237 | Voidspell | 0x0107001b | `cPlActWpnWandBase` | 0xed | `m0007_at_27` (0x700) |
| 238 | Spellscreen | 0x0107001c | `cPlActWpnWandBase` | 0xee | `m0007_at_28` (0x700) |
| 239 | Necromancy | 0x0107001d | `cPlActWpnWandBase` | 0xef | `m0007_at_29` (0x700) |
| 270 | Shield Strike | 0x02020010 | `cPlActWpnShieldCstmAtck` | 0x10e | `m0002_at_00` (0x900) |
| 271 | Springboard | 0x02020011 | `cPlActWpnShieldCstmCatapult` | 0x10f | `m0002_at_01` (0x900) |
| 272 | Shield Summons | 0x02020012 | `cPlActWpnShieldCstmProvoke` | 0x110 | `m0002_at_02` (0x900) |
| 273 | Cymbal Attack | 0x02020013 | `cPlActWpnShieldCstmRengeki` | 0x111 | `m0002_at_03` (0x900) |
| 274 | Sheltered Spike | 0x02020014 | `cPlActWpnShieldCstmBougyoKengeki` | 0x112 | `m0002_at_04` (0x900) |
| 275 | Perfect Defense | 0x02020015 | `cPlActWpnShieldCstmKyuukyokuBougyo` | 0x113 | `m0002_at_05` (0x900) |
| 276 | Moving Castle | 0x02020016 | `cPlActWpnShieldCstmBougyoFumikomi` | 0x114 | `m0002_at_06` (0x900) |
| 277 | Flight Response | 0x02020017 | `cPlActWpnShieldCstmBougyoShouhi` | 0x115 | `m0002_at_07` (0x900) |
| 278 | Staredown | 0x02020019 | `cPlActWpnShieldCstmKarayaburi` | 0x116 | `m0002_at_08` (0x900) |
| 310 | Firecounter | 0x02080010 | `cPlActWpnMagicShieldBase` | 0x136 | `m0009_at_00` (0x900) |
| 311 | Icecounter | 0x02080011 | `cPlActWpnMagicShieldBase` | 0x137 | `m0009_at_00` (0x900) |
| 312 | Thundercounter | 0x02080012 | `cPlActWpnMagicShieldBase` | 0x138 | `m0009_at_00` (0x900) |
| 313 | Holycounter | 0x02080013 | `cPlActWpnMagicShieldBase` | 0x139 | `m0009_at_00` (0x900) |
| 314 | Darkcounter | 0x02080014 | `cPlActWpnMagicShieldBase` | 0x13a | `m0009_at_00` (0x900) |
| 315 | Fire Enchanter | 0x02080015 | `cPlActWpnMagicShieldBase` | 0x13b | `m0009_at_05` (0x900) |
| 316 | Ice Enchanter | 0x02080016 | `cPlActWpnMagicShieldBase` | 0x13c | `m0009_at_05` (0x900) |
| 317 | Thunder Enchanter | 0x02080017 | `cPlActWpnMagicShieldBase` | 0x13d | `m0009_at_05` (0x900) |
| 318 | Holy Enchanter | 0x02080018 | `cPlActWpnMagicShieldBase` | 0x13e | `m0009_at_05` (0x900) |
| 319 | Dark Enchanter | 0x02080019 | `cPlActWpnMagicShieldBase` | 0x13f | `m0009_at_05` (0x900) |
| 320 | Holy Glare | 0x0208001a | `cPlActWpnMagicShieldBase` | 0x140 | `m0009_at_10` (0x900) |
| 321 | Dark Anguish | 0x0208001b | `cPlActWpnMagicShieldBase` | 0x141 | `m0009_at_11` (0x900) |
| 322 | Holy Wall | 0x0208001c | `cPlActWpnMagicShieldBase` | 0x142 | `m0009_at_12` (0x900) |
| 323 | Demonspite | 0x0208001d | `cPlActWpnMagicShieldBase` | 0x143 | `m0009_at_13` (0x900) |
| 324 | Holy Aid | 0x0208001e | `cPlActWpnMagicShieldBase` | 0x144 | `m0009_at_14` (0x900) |
| 350 | Threefold Arrow | 0x02060000 | `cPlActWpnBow` | 0x0 | `m0006_at` (0x900) |
| 351 | Triad Shot | 0x02060000 | `cPlActWpnBow` | 0x0 | `m0006_at` (0x900) |
| 352 | Full Bend | 0x02060000 | `cPlActWpnBow` | 0x0 | `m0006_at` (0x900) |
| 353 | Cloudburst Volley | 0x02060000 | `cPlActWpnBow` | 0x0 | `m0006_at` (0x900) |
| 354 | Splinter Dart | 0x02060000 | `cPlActWpnBow` | 0x0 | `m0006_at` (0x900) |
| 355 | Whistle Dart | 0x02060000 | `cPlActWpnBow` | 0x0 | `m0006_at` (0x900) |
| 356 | Keen Sight | 0x02060000 | `cPlActWpnBow` | 0x0 | `m0006_at` (0x900) |
| 357 | Puncture Dart | 0x02060000 | `cPlActWpnBow` | 0x0 | `m0006_at` (0x900) |
| 358 | Blunting Arrow | 0x02060000 | `cPlActWpnBow` | 0x0 | `m0006_at` (0x900) |
| 359 | Threefold Bolt | 0x02060001 | `cPlActWpnMagicBow` | 0x2 | `m0010_at` (0x900) |
| 360 | Seeker Bolt | 0x02060001 | `cPlActWpnMagicBow` | 0x2 | `m0010_at` (0x900) |
| 361 | Explosive Bolt | 0x02060001 | `cPlActWpnMagicBow` | 0x2 | `m0010_at` (0x900) |
| 362 | Ricochet Seeker | 0x02060001 | `cPlActWpnMagicBow` | 0x2 | `m0010_at` (0x900) |
| 363 | Magickal Flare | 0x02060001 | `cPlActWpnMagicBow` | 0x2 | `m0010_at` (0x900) |
| 364 | Funnel Trail | 0x02060001 | `cPlActWpnMagicBow` | 0x2 | `m0010_at` (0x900) |
| 365 | Ward Arrow | 0x02060001 | `cPlActWpnMagicBow` | 0x2 | `m0010_at` (0x900) |
| 366 | Bracer Arrow | 0x02060001 | `cPlActWpnMagicBow` | 0x2 | `m0010_at` (0x900) |
| 367 | Sacrificial Bolt | 0x02060001 | `cPlActWpnMagicBow` | 0x2 | `m0010_at` (0x900) |
| 400 | Sixfold Arrow | 0x02060002 | `cPlActWpnBow` | 0x1 | `m0008_at` (0x900) |
| 401 | Heptad Shot | 0x02060002 | `cPlActWpnBow` | 0x1 | `m0008_at` (0x900) |
| 402 | Dire Arrow | 0x02060002 | `cPlActWpnBow` | 0x1 | `m0008_at` (0x900) |
| 403 | Foot Binder | 0x02060002 | `cPlActWpnBow` | 0x1 | `m0008_at` (0x900) |
| 404 | Invasive Arrow | 0x02060002 | `cPlActWpnBow` | 0x1 | `m0008_at` (0x900) |
| 405 | Flying Din | 0x02060002 | `cPlActWpnBow` | 0x1 | `m0008_at` (0x900) |
| 406 | Meteor Shot | 0x02060002 | `cPlActWpnBow` | 0x1 | `m0008_at` (0x900) |
| 407 | Whirling Arrow | 0x02060002 | `cPlActWpnBow` | 0x1 | `m0008_at` (0x900) |
| 408 | Gamble Draw | 0x02060002 | `cPlActWpnBow` | 0x1 | `m0008_at` (0x900) |

## Skill and augment tables (data + traced)

Joined from `rSkillList` (cost, rank), the exe skill table (vocations, weapon, action), the text table and `skillName` / `abilityName` / `abilityInfo` (English). Vocations: F Fighter, St Strider, Ma Mage, MK Mystic Knight, As Assassin, MA Magick Archer, W Warrior, R Ranger, So Sorcerer. Cost is discipline, rank the vocation rank required; cost 0 / rank 0 means known from the start.

### Core skills learned with discipline

| Id | Name | Vocations | Weapon | Cost | Rank | Action |
|---:|---|---|---|---:|---:|---|
| 30 | Dire Onslaught | any user of the weapon | Sword / Mace | 800 | 1 | 0x0101000a |
| 31 | Takedown | any user of the weapon | Sword / Mace | 600 | 1 | 0x0101000d |
| 32 | Controlled Fall | any user of the weapon | Sword / Mace | 1200 | 3 | 0xffffffff |
| 90 | Devastate | any user of the weapon | Longsword / Warhammer | 800 | 1 | 0x01030013 |
| 91 | Eviscerate | any user of the weapon | Longsword / Warhammer | 600 | 1 | 0x01030014 |
| 92 | Controlled Fall | any user of the weapon | Longsword / Warhammer | 900 | 2 | 0xffffffff |
| 140 | Engrave | any user of the weapon | Dagger | 800 | 1 | 0x01050011 |
| 141 | Roundelay | any user of the weapon | Dagger | 600 | 1 | 0x01050015 |
| 142 | Double Vault | any user of the weapon | Dagger | 1800 | 5 | 0x01050016 |
| 143 | Forward Roll | any user of the weapon | Dagger | 2200 | 5 | 0xffffffff |
| 200 | Focused Bolt | any user of the weapon | Staff / Archistaff | 3000 | 6 | 0xffffffff |
| 201 | Magick Agent | any user of the weapon | Staff / Archistaff | 3000 | 6 | 0xffffffff |
| 202 | Levitate | any user of the weapon | Staff / Archistaff | 600 | 2 | 0xffffffff |
| 260 | Deflect | any user of the weapon | Shield | 1400 | 3 | 0xffffffff |
| 300 | Reflect | any user of the weapon | Magick Shield | 1600 | 3 | 0xffffffff |
| 340 | Quick-Loose | any user of the weapon | Bow | 600 | 2 | 0xffffffff |
| 341 | True Seeker | any user of the weapon | Magick Bow | 4000 | 6 | 0xffffffff |
| 390 | Quick-Loose | any user of the weapon | Longbow | 600 | 2 | 0xffffffff |

### Weapon skills

Names are levels 1 / 2 / 3 (`-` = no level 3). Stamina is `PlStamina_<weapon>` row `id - first` (mStamina, and the frame count when the cost is spread).

| Id | Names | Vocations | Weapon | Lv1 cost / rank | Lv2 cost / rank | Stamina | Action |
|---:|---|---|---|---|---|---|---|
| 40 | Blink Strike / Burst Strike / Blitz Strike | F, MK, As | Sword / Mace | 0 / 0 | 600 / 3 | 100 | 0x0101001e |
| 41 | Broad Cut / Broad Slash / - | F, MK, As | Sword / Mace | 200 / 1 | 1000 / 4 | 80 | 0x0101001f |
| 42 | Downthrust / Downcrack / Downcrush | F, MK, As | Sword / Mace | 1400 / 5 | 2000 / 7 | 300 | 0x01010020 |
| 43 | Tusk Toss / Antler Toss / - | F, MK, As | Sword / Mace | 400 / 2 | 1600 / 6 | 120 | 0x01010021 |
| 44 | Compass Slash / Full Moon Slash / - | F, MK, As | Sword / Mace | 1400 / 5 | 2000 / 7 | 140 | 0x01010022 |
| 45 | Skyward Lash / Heavenward Lash / - | F | Sword / Mace | 250 / 1 | 1300 / 4 | 180 | 0x01010023 |
| 46 | Flesh Skewer / Soul Skewer / Fate Skewer | F | Sword / Mace | 800 / 3 | 1800 / 6 | 200 | 0x01010024 |
| 47 | Hindsight Slash / Hindsight Sweep / Hindsight Strike | F | Sword / Mace | 1300 / 4 | 1800 / 6 | 160 | 0x01010025 |
| 48 | Stone Will / Steel Will / - | F | Sword / Mace | 2000 / 6 | 4000 / 8 | 250 | 0x01010026 |
| 49 | Legion's Bite / Dragon's Maw / - | F | Sword / Mace | 5000 / 9 | 7500 / 9 | 400 | 0x01010027 |
| 50 | Perilous Sigil / Ruinous Sigil / - | MK | Sword / Mace | 1400 / 4 | 2800 / 6 | 140 | 0x01010028 |
| 51 | Magick Cannon / Great Cannon / - | MK | Sword / Mace | 1400 / 4 | 2800 / 6 | 120 | 0x01010029 |
| 52 | Funnel Sigil / Vortex Sigil / Maelstrom Sigil | MK | Sword / Mace | 1400 / 4 | 2800 / 6 | 180 | 0x0101002a |
| 53 | Sky Dance / Sky Rapture / - | MK | Sword / Mace | 400 / 1 | 1000 / 3 | 100 | 0x0101002b |
| 54 | Stone Grove / Stone Forest / Stone Jungle | MK | Sword / Mace | 4500 / 8 | 7000 / 9 | 250 | 0x0101002c |
| 55 | Intimate Strike / Intimate Gambit / - | As | Sword / Mace | 400 / 1 | 1200 / 3 | 120 | 0x0101002d |
| 56 | Windmill Slash / Great Windmill / - | As | Sword / Mace | 3000 / 6 | 5000 / 8 | 180 | 0x0101002e |
| 57 | Powder Charge / Powder Blast / Powder Barrage | As | Sword / Mace | 1200 / 3 | 2200 / 5 | 140 | 0x0101002f |
| 58 | Gouge / Dire Gouge / Deadly Gouge | As | Sword / Mace | 4000 / 7 | 6000 / 9 | 180 | 0x01010030 |
| 59 | Clarity / Clairvoyance / - | As | Sword / Mace | 2000 / 5 | 6000 / 9 | 160 | 0x01010031 |
| 100 | Upward Strike / Whirlwind Slash / - | W | Longsword / Warhammer | 300 / 1 | 1500 / 4 | 150 | 0x01030028 |
| 101 | Pommel Strike / Pommel Bash / - | W | Longsword / Warhammer | 300 / 1 | 1500 / 4 | 50 | 0x01030029 |
| 102 | Savage Lunge / Indomitable Lunge / Calamitous Lunge | W | Longsword / Warhammer | 900 / 3 | 2800 / 6 | 300 | 0x0103002a |
| 103 | Escape Slash / Exodus Slash / - | W | Longsword / Warhammer | 650 / 2 | 2000 / 5 | 100 | 0x0103002b |
| 104 | Savage Lash / Indomitable Lash / Calamitous Lash | W | Longsword / Warhammer | 0 / 0 | 900 / 3 | 100 | 0x0103002c |
| 105 | Ladder Blade / Catapult Blade / - | W | Longsword / Warhammer | 650 / 2 | 2000 / 5 | 0 | 0x0103002d |
| 106 | Spark Slash / Corona Slash / Ecliptic Slash | W | Longsword / Warhammer | 1500 / 4 | 2800 / 6 | 150 | 0x0103002e |
| 107 | Act Of Atonement / Act Of Vengeance / - | W | Longsword / Warhammer | 2000 / 5 | 3400 / 7 | 50 | 0x0103002f |
| 108 | Battle Cry / War Cry / - | W | Longsword / Warhammer | 2000 / 5 | 3400 / 7 | 0 | 0x01030030 |
| 109 | Arc Of Might / Arc Of Deliverance / Arc Of Obliteration | W | Longsword / Warhammer | 2800 / 6 | 6000 / 8 | 400 | 0x01030031 |
| 150 | Biting Wind / Cutting Wind / Shearing Wind | St, As, MA, R | Dagger | 200 / 1 | 900 / 4 | 180 | 0x01050028 |
| 151 | Toss And Trigger / Advanced Trigger / - | St, As, MA, R | Dagger | 700 / 3 | 1200 / 5 | 140 | 0x01050029 |
| 152 | Scarlet Kisses / Hundred Kisses / Thousand Kisses | St, As, MA, R | Dagger | 0 / 0 | 2000 / 7 | 120 | 0x0105002a |
| 153 | Dazzle Hold / Dazzle Blast / - | St, As, MA, R | Dagger | 900 / 4 | 2000 / 7 | 140 | 0x0105002b |
| 154 | Sprint / Mad Dash / - | St | Dagger | 1100 / 4 | 2000 / 6 | 100 | 0x0105002c |
| 155 | Helm Splitter / Skull Splitter / Brain Splitter | St | Dagger | 900 / 3 | 2000 / 6 | 140 | 0x0105002d |
| 156 | Ensnare / Implicate / - | St | Dagger | 900 / 3 | 2000 / 6 | 140 | 0x0105002e |
| 157 | Pilfer / Master Thief / - | St | Dagger | 2000 / 6 | 3500 / 8 | 80 | 0x0105002f |
| 158 | Reset / Instant Reset / - | St, As, MA, R | Dagger | 900 / 4 | 2000 / 7 | 100 | 0x01050030 |
| 159 | Stepping Stone / Leaping Stone / Soaring Stone | St | Dagger | 3500 / 8 | 5000 / 9 | 120 | 0x01050031 |
| 160 | Sunburst / Sunflare / - | MA | Dagger | 300 / 1 | 1100 / 3 | 100 | 0x01050032 |
| 161 | Shadowpin / Shadowshackle / Shadowsnare | MA | Dagger | 1500 / 4 | 2500 / 6 | 180 | 0x01050033 |
| 162 | Scension / Grand Scension / - | MA | Dagger | 1100 / 3 | 2000 / 5 | 120 | 0x01050034 |
| 163 | Magick Rebuffer / Magick Rebalancer / - | MA | Dagger | 2000 / 5 | 4000 / 8 | 140 | 0x01050035 |
| 164 | Wind Harness / Gale Harness / Tempest Harness | As | Dagger | 4000 / 7 | 6000 / 9 | 100 | 0x01050036 |
| 165 | Back Kick / Escape Onslaught / Shirking Offensive | As | Dagger | 800 / 2 | 1600 / 4 | 140 | 0x01050037 |
| 166 | Spiderbite / Snakebite / - | As | Dagger | 3000 / 6 | 5000 / 8 | 180 | 0x01050038 |
| 167 | Backfire / Immolation / Flameshroud | MA | Dagger | 3200 / 7 | 5000 / 9 | 100 | 0x01050039 |
| 168 | Stealth / Invisibility / - | As | Dagger | 5000 / 8 | 6000 / 9 | 400 | 0x0105003a |
| 169 | Easy Kill / Masterful Kill / - | As | Dagger | 2200 / 5 | 6000 / 9 | 160 | 0x0105003b |
| 210 | Ingle / High Ingle / Grand Ingle | Ma, MK, MA, So | Staff / Archistaff | 0 / 0 | 700 / 3 | n/a | 0x01070000 |
| 211 | Frazil / High Frazil / - | Ma, MK, MA, So | Staff / Archistaff | 200 / 1 | 700 / 3 | n/a | 0x01070001 |
| 212 | Levin / High Levin / Grand Levin | Ma, MK, MA, So | Staff / Archistaff | 200 / 1 | 700 / 3 | n/a | 0x01070002 |
| 213 | Comestion / High Comestion / - | Ma, MK, MA, So | Staff / Archistaff | 1400 / 5 | 1800 / 7 | n/a | 0x01070003 |
| 214 | Frigor / High Frigor / Grand Frigor | Ma, MK, MA, So | Staff / Archistaff | 1400 / 5 | 1800 / 7 | n/a | 0x01070004 |
| 215 | Brontide / High Brontide / Grand Brontide | Ma, MK, MA, So | Staff / Archistaff | 1400 / 5 | 1800 / 7 | n/a | 0x01070005 |
| 216 | Grapnel / High Grapnel / - | Ma | Staff | 1800 / 5 | 5000 / 8 | n/a | 0x01070006 |
| 217 | Silentium / High Silentium / - | Ma, So | Staff / Archistaff | 1000 / 4 | 1400 / 6 | n/a | 0x01070007 |
| 218 | Blearing / High Blearing / - | Ma, So | Staff / Archistaff | 1000 / 4 | 1400 / 6 | n/a | 0x01070008 |
| 219 | Lassitude / High Lassitude / - | So | Archistaff | 1300 / 3 | 2600 / 5 | n/a | 0x01070009 |
| 220 | Anodyne / High Anodyne / Grand Anodyne | Ma | Staff | 0 / 0 | 2800 / 7 | n/a | 0x0107000a |
| 221 | Halidom / High Halidom / - | Ma | Staff | 800 / 3 | 2800 / 7 | n/a | 0x0107000b |
| 222 | Fire Boon / Fire Affinity / Fire Pact | Ma, So | Staff / Archistaff | 200 / 1 | 1200 / 6 | n/a | 0x0107000c |
| 223 | Ice Boon / Ice Affinity / Ice Pact | Ma, So | Staff / Archistaff | 200 / 1 | 1200 / 6 | n/a | 0x0107000d |
| 224 | Thunder Boon / Thunder Affinity / Thunder Pact | Ma, So | Staff / Archistaff | 200 / 1 | 1200 / 6 | n/a | 0x0107000e |
| 225 | Holy Boon / Holy Affinity / Holy Pact | Ma, So | Staff / Archistaff | 500 / 3 | 1200 / 6 | n/a | 0x0107000f |
| 226 | Dark Boon / Dark Affinity / Dark Pact | Ma, So | Staff / Archistaff | 500 / 3 | 1200 / 6 | n/a | 0x01070010 |
| 227 | Bolide / High Bolide / Grand Bolide | So | Archistaff | 3000 / 6 | 5000 / 8 | n/a | 0x01070011 |
| 228 | Gicel / High Gicel / Grand Gicel | So | Archistaff | 3000 / 6 | 5000 / 8 | n/a | 0x01070012 |
| 229 | Fulmination / High Fulmination / Grand Fulmination | So | Archistaff | 3000 / 6 | 5000 / 8 | n/a | 0x01070013 |
| 230 | Seism / High Seism / Grand Seism | So | Archistaff | 4800 / 7 | 8500 / 9 | n/a | 0x01070014 |
| 231 | Maelstrom / High Maelstrom / - | So | Archistaff | 4800 / 7 | 8500 / 9 | n/a | 0x01070015 |
| 232 | Exequy / High Exequy / - | So | Archistaff | 2600 / 5 | 5000 / 8 | n/a | 0x01070016 |
| 233 | Petrifaction / High Petrifaction / - | So | Archistaff | 2600 / 5 | 5000 / 8 | n/a | 0x01070017 |
| 234 | Miasma / High Miasma / - | So | Archistaff | 0 / 0 | 1300 / 3 | n/a | 0x01070018 |
| 235 | Perdition / High Perdition / - | MA | Staff | 3500 / 6 | 7500 / 9 | n/a | 0x01070019 |
| 236 | Sopor / High Sopor / Grand Sopor | MK | Staff | 3500 / 6 | 7500 / 9 | n/a | 0x0107001a |
| 237 | Voidspell / High Voidspell / - | So | Archistaff | 1800 / 4 | 3000 / 6 | n/a | 0x0107001b |
| 238 | Spellscreen / High Spellscreen / - | Ma | Staff | 5000 / 8 | 7000 / 9 | n/a | 0x0107001c |
| 239 | Necromancy / High Necromancy / - | So | Archistaff | 1800 / 4 | 3000 / 6 | n/a | 0x0107001d |
| 270 | Shield Strike / Shield Storm / Shield Slam | F, As | Shield | 200 / 1 | 1000 / 4 | 100 | 0x02020010 |
| 271 | Springboard / Launchboard / - | F, As | Shield | 400 / 2 | 1600 / 6 | 0 | 0x02020011 |
| 272 | Shield Summons / Shield Drum / - | F, As | Shield | 0 / 0 | 600 / 3 | 0 | 0x02020012 |
| 273 | Cymbal Attack / Cymbal Onslaught / - | F | Shield | 250 / 1 | 1300 / 4 | 50 | 0x02020013 |
| 274 | Sheltered Spike / Sheltered Assault / Sheltered Fusillade | F | Shield | 800 / 3 | 1800 / 6 | 100 | 0x02020014 |
| 275 | Perfect Defense / Divine Defense / - | F | Shield | 2000 / 6 | 4000 / 8 | 0 | 0x02020015 |
| 276 | Moving Castle / Swift Castle / - | As | Shield | 800 / 2 | 1600 / 4 | 50 | 0x02020016 |
| 277 | Flight Response / Enhanced Response / - | As | Shield | 1600 / 4 | 3000 / 6 | 100 | 0x02020017 |
| 278 | Staredown / Showdown / Crackdown | As | Shield | 5000 / 8 | 6000 / 9 | 150 | 0x02020019 |
| 310 | Firecounter / Flame Riposte / Inferno Feint | MK | Magick Shield | 0 / 0 | 1200 / 4 | n/a | 0x02080010 |
| 311 | Icecounter / Frost Riposte / Blizzard Feint | MK | Magick Shield | 300 / 1 | 1200 / 4 | n/a | 0x02080011 |
| 312 | Thundercounter / Thunder Riposte / Boltstorm Feint | MK | Magick Shield | 300 / 1 | 1200 / 4 | n/a | 0x02080012 |
| 313 | Holycounter / Blessed Riposte / Hallowed Feint | MK | Magick Shield | 300 / 1 | 1200 / 4 | n/a | 0x02080013 |
| 314 | Darkcounter / Abyssal Riposte / Desecration Feint | MK | Magick Shield | 300 / 1 | 1200 / 4 | n/a | 0x02080014 |
| 315 | Fire Enchanter / Flame Trance / Inferno Invocation | MK | Magick Shield | 1600 / 5 | 4000 / 8 | n/a | 0x02080015 |
| 316 | Ice Enchanter / Frost Trance / Blizzard Invocation | MK | Magick Shield | 1600 / 5 | 4000 / 8 | n/a | 0x02080016 |
| 317 | Thunder Enchanter / Lightning Trance / Boltstorm Invocation | MK | Magick Shield | 1600 / 5 | 4000 / 8 | n/a | 0x02080017 |
| 318 | Holy Enchanter / Blessed Trance / Hallowed Invocation | MK | Magick Shield | 1600 / 5 | 4000 / 8 | n/a | 0x02080018 |
| 319 | Dark Enchanter / Abyssal Trance / Desecration Invocation | MK | Magick Shield | 1600 / 5 | 4000 / 8 | n/a | 0x02080019 |
| 320 | Holy Glare / Holy Furor / Holy Retribution | MK | Magick Shield | 5000 / 9 | 7500 / 9 | n/a | 0x0208001a |
| 321 | Dark Anguish / Abyssal Anguish / - | MK | Magick Shield | 3800 / 7 | 6000 / 9 | n/a | 0x0208001b |
| 322 | Holy Wall / Holy Fortress / - | MK | Magick Shield | 900 / 2 | 2000 / 5 | n/a | 0x0208001c |
| 323 | Demonspite / Demonswrath / - | MK | Magick Shield | 900 / 2 | 2000 / 5 | n/a | 0x0208001d |
| 324 | Holy Aid / Holy Grace / - | MK | Magick Shield | 3800 / 7 | 6000 / 9 | n/a | 0x0208001e |
| 350 | Threefold Arrow / Fivefold Flurry / - | St, As | Bow | 0 / 0 | 2000 / 7 | n/a | 0x02060000 |
| 351 | Triad Shot / Pentad Shot / - | St, As | Bow | 200 / 1 | 900 / 4 | n/a | 0x02060000 |
| 352 | Full Bend / Mighty Bend / Terrible Bend | St, As | Bow | 700 / 3 | 1200 / 5 | n/a | 0x02060000 |
| 353 | Cloudburst Volley / Downpour Volley / Hailstorm Volley | St | Bow | 250 / 1 | 1100 / 4 | n/a | 0x02060000 |
| 354 | Splinter Dart / Fracture Dart / - | St | Bow | 2000 / 6 | 3500 / 8 | n/a | 0x02060000 |
| 355 | Whistle Dart / Shriek Dart / - | St | Bow | 3500 / 8 | 5000 / 9 | n/a | 0x02060000 |
| 356 | Keen Sight / Lyncean Sight / Eagle Sight | As | Bow | 1200 / 3 | 2200 / 5 | n/a | 0x02060000 |
| 357 | Puncture Dart / Skewer Dart / - | As | Bow | 400 / 1 | 1200 / 3 | n/a | 0x02060000 |
| 358 | Blunting Arrow / Plegic Arrow / - | As | Bow | 1600 / 4 | 3000 / 6 | n/a | 0x02060000 |
| 359 | Threefold Bolt / Sixfold Bolt / Ninefold Bolt | MA | Magick Bow | 0 / 0 | 1500 / 4 | n/a | 0x02060001 |
| 360 | Seeker Bolt / Hunter Bolt / - | MA | Magick Bow | 300 / 1 | 1100 / 3 | n/a | 0x02060001 |
| 361 | Explosive Bolt / Explosive Rivet / Explosive Volley | MA | Magick Bow | 2500 / 6 | 4000 / 8 | n/a | 0x02060001 |
| 362 | Ricochet Seeker / Ricochet Hunter / - | MA | Magick Bow | 1500 / 4 | 2500 / 6 | n/a | 0x02060001 |
| 363 | Magickal Flare / Magickal Gleam / Magickal Radiance | MA | Magick Bow | 1100 / 3 | 2000 / 5 | n/a | 0x02060001 |
| 364 | Funnel Trail / Vortex Trail / - | MA | Magick Bow | 2000 / 5 | 4000 / 8 | n/a | 0x02060001 |
| 365 | Ward Arrow / Great Ward Arrow / - | MA | Magick Bow | 3200 / 7 | 5000 / 9 | n/a | 0x02060001 |
| 366 | Bracer Arrow / Great Bracer Arrow / - | MA | Magick Bow | 3200 / 7 | 5000 / 9 | n/a | 0x02060001 |
| 367 | Sacrificial Bolt / Great Sacrifice / - | MA | Magick Bow | 5500 / 9 | 7500 / 9 | n/a | 0x02060001 |
| 400 | Sixfold Arrow / Tenfold Flurry / - | R | Longbow | 800 / 3 | 1700 / 5 | n/a | 0x02060002 |
| 401 | Heptad Shot / Endecad Shot / - | R | Longbow | 300 / 1 | 1250 / 4 | n/a | 0x02060002 |
| 402 | Dire Arrow / Deathly Arrow / Reaper's Arrow | R | Longbow | 0 / 0 | 800 / 3 | n/a | 0x02060002 |
| 403 | Foot Binder / Body Binder / Secure Binder | R | Longbow | 800 / 3 | 1700 / 5 | n/a | 0x02060002 |
| 404 | Invasive Arrow / Crippling Arrow / - | R | Longbow | 3300 / 7 | 6000 / 9 | n/a | 0x02060002 |
| 405 | Flying Din / Fearful Din / - | R | Longbow | 1700 / 5 | 3300 / 7 | n/a | 0x02060002 |
| 406 | Meteor Shot / Comet Shot / - | R | Longbow | 1250 / 4 | 3300 / 7 | n/a | 0x02060002 |
| 407 | Whirling Arrow / Spiral Arrow / Corkscrew Arrow | R | Longbow | 2500 / 6 | 4500 / 8 | n/a | 0x02060002 |
| 408 | Gamble Draw / Great Gamble / - | R | Longbow | 6500 / 9 | 9000 / 9 | n/a | 0x02060002 |

### Augments

Name message index per augment id: table 0x015214a0 (2 bytes per id, first byte; 0xff none). It is not `id / 10 * 7 + id % 10`: e.g. augment 1 is Sinew (message 68), not Thrust (message 1, unused). Values are `rPlAbilityParam` rows by augment id (PARAMS.md).

| Id | Name | Vocation | Cost | Rank | Description |
|---:|---|---|---:|---:|---|
| 0 | Fitness | Fighter | 700 | 2 | Halves the Stamina consumed when you lift objects or grapple-hold opponents. |
| 1 | Sinew | Fighter | 2800 | 5 | Grants you the energy to carry additional weight. |
| 2 | Egression | Fighter | 2800 | 5 | Facilitates easier escapes from battle. |
| 3 | Prescience | Fighter | 4500 | 7 | Allows you to more easily perform perfect blocks, just as foes strike. |
| 4 | Exhilaration | Fighter | 4500 | 7 | Provides a surge in Strength when you stand at death's door. |
| 5 | Vehemence | Fighter | 6000 | 8 | Augments your Strength. |
| 6 | Vigilance | Fighter | 700 | 2 | Extends the limit of your Health. |
| 10 | Leg-Strength | Strider | 7000 | 9 | Renders you one rank lighter in terms of mobility. |
| 11 | Arm-Strength | Strider | 4500 | 7 | Halves the Stamina consumed when you cling to objects or climb terrain. |
| 12 | Grit | Strider | 2200 | 5 | Helps you maintain your mobility when you run out of Stamina. |
| 13 | Damping | Strider | 1300 | 3 | Decreases recoil after firing your bow. |
| 14 | Dexterity | Strider | 700 | 2 | Enables you to clamber up cliffs and other terrain more quickly. |
| 15 | Eminence | Strider | 2200 | 5 | Renders your jumping attacks more powerful. |
| 16 | Endurance | Strider | 700 | 2 | Extends the limit of your Stamina. |
| 20 | Inflection | Mage | 7000 | 9 | Halves the severity of wounds you sustain whilst intoning spells. |
| 21 | Equanimity | Mage | 700 | 2 | Amplifies your Magick when you stand at death's door. |
| 22 | Beatitude | Mage | 1800 | 4 | Increases the duration of curative magick's effects. |
| 23 | Perpetuation | Mage | 3500 | 7 | Causes enchantments to persist for a longer period of time. |
| 24 | Intervention | Mage | 700 | 2 | Reduces cumulative damage when you are debilitated by the Five Archmagicks. |
| 25 | Attunement | Mage | 7000 | 9 | Augments your Magick. |
| 26 | Apotropaism | Mage | 1800 | 4 | Reduces damage sustained from Magick attacks. |
| 30 | Adamance | Mystic Knight | 1800 | 3 | Halves the Stamina consumed when you block attacks with your shield. |
| 31 | Periphery | Mystic Knight | 1800 | 3 | Helps keep your footing when you block attacks with your shield. |
| 32 | Sanctuary | Mystic Knight | 6500 | 8 | Improves your Defenses and Magick Defenses when you stand at death's door. |
| 33 | Restoration | Mystic Knight | 7500 | 9 | Increases the amount of Health restored when rescuing pawns. |
| 34 | Retribution | Mystic Knight | 4000 | 6 | Grants pawns you aid a temporary gain in their Strength and Magick. |
| 35 | Reinforcement | Mystic Knight | 4000 | 6 | Pawns you aid temporarily take less damage from physical and Magick attacks. |
| 36 | Fortitude | Mystic Knight | 1200 | 1 | Reduces damage sustained from physical attacks. |
| 40 | Watchfulness | Assassin | 1200 | 1 | Halves the severity of wounds you sustain whilst your blade is sheathed. |
| 41 | Preemption | Assassin | 4000 | 6 | Multiplies the harm done by your attacks when targets are not in battle stance. |
| 42 | Autonomy | Assassin | 8000 | 9 | Grants you additional Strength and Defenses when you venture forth alone. |
| 43 | Bloodlust | Assassin | 2400 | 4 | Augments your Strength and mitigates damage taken, but only at nighttime. |
| 44 | Entrancement | Assassin | 1200 | 2 | Increases your Stamina, but only at nighttime. |
| 45 | Sanguinity | Assassin | 1800 | 3 | Increases your Health, but only at nighttime. |
| 46 | Toxicity | Assassin | 7000 | 8 | Sharply increases the cumulative damage wrought by your poison attacks. |
| 50 | Resilience | Magick Archer | 4000 | 6 | Halves the severity of injuries you sustain from falling. |
| 51 | Resistance | Magick Archer | 7500 | 9 | Reduces cumulative damage when you are debilitated by non-Archmagick attacks. |
| 52 | Detection | Magick Archer | 1000 | 2 | Expands one's field of vision, more easily revealing unknown areas of the map. |
| 53 | Regeneration | Magick Archer | 7000 | 8 | Causes your Health to slowly return with the passage of time. |
| 54 | Allure | Magick Archer | 7000 | 8 | Doubles the increase in your affinity when you give items to non-pawns. |
| 55 | Potential | Magick Archer | 1000 | 2 | Extends the limit of your Stamina. |
| 56 | Magnitude | Magick Archer | 7500 | 9 | Increases the effect when you debilitate adversaries with the Five Archmagicks. |
| 60 | Temerity | Warrior | 4500 | 7 | Halves the severity of wounds sustained whilst you mount a focused strike. |
| 61 | Audacity | Warrior | 4500 | 7 | Makes you harder to knock off your feet whilst mounting a focused strike. |
| 62 | Proficiency | Warrior | 10000 | 9 | Reduces the Stamina consumed when you perform a physical weapon skill. |
| 63 | Ferocity | Warrior | 1300 | 3 | Increases the lethality of your Core Skills. |
| 64 | Impact | Warrior | 8000 | 8 | Improves your ability to knock down opponents. |
| 65 | Bastion | Warrior | 900 | 1 | Greatly reduces damage sustained in physical attacks. |
| 66 | Clout | Warrior | 10000 | 9 | Greatly augments your Strength. |
| 70 | Trajectory | Ranger | 3500 | 6 | Causes your arrows to fly farther. |
| 71 | Morbidity | Ranger | 3500 | 6 | Increases cumulative damage to foes you debilitate with non-Archmagick attacks. |
| 72 | Precision | Ranger | 7000 | 8 | Keeps your arrows from flying off mark when you loose them whilst in motion. |
| 73 | Stability | Ranger | 7000 | 8 | Prevents mighty winds from hampering your mobility. |
| 74 | Efficacy | Ranger | 3500 | 6 | Increases the potency of curatives you use. |
| 75 | Radiance | Ranger | 700 | 2 | Causes your lantern to illuminate a wider area. |
| 76 | Longevity | Ranger | 700 | 2 | Extends the limit of your Health. |
| 80 | Gravitas | Sorcerer | 6000 | 7 | Makes it difficult for foes to knock you off your feet whilst you intone spells. |
| 81 | Articulacy | Sorcerer | 9000 | 9 | Diminishes the time required to intone any spell. |
| 82 | Conservation | Sorcerer | 6000 | 7 | Reduces the Stamina consumed when you perform a magick weapon skill. |
| 83 | Emphasis | Sorcerer | 1200 | 2 | Augments the power of magick attacks designed to knock down foes. |
| 84 | Suasion | Sorcerer | 1200 | 2 | Enables you to negotiate higher prices for items and equipment you sell. |
| 85 | Acuity | Sorcerer | 9000 | 9 | Greatly augments your Magick. |
| 86 | Awareness | Sorcerer | 1200 | 1 | Greatly reduces damage sustained in Magick attacks. |
| 100 | Predation | any (unlock flag) | 8000 | 0 | Slightly restores Health when you deliver the killing blow to a foe. |
| 101 | Fortune | any (unlock flag) | 12000 | 0 | Makes it more likely that a foe will drop rift crystals. |
| 102 | Tenacity | any (unlock flag) | 11000 | 0 | Grants a chance that any attack which would reduce your Health to 0 leaves you with 1 Health instead. |
| 103 | Conveyance | any (unlock flag) | 1700 | 0 | Hastens your movement speed whilst  carrying a person or object. |
| 104 | Acquisition | any (unlock flag) | 2000 | 0 | Hastens the speed with which you gather and mine items. |
| 105 | Prolongation | any (unlock flag) | 7000 | 0 | Makes your lantern oil last longer, greatly extending its duration of use. |
| 106 | (no name: 0x015214a0 entry 0xff) | never shown (cost 0) | 0 | 0 | - |
| 107 | Mettle | any (unlock flag) | 8000 | 0 | Increases the ratio of white, recoverable Health when you take damage. |
| 108 | Athleticism | any (unlock flag) | 13000 | 0 | Reduces Stamina consumed whilst running. |
| 109 | Recuperation | any (unlock flag) | 7000 | 0 | Hastens the speed with which you recover lost Health from curative magic and  healing springs. |
| 110 | Adhesion | any (unlock flag) | 10000 | 0 | Makes it harder for an enemy to throw  you off whilst you're clinging to it. |
| 111 | Opportunism | any (unlock flag) | 11000 | 0 | Augments your Strength whilst you cling to a foe. |
| 112 | Flow | any (unlock flag) | 4000 | 0 | Keeps you from being slowed while fording water or marshland. |
| 113 | Grace | any (unlock flag) | 10000 | 0 | Raises the threshold for taking damage from falls, allowing you to safely alight from greater heights. |
| 114 | Facility | any (unlock flag) | 3000 | 0 | Hastens the reloading process for ballistae, allowing you to loose bolts more quickly. |

### Named augment ids not in rSkillList

| Id | Name (0x015214a0) |
|---:|---|
| 90 | Radiance |
| 91 | Suasion |
| 92 | Thrift |
| 93 | Weal |
| 94 | Renown |
| 95 | Sinew |

## Level and experience (traced)

Per character, in `cPlayerInfo` (Arisen sGameSys+0xa76d0, main pawn
+0xa7ec0, support pawns +0xa9520 / +0xaab80): `mLevel` +0x700, `mExp`
+0x2c4 (experience toward the next level), `mNextExp` +0x2c8.

**Next-level requirement** 0x00b5c080 (`ddskill::growth::next_exp`), from
`param\pl\level\exp` (`cPlExp`: +4 `mRate` f32, +8 `mLv` u32, constructor
0x00cd5ed0). Row 0 `mLv` (200) is the requirement at level 1, row 1 `mLv`
(220) the unit. For level L >= 2, starting from 200, each u = 2..L adds
`trunc(rate(u) * 220)` where `rate(u)` is the `mRate` of the last row k >= 2
with `mLv <= u`: 1.3 (L 2-5), 3 (6-13), 5 (14-17), 8 (18-24), 9 (25-35),
10 (36-41), 11 (42-50), 4 (51-100), 15 (101-150), 20 (151-199). Computed in
the x87 FPU with truncation, at (at least) double precision: the running
game returns 485 for level 2 (1.3f x 220 = 285.99999), where single
precision would give 486 (**live**). Total experience (0x00b5c150, cdecl)
is the sum of the curve up to level 199, then the constant 0x022f0778 =
36,636,536 at level 200, 786 more than the curve's sum of 36,635,750.

| Level | Exp to next | Total exp at this level |
|---:|---:|---:|
| 1 | 200 | 0 |
| 2 | 485 | 200 |
| 3 | 770 | 685 |
| 5 | 1340 | 2510 |
| 6 | 2000 | 3850 |
| 10 | 4640 | 15810 |
| 14 | 7720 | 38330 |
| 25 | 25320 | 202230 |
| 50 | 80100 | 1460250 |
| 51 | 80980 | 1540350 |
| 100 | 124100 | 6543250 |
| 101 | 127400 | 6667350 |
| 150 | 289100 | 16790750 |
| 151 | 293500 | 17079850 |
| 199 | 504700 | 36131050 |
| 200 | 0 | 36636536 |

Live values (board #722) agree for next-exp at levels 1, 2, 3, 5, 6, 10,
14, 50, 100, 150, 199 and total at 2, 3, 10, 100, 199, 200.

**Level-up** 0x00b5bd20, called once per frame from the `uPlayer` update
(vf90 0x00b61a20), so one level per frame. Skipped for flags at
`uPlayer`+0x206d (bit 8; bit 2 with +0x31ea; bit 4 with +0x273c bit 0x8000;
meanings unresolved). Needs level < 200 and `mExp >= mNextExp`. Then
`mExp -= mNextExp`, level + 1 (cap 200), the growth row of the *current*
vocation is added, `mNextExp = next_exp(level)` (0 at 200). Growth row:
`rPlLevel` rows from 1 up, the row with `mLevel <= L < next.mLevel` (bands
2-10, 11-100, 101-200 in all shipped tables). Each row adds:

| `cPlLevel` field (0x00cd6610) | Adds to `cPlayerInfo` |
|---|---|
| +6 mHPMax | `mHpMax` +0x2a0, capped at 9999.0 |
| +8 mStaminaMax | `mStaminaLv` +0x2b0 (level part of max stamina) |
| +0xa mAttack | +0x2b4 (`mBasicAttack`, Strength) |
| +0xc mDefend | `mBasicDefend` +0x2b8 |
| +0xe mMgcAttack | `mBasicMgcAttack` +0x2bc |
| +0x10 mMgcDefend | +0x2c0 (`mBasicMgcDefend`) |

Level 1 (0x00b5ccb0) sets those fields to row 1 (e.g. Fighter 450 HP, 80 /
80 / 60 / 60) and `mNextExp` to 200. 0x00b5ccb0 is also a "set level N"
routine that replays the level-ups with the current vocation; 0x00b5cb30
calls it when `cPlayerInfo`+0x35a is set (who sets it is not traced). The
`mNextExp` column of `rPlLevel` is unused by these routines. Side effects on
level-up (player only): achievement bits at level 100 and 200
(DAT_018d0cc4+0x5a74 |= 0x800000 / 0x1000000); at level >= 10 0x0044d080(1)
(meaning unresolved).

**Live** (board #722): the Arisen, a level-7 Mage, has 542 / 120 / 72 / 78 /
104 / 98 (HP, stamina from levels, Strength, Defense, Magick, Magick
Defense) and `mNextExp` 2660; the main pawn, a level-7 Fighter, 630 / 120 /
104 / 98 / 72 / 72. Both equal `Progress::at_level(7, ..)` with their own
vocation's table (install test `live_party_matches`).

Stats after 199 level-ups in one vocation (data, computed):

| Vocation | HP | Stamina from levels | Strength | Defense | Magick | Magick Defense |
|---|---:|---:|---:|---:|---:|---:|
| Fighter | 5550 | 2030 | 576 | 767 | 258 | 168 |
| Strider | 3405 | 3975 | 467 | 467 | 467 | 368 |
| Mage | 3498 | 2080 | 258 | 177 | 676 | 667 |
| Mystic Knight | 4920 | 2480 | 396 | 477 | 448 | 448 |
| Assassin | 3135 | 4155 | 937 | 377 | 277 | 178 |
| Magick Archer | 3498 | 2980 | 358 | 357 | 486 | 667 |
| Warrior | 4820 | 2580 | 766 | 577 | 258 | 168 |
| Ranger | 3045 | 4425 | 657 | 377 | 367 | 368 |
| Sorcerer | 3048 | 2530 | 258 | 177 | 866 | 657 |

### Experience awards (traced)

0x00443470 distributes an award to the four party slots (sGameSys):

- The award is doubled when DAT_018fa4f8+0x20e77b bit 4 is set and the
  stage (sGameSys+0x34) is not 0x25d (meaning unresolved).
- Each present member's share is `trunc(trunc(m * exp + 0.5) * rate)`, `rate`
  the caller's multiplier and `m` from 0x004437d0: the product over the
  other three slots of `clamp(1 + (L_self - L_other) * 0.01, 0.75, 1.25)`,
  an empty slot counting as level 0 (`ddskill::growth::party_share`).
- Slot 0, the Arisen: 0x00782d10 (below).
- Slot 1, the main pawn: share plus a catch-up term
  `(total_arisen - total_pawn) * k * 0.01`, totals = cumulative exp of the
  level plus `mExp`, `k` = `param\game` value 0 = **0.7** (sGameSys+0xb8754
  is the `rAdjustParam` `param\game`, loaded by 0x00444740; read through
  `rAdjustParam::vf15` 0x00ca47d0, which indexes without a bound check),
  floored at 0. All in SSE single precision, then truncated.
- Slots 2, 3, support pawns: no experience; `trunc(share * 0.1)` is added to
  their +0x34c (`OnlinePoint`, rift crystals for the owner; claude-5).

0x00782d10 (`ddskill::growth::award_exp`): `mExp += trunc(exp * w)`, `w` =
augment 93 value (named Weal, 1.15 / 1.0; augment 93 is not in rSkillList,
see below). Discipline `dp = discipline_from_exp(that)` (0x00782f30), times
two equipment factors: item 0x361 (Asura Armor, "boosts the amount of
discipline earned") in slot +0x3e4 multiplies by `param\dlc` value 7 =
**2**, and item 0x47f (Ring of Perseverance) in each of the slots +0x3f4
and +0x3f8 by value 0x1c = **1.5** (sGameSys+0xb875c is the
`rAdjustParam` `param\dlc`, loaded by 0x004447c0; item names from
`itemName_eng` 865 and 1151, which match their descriptions), added to both `mJobExp[mJob]` (+0x2f4) and `mJobPoint`
(+0x344, cap 999,999). `mExp` is capped at 1e9. For one record class
(DAT_0199315c) the award needs +0x7f4 == 1 (unresolved).

**Experience to discipline** 0x00782f30 (SSE single, truncation): the award
is split over bands starting at 0, 50, 100, 150, 200, 300, 400, 500, 700,
900, 1200, 1500, 2000, 3000, 5000, 7500, 10000, 15000, 20000, 30000 that
keep 40, 25, 15, 10, 8, 6, 5, 4, 3, 2.5, 2, 1.5, 1, 0.8, 0.6, 0.5, 0.4,
0.3, 0.2, 0.1 % (table 0x014f24a0). **Live** (board #722): 10 -> 4,
49 -> 19, 50 -> 20, 100 -> 32, 1000 -> 80, 5000 -> 125, 100000 -> 277, all
equal to `discipline_from_exp`. Each award is converted separately, so many
small kills give more discipline than one large award.

### Vocation rank (traced)

0x00b5c230, also once per frame: for the current vocation, below rank 9,
when `mJobExp >= mJobNextExp` (+0x31c) subtract it, rank + 1 (cap 9), set
the next threshold from `rPlJobLevel` (row with `mLevel <= rank < next`,
0x00b5cc50), then posts an event and calls 0x00452d80 (presumably the
"Vocation Rank Increased!" notice, `cockpit_eng` 5; **guessed**). Rank and
rank experience are per vocation and kept across changes. Thresholds (data,
PARAMS.md): rank 1 to 2 needs 500 (most) up to 800 (Mystic Knight,
Assassin); rank 8 to 9 needs 7500 to 14500. **Live**: the Arisen's
thresholds at ranks 1 / 3 equal `rank_next_exp` for all nine vocations.

## Changing vocation (traced)

Where: the rest menu at inns and similar places ("You can learn skills and
change vocations at inns and other locations you can rest.",
`load_message_eng` 46; NPC menu "Change Skills/Vocation", `n783_eng`). The
screen is `uGUIJobChange`; it handles the Arisen (index 0) and the main pawn
(index 1).

- **Offered vocations** (0x00675780): during the free first choice (screen
  flag +0x298) or below level 10, Fighter, Strider, Mage. From level 10: the
  Arisen gets all nine (display order 1,2,3,7,8,9,4,5,6, table 0x01517a18),
  the main pawn six (1,2,3,7,8,9, 0x01517a3c). The tutorial says the same:
  "once you've grown enough as Arisen, you will be able to change vocations
  here" (`tutorial_eng` 199).
- **Cost** (filled by 0x00671b70, charged by 0x00672c10): 1000 discipline for
  basic and advanced vocations, 1500 for Mystic Knight, Assassin and Magick
  Archer (0x01517a7c), unless the vocation's game flag is set (Arisen flags
  540..548, pawn 549..554, table 0x01517a54, bitset sGameSys+0xac6e4 tested
  and set by 0x00401280). Paying sets the flag, so each vocation is paid
  once per character. The first choice is free. Discipline is subtracted
  without a visible floor check in that path (the GUI's availability check
  is not traced).
- **Apply** (0x00675da0, for each changed character): `mJob` = new id;
  0x00673740 empties the six active skill slots, recomputes equipment state
  for all four party slots (0x00442af0) and re-equips (0x006749a0, not
  read); 0x00780590 and 0x00780840 filter skills (above); sGameSys+0xac3c0 +
  index*4 = 1 (reload marker). Counters "Number of Vocation Changes" /
  "Main Pawn's Vocation Changes" (sGameSys+0xb88b4 via 0x0044b7a0 and its
  pawn twin 0x0044b4d0, cap 9999).
- **First choice** extras (0x00673740 with flag +0x298): gives starting
  equipment by vocation: Fighter items 0x141 and 0x1db, Strider 0x195 and
  0x1f8, Mage 0x1aa, then 0x227 and 0x228 for all (item names: claude-6).

**Carries over:** level, experience, all stats (growth already applied),
discipline points, every vocation's rank and rank experience, learned skills
(bitsets are per character, not per vocation), learned and equipped
augments (`mAbility` is not touched on the apply path; augments under 100
can only be *learned* in their vocation but work in any), per-weapon skill
sets of weapons the new vocation also uses. **Lost:** the six active skill
slots, skill sets for weapons the new vocation cannot use, staff slots 3..5
outside Mage and Sorcerer.

## Pawns (notes for claude-5)

- Pawn vocations: 1, 2, 3, 7, 8, 9 only (above).
- Main pawn experience catches up with the Arisen (0x00443470, slot 1).
- Support pawns earn no experience; 10 % of their share becomes +0x34c.
- Random pawns: 0x00796160 loads `rSkillList`, the six pawn `rPlLevel`
  tables, `PlStaminaMax`, `AI\preset_charainfo`, `AI\preset_study`.
- `rPlAbilityParam` rows 90..95 are named Radiance, Suasion, Thrift, Weal,
  Renown, Sinew by 0x015214a0 but are not learnable; augment 93 (Weal) is
  read by the experience award. Who equips 90..95 is not traced (pawn or
  item grants are candidates, **guessed**).

## Implementation in this repo

`crates/ddskill` (new):

- `skill_list::SkillList`: reader and writer for `rSkillList`. Tests:
  synthetic round trip, every truncation and trailing byte rejected; install:
  both archive copies parse and re-encode byte-identically, counts 18 / 262
  / 78, seven augments per vocation, rank 0 for 100+.
- `growth`: `next_exp`, `cumulative_exp`, `level_row`, `Stats`, `Progress`
  (`at_level`, `level_up`), `discipline_from_exp`, `award_exp`,
  `rank_next_exp`, `rank_up`, `party_share`, each marked with its source
  address. Tests: the curve and discipline against live values (#722),
  Fighter growth by hand, level-up carry-over, rank steps, caps, party
  shares; install: all nine vocations to level 200, the live party's stats
  and rank thresholds.
- `vocation`: ids, names (labels), weapon categories, skill ranges, stamina
  tables, motion banks, change cost, unlock flags, offered lists. Tests:
  masks against the exe table values, costs, lists; install: names.
- The exe is read through the shared `ddexe` crate (PE timestamp check, VA
  to file offset, typed reads, class names from `MtDTI` registration
  calls; METHODOLOGY.md, "Reading tables from the installed exe"). Nothing
  from the exe is committed.
- `catalogue`: `ExeTables::from_exe` / `from_image` (skill records, text table, augment
  names, action tables, motion lists) and `Catalogue::build`, which joins
  them with `rSkillList` and the text into `Skill` (levels with names,
  commands, costs and ranks; vocations; weapons; action id, class and
  parameter; motion list) and `Augment` (name, description, vocation, cost,
  rank). Tests: synthetic PE (build check, reads, class names); install
  (`DDDA_NATIVE_PC`, `DDDA_EXE`, skipped when absent): 18 core + 131
  weapon skills and 78 augments resolve, every name rebuilt from the text,
  58 weapon skills with a level 3, vocations consistent with their weapons,
  action parameter = skill id outside the listed exceptions, every motion
  list exists.
- Examples: `cargo run -p ddskill --example skill_dump -- <nativePC>`;
  `cargo run -p ddskill --example catalogue_dump -- <nativePC> [DDDA.exe] [lang]`.

`cargo test -p ddskill`: 16 unit + 9 install tests pass; clippy clean.

## Progression runtime (`crates/ddprogress`)

Host-owned, N-player progression on top of `ddskill` (brief progression).

- `Tables::load(vfs, exe)`: every vocation's `rPlLevel` and `rPlJobLevel`,
  `rPlExp`, `rPlAbilityParam`, `rSkillList`, the exe's skill records,
  `param\game` and `param\dlc`.
- `Character`: one `cPlayerInfo`'s progression (vocation, level and stats,
  discipline and ranks, learn bits, six augments, per-weapon skill sets,
  six active skills, Rift Crystals, paid vocations, change counter,
  discipline gear). New characters start at level 1 with row 1 of their
  vocation, rank 1 in every vocation (the live save shows rank 1 for
  unused vocations) and the cost-0 rank-0 skills known (how the save
  marks those is not traced). `award` is 0x00782d10 (Weal and gear
  included); `step` is one frame of vf90: at most one level-up, then at
  most one rank-up.
- `augment_value` (0x00783340): for the ids in its jump table, equipped ->
  `mFVal0`, else `mFVal1`; id 10 gives -1 / 0; others nothing.
- `learn::check` / `learn`: the skill screen's filters (0x006fd540,
  0x006fdb40, 0x006fdc30) and its confirm check 0x00702b94 (rank met,
  cost <= discipline, not yet known); learning sets the bit
  (0x00780c90 / +0x7dc) and subtracts the cost (0x006fc758). The screen's
  weapon tabs (0x01516738 via 0x01516bd8) cover exactly the weapons of
  the vocation's skill-set list (0x0163e784) for all nine vocations, so
  the list stands in for the tabs.
- `learn::equip_augment`, `equip_skill`, `active_skills`: learned only;
  weapon skills only in their weapon's set and for the current vocation;
  three slots per weapon, six for staves in a Mage's or Sorcerer's hands.
  Two points are **guessed**: refusing the same augment twice, and filling
  the active slots from the per-weapon sets when weapons change (only the
  layout that 0x00780590 enforces is traced).
- `change::check` / `change`: offered list (0x00675780), selectable when not
  current and the cost is affordable (0x00675e8a), cost and flags
  (0x00671b70, 0x00672c10), apply (0x00675da0, 0x00673740, 0x00780840:
  active slots emptied, foreign skills and extra staff slots dropped from
  the new vocation's weapon sets; other weapons' sets untouched), counter
  (0x0044b7a0), starting items on the first choice.
- `Progression::award` (0x00443470 / 0x004437d0) and `step`.

**Co-op adaptations (design, not from the exe):**
- Party roles decide the award path instead of slot numbers: `Arisen(p)`,
  `MainPawn { owner }`, `Support { owner }`. P2's Arisen sits in slot 3
  (COOP_DESIGN.md) and earns experience like slot 0; the level share still
  runs over all four slots. The main pawn catches up toward its owner's
  Arisen. With one player and the main pawn in slots 0 and 1 the result is
  the original's.
- Vocation flags 540..554 are kept per character (`Character::paid`); in
  the original they are global GSF bits of the single save.
  `sync_to_world` / `sync_from_world` mirror player 0's Arisen and the main
  pawn into `ddstory::World` (`gsf`, `player_job`).
- Support pawns' Rift Crystals accumulate on the pawn's record (+0x34c);
  crediting the owner is the host's job (Rift code, not traced here).

Tests (`cargo test -p ddprogress`, install tests skip without the game or
exe): a Fighter from level 1 to 10 one level per step, a change to Warrior
at 10 (level gate, cost 1000, discipline check, flags, carry-over) and
level 11 with the Warrior's row; learning (costs, ranks, level order,
vocation and unlock refusals, rank-up from discipline experience) and
equipping; a change to Mystic Knight keeping the shared sword skills; the
first choice's free change and items; pawn vocation limits; two players,
the main pawn and a hired pawn sharing one award; the single-player layout
and the World mirror; the live level-7 Mage values (#722).

## Open questions

1. Not yet compared live: level-up order within a frame, party shares,
   pawn catch-up, vocation change (cost, flags, slot clearing), learning.
   These change game state; ask claude-4 with a save backup.
2. Unlock of augments 100..114 (save bytes sGameSys+0xbe1b4..): which event
   or purchase sets them (claude-7 events, claude-6 shops).
3. Items that grant level-3 skills (item flag bit 29, skill ids at +8 / +10)
   and the item ids 0x361 / 0x47f that scale discipline (claude-6).
4. Resolved: `param\game` and `param\dlc` (see Experience awards).
5. Inside each action class, which motion of its list plays when (the
   classes' vf05/vf06, ANIMATION.md request path), and what core-skill
   record word 0 means. claude-8.
6. Stamina for staff, magick-shield and bow skills, and the core-skill
   stamina tables (rows below the weapon-skill range) are not traced.
7. Skill table fields +0x14 and +0x18, text-table field +8 and flags.
8. `cPlayerInfo`+0x35a (rebuild stats with the current vocation): who sets it.
9. Proficiency's 1 % discount without the augment (mFVal1 = 1): confirm live.

## Proposed implementation briefs

1. **Character progression**: done (`ddprogress`, brief progression).
   Next: the sim calls `Progression::step` once per frame after stamina,
   and enemy deaths call `award` (combat side).
2. **Skill catalogue**: done (`ddskill::catalogue`, brief skill-catalogue).
3. **Learn and change rules**: done (`ddprogress::learn`, `change`).
4. **Weapon skill stamina and Proficiency** (with claude-8): wire
   `PlStamina_*` rows into skill actions; trace the magick and bow costs.
5. **Level-3 skills from equipment** (with claude-6): item flag bit 29 and
   skill ids in item data.

## Equipped skill runtime (in progress)

`ddskill::magic` reads `rMagicActParamList` (`map\0`, version 4) from the
installation. Loader 0x00CC71A0 allocates 0x490-byte objects; 0x00825F80,
0x00826E90 and 0x008253C0 define the serialized order (769 bytes for a
three-level record). All four installed lists parse: staff 30, shield 15,
human staff 32, NPC staff 2. Truncated and trailing files are rejected.
This closes the earlier unknown about staff stamina: level fields +0xE4
(chant frames) and +0xE8 (signed stamina change) are consumed by 0x00B49B60
and 0x00B47890. Disassembly identifies Articulacy as augment 81 and
Conservation as 82. Both unequipped values are read from PlAbilityParam.

The initial supported actions are base-level Broad Cut (41), Tusk Toss
(43), Ingle (210) and Levin (212). Sword vf06s 0x00B3E290/0x00B3E9A0
select motions 0x73A/0x740. The equipped skill's own list replaces bank 7
(0x00AC65A0); per-character banks prevent one player changing another's
animation. Sword costs use PlStamina_Sword and Proficiency's resource
value. 0x00B60F10 triggers on the **falling** edge of event group 1 bit 30,
not action entry or its rising edge. Regression tests distinguish these.

The base spell path follows chant/release/end phases at 0x00B456C0,
0x00B45DD0 and 0x00B47890. Ingle/Levin select shell groups through action
record parameters and the runtime magic list. The current checkpoint
loads four motion banks and runs all four actions headlessly, charging
once and keeping a second player independent. Damage, viewer keys,
higher levels, manual targeting, chant sharing and LAN skill activation
are not implemented at this checkpoint. No live comparison is claimed.

Shell checkpoint: `skills::spells` loads Ingle's speed shell and hit child,
and Levin's three delayed lightning shells. C12F70/C12D60 provide the
speed, distance and impact-child transitions; BF7A20/BF7CD0 give lightning
jitter and lifetime. Collision phases use distance for speed shells
(C13290), elapsed time otherwise (BD95B0); BD8B90 selects the sequence.
Tests verify the installed collision sequences yield volumes, the speed
ramp, impact child, lightning delays and expiry. The spawn origin uses the
posed chant-ball joint and offset from the magic data (B4A090), not a
fixed height. The deterministic RNG is a co-op adapter: the original RNG
algorithm is unresolved. Target selection is supplied by the host; free
aim/lock selection and wall impacts remain incomplete.
