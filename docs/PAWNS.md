# Pawns, Riftstones and the Rift

How the original game builds, lists, hires, dismisses and saves pawns, read
from the Steam build (see `METHODOLOGY.md` for the binary identity). Every
address refers to that build.

Evidence labels:

- **traced**: read in the exe (decompiler, checked in the disassembly where
  float order or registers matter);
- **tested**: translated to Rust with a regression test (`crates/ddpawn`,
  `crates/ddsave`);
- **compared**: checked against output of the original game (a real save
  written by it, or its data files);
- **guessed**: inferred from names, data or context, not read in code.

One live read of the running original has been made through the oracle
(claude-4, board #358). It confirmed the pool rules, the story-stock IDs,
the offline mode, the multiplier and the Rift Crystal field
(§3, §4). The remaining live questions are listed in
[Open questions](#open-questions).

## Summary

- **Pawn records** are `cSAVE_DATA_CMC` objects (0x14e8 bytes): appearance
  (`cSAVE_DATA_EDIT`), stats and gear (`cSAVE_DATA_PARAM`), knowledge
  (`mStudyFlag`...), inclinations (`mInfo[9]`) and `cOnlinePawnInfo`. The
  same class is used in memory, in the save, in the built-in data and in the
  network upload. (traced, compared)
- **Where Rift pawns come from**: a 120-slot pawn stock in sGameSys. Slots
  0..19 hold story pawns from `romPawnInfo` (e.g. slot 14 is Rook). Slots
  20..119 are the Rift pool. Online, network pawns go first and generated
  ("nora", 野良) pawns fill the rest. Offline, favourites with built-in IDs
  come first and nora pawns fill the rest. They are drawn around the
  player's level from 33 level-banded files of 900 templates.
  (traced, tested; compared through a real save and a live read of the
  pool)
- **The network** is Steam only: Steam Cloud files shared as UGC, found
  through Steam leaderboards. There is no Capcom HTTP server in the client.
  (traced)
- **Hiring cost** in Rift Crystals: free at or below the player's level and
  for friends' pawns. Above it, `trunc(m*((L+P)*(L-P)^2 + 18))`, with m = 1.0
  offline. (traced, tested)
- **Dismissal or death** writes the pawn into the summon history, past
  summons and a top-4 "close friends" list. Ratings and gifts travel only
  over the network. (traced)
- **The save** is a 32-byte header, a zlib stream and an XML dump of the
  reflected save objects, so every field has the engine's name.
  (traced, tested, compared)

## 1. Offline pawn data

All of this is in `rom\etc\romNoraPawnData.arc`, loaded together by
`cRomPawnManager` (`0x00796160`, manager size 0xc0, owned by sPawnManager
at +0x1574). (traced)

| cRomPawnManager | Resource | Class |
|---|---|---|
| +0x0c..+0x8c | `etc\editPattern\romNoraPawn00`..`32` | `rRomNoraPawn` (33 files) |
| +0x90 | `etc\editPattern\romPawnInfo` | `rRomPawnInfo` |
| +0x94 | `etc\skill\skill` | `rSkillList` |
| +0x98..+0xac | `param\pl\level\LvFighter`, `LvStrider`, `LvMagicUser`, `LvBattler`, `LvHunter`, `LvWizard` | `rPlLevel` |
| +0xb0 | `param\pl\stamina\PlStaminaMax` | `rPlStamina` |
| +0xb4 | `AI\preset_charainfo` | `rAIPresetCharaInfo` |
| +0xb8 | `AI\preset_study` | `rAIPresetStudy` |
| +0xbc | `etc\editPattern\romPawnNameAlphabet` | `rRomPawnName` |

`+4` is a "loaded" flag. A failed load clears it, and all Rift generation
then returns nothing. (traced)

### Nora templates (`rRomNoraPawn`)

Each file holds `mQuality` and an `MtArray` of 900
`rRomNoraPawn::cRomNoraPawn` records (all 33 files: 29,700 templates).
(compared: `ddpawn` install test)

| Field | Offset | Use in the builder `0x00795610` |
|---|---|---|
| `mRomPawnEdit` (`cSAVE_DATA_EDIT`) | +0x04 | copied as the pawn's appearance |
| `mRomPawnWeightType` | +0x58 | row of `PlStaminaMax` for base stamina |
| `mRomPawnJob` | +0x59 | vocation: 1 Fighter, 2 Strider, 3 Mage, 7 Warrior, 8 Ranger, 9 Sorcerer (counts 6722/6831/6757/3169/3111/3110) |
| `mRomPawnAiNo` (s16) | +0x5a | row of `AI\preset_study` copied into the knowledge block (+0x684) |
| `mRomPawnCharaNo` (s16) | +0x5c | entry of `AI\preset_charainfo` (inclinations), applied by `0x00ca7320` |
| `mRomPawnNameNo` | +0x5e | index into the name lists |
| `mRomPawnJobLevel[6]` | +0x5f | job levels for vocations 1,2,3,7,8,9 |
| `mRomPawnNormalSkill[5]` | +0x65 | core skills; each is set only if its `rSkillList` level requirement is met by the current vocation's job level |
| `mRomPawnCustomSkill[6]` | +0x6a | weapon skills, slotted by `0x00796080` per vocation |
| `mRomPawnAbility[6]` | +0x76 | augments; same job-level check; the id is stored converted: `c<63: (c/7)*10 + c%7`, `c>=63: c+37` (69 counts as 68) |
| `mRomPawnEquipItemId[11]` | +0x7c | items 301..0x76c are equipped by `0x00786b90`; 0x74b..0x76c get an extra key from `0x0045e130` |

**Level bands** (traced, tested, compared): a level picks the first file whose
bound is at least the level. The bounds are the 33 bytes at `0x0150a384`:

```
5 9 13 17 21 25 30 35 40 45 50 55 60 65 70 75 80 85 90 95
100 105 110 115 120 125 130 135 140 145 150 170 200
```

So `romNoraPawn00` covers levels 1-5 and `romNoraPawn32` covers 171-200.

**Building a pawn at level L** (`0x00795610`; traced in outline, not
translated):

- The name is replaced by a Latin name when `sGameSys.mLanguage`
  (+0x16d8) is not 0 (Japanese).
- `mLevel` = L and `mJob` = the template's vocation. Job levels are copied.
  Vocations 4-6 (the Arisen-only ones) get job level 1.
- Stats come from the vocation's `rPlLevel` table:
  - Row 1 gives the level-1 values. Each level 2..L adds the gains of the row
    covering that level (the row's u16 at +4 is its first level).
  - Gains: +6 → `mHpMax`, +8 → `mStaminaLv`, +10 → `mBasicAttack`,
    +0xc → `mBasicDefend`, +0xe → `mBasicMgcAttack`, +0x10 → `mBasicMgcDefend`.
  - `mHp` = `mHpMaxWhite` = `mHpMax`.
  - `mStaminaBase` = `PlStaminaMax[weight type]`, and
    `mStamina` = `mStaminaBase` + `mStaminaLv`.
  - Whether this matches the player's own level-up is not checked
    (compare `player-params`).
- Inclinations come from the chara preset, knowledge from the study preset.
  `mPawnType`, `mStatus` and `mEmployGamePlayTime` are zeroed, and the online
  info is reset by `0x0077b2a0`.

### Built-in pawns (`rRomPawnInfo`)

`mQuality`, `mSaveDataCmcArray` (100 complete `cSAVE_DATA_CMC`) and
`mNameNoArray` (100 u32). (compared)

- **Records 0..19 are the story stock.** `0x00796420` copies them into stock
  slots 0..19 with ID `0x2000_0000 | i`. (traced)
  - The names are localised.
  - Slots 10..16 get appearance overrides.
  - Slots 14, 15 and 16 get their names from the NPC table (`0x004769c0`
    with ids 0x1f8, 0x380, 0x381).
  - Record 14 is a level-6 Mage named ルーク (Rook). A real save holds it in
    the summon history as `0x2000000E` (compared).
  - Records 10..16 are level 20/6 pawns; the rest are placeholder copies of
    one level-51 Fighter (data).
- **Records 20..99** are reachable only through a saved ID
  (`0x00795010` rebuilds `romPawnInfo[i]` for `i >= 20`). No offline menu
  lists them; "Special Pawns" (category 7) needs the network. (traced)

### Names (`rRomPawnName`)

Six lists: `mNameFighterMale` (78), `mNameFighterFemale` (72),
`mNameStriderMale` (78), `mNameStriderFemale` (79), `mNameMageMale` (69),
`mNameMageFemale` (64). (compared)

The lookup `0x00cdbf00` picks:

- the Fighter lists for vocation 1 or 7, the Strider lists for 2 or 8, and
  the Mage lists otherwise;
- the male list for gender 0, the female list otherwise.

`name_no` is not bounds-checked. (traced, tested, compared: every nora pawn
in a real save carries exactly this name)

## 2. Pawn unique IDs (`cOnlinePawnInfo.mRomPawnUniqueID`)

From `0x00794f10`, `0x007951c0` (writers) and `0x00795010` (builds a pawn
from an ID). (traced, tested, compared)

| ID | Meaning |
|---|---|
| `0xFFFFFFFF` | not built-in: the player's main pawn, or a network pawn identified by its 64-byte `MtNetUniqueId` |
| `0x1LLLIIII` = `(L + 0x1000) << 16 \| I` | nora template `I` of the band file for level `L`, built at level `L`. On rebuild, `L > 200` becomes 0 and an `I` past the file's end becomes 0 |
| `0x2000IIII` | `I < 20`: copy of stock slot `I` (story pawn). `20 <= I <= 99`: `romPawnInfo[I]` rebuilt with its localised name. `I > 99` is read as 20 |

A real save holds 8 nora IDs, at levels 4 and 6. Each one's appearance,
vocation and English name equal the template its ID names
(`crates/ddsave/tests/private_save.rs`). (compared)

## 3. The pawn stock and the Rift pool

`sGameSys` (global `0x018fa4bc`) holds `mCmcStock`: 120 `cSAVE_DATA_CMC` at
+0x16e0, stride 0x14e8. (traced)

- **Slots 0..19:** the story pawns (above).
- **Slots 20..119:** the 100 pawns of the Rift pool.
- **Not saved:** the save has no 120-element array. (traced, compared)

**Refill.** The pool is rebuilt on every game load and new game.

- The load state machines `0x005327f0` and `0x00532ae0` call `0x0047cc00`,
  then run sPawnManager's fill callback, then `0x00796420` (story stock).
  (traced)
- No refill was found on resting or on a new day. (traced; absence not
  proven)
- `0x0047cc00` stores the load mode in sPawnManager+0x156c. While it is 0,
  the generator uses level 5 instead of the player's. The mode values are
  not traced (guess: 0 = new game). (traced)
- The fill callback:
  - **Online:** `0x0047cd70`, the network search state machine. Downloaded
    pawns go into slots 20 + n (`0x0046d7d0`). `0x0047d330` then adds
    favourites that have built-in IDs, then nora pawns. (traced)
  - **Offline:** `0x0047d330` with n = 0: favourites with built-in IDs
    first, then `0x007951c0` fills the remaining `100 - n` slots. The
    offline-only path `0x0047d590` fills all 100. (traced)

**Generating the pool** (`0x007951c0`, translated in
`ddpawn::rift::fill_offline_pool`; traced, tested):

1. P = player level (sGameSys+0xa7dd0), clamped to 5..200. It is 5 when no
   player exists yet or the load mode is 0.
2. The `(offset, count)` table at `0x0150a358` gives the levels: 25 pawns at
   P, then +2 x16, -2 x2, +4 x16, -4 x2, +6 x10, -6 x2, +8 x6, -8 x1,
   +10 x6, -10 x1, +12 x2, +16 x2, +20 x2, +30 x2, +40, +50, +60, +80 and
   +100 x1 each (100 in all). A level outside 1..200 becomes P.
3. Pawn k goes to stock slot 119 - k.
4. Template: start at `rng() % 900` in the band file and walk forward while
   the ID is already in slots 20..119. Stale IDs from the previous fill
   count too. Past the end of the file every further try uses index 0
   (an exe quirk, kept in the translation). If every try fails, the pawn is
   redrawn.

**Compared live** (oracle, board #358; read-only memory of the running
original):

- The game was offline (sNetwork+0x545 = 1, 0 downloads), the player at
  level 7, load mode 1.
- Slots 0..19 held `0x20000000`..`0x20000013` in order.
- Slots 20..119 held 100 distinct nora IDs: one favourite with a built-in
  ID (`0x1004005E`, also in the save's `mFavoritePawn`; the other favourite
  was in the party and was skipped) and 99 generated pawns.
- The 99 levels match the table for P = 7 exactly: 25 at 7, 16 at 9, ...,
  one at 87. The +100 entry is the one not drawn, because the favourite
  took a slot.
- Regression test: `ddpawn` `live_pool_level_multiset`.
- The placement step had moved the level-7 pawns into slots 88..119.

**RNG.** The random generator is the global at `0x018d017c`: a xorshift128
with shifts (15, 17, 4) (`0x00d00690`, tested). Its seeding is not traced,
so the exact pool cannot be reproduced yet.

**Placement in the world.** `0x0047ddf0(stage)` arranges stock slots into
groups for stages 100, 210, 220/230 and 601, via `0x0047d610`. (traced)

- Friends come first, then favourites, past summons and lent pawns, then
  pawns near the player's level. Slots inside a group are then shuffled.
- Groups: [88,120), [50,58), [20,50), [58,88).
- That these are the pawns standing in towns and the Rift is guessed.
- Placed units name their stock slot by a short at setinfo+0xe
  (`0x0073b310`).

## 4. The Rift

sPawnManager is the global `0x018fa4e8`; uRim is the Rift room. uRim
`0x0073b5c0(cat)` sets the search category (sPawnManager+0x154c) and resets
the status (+0x1544). The GUI keeps the same value at uGUIRimSearch+0x618.
(traced)

Menu text: `id\message\pawn_search\pawn_search_<lang>`.

| Category | Menu item (message) | Offline |
|---|---|---|
| 1 | Pawns by Level (36) | yes: nora pawns at the chosen level |
| 3, 4 | Pawns by username / online ID (279) | no |
| 6 | Top Ranked (66) | no |
| 7 | Special Pawns (67; guessed) | no |
| 0x12 | Top Ranked This Month (300) | no |
| 0xd (9 = network-only twin) | Past Summons (68), list sGameSys+0x9e404 = `mBorrowPawn` | yes |
| 0xe (10) | Favorites List (69), list +0x9e3ec = `mFavoritePawn` | yes |
| 0xf (0xb) | Pawns Your Main Pawn Knows (70), list +0x9e41c = `mLentPawn` | no |
| 0x10 (0xc) | Friend List (71) | no |

- Categories 2, 5, 8 and 0x11 are unmapped.
- Offline, categories 3-8, 0xb, 0xc, 0xf, 0x10 and 0x12 end with status 6
  (`0x0047a460`). (traced)
- Online means sNetwork (`0x018d0cc4`) +0x545 == 2, or == 4 with +0x5a34 == 0.

**Building the list** (`0x0047a550`; traced). It makes at most 100 entries.

- For "by level": network results first, then `0x00794f10` nora pawns at
  exactly the selected level. The level is sPawnManager+0x1528, the cursor
  + 1, at most 200, shown as "99~" above 99.
- A duplicate ID is redrawn, up to 100 times.
- The vocation, gender and weapon-skill filters (+0x152c, +0x1530) are only
  read by the network query (`cNetPawnFilter`: `mBoard mLevel mGender mJob
  mSkill mEnemy mSortKey mSortType`). Offline they seem to have no effect.
  (traced as far as `0x0047a550`/`0x00794f10`; a GUI-side filter is not
  ruled out)
- `0x0047af50` moves to the front the pawns whose knowledge passes
  `0x005d0760` (guessed: knowledge of the current quest).

Each row is an `nRim::cPawnInfoList` (0x18 bytes):
`{+4 flags, +8 index, +0xc, +0x10 mDDVersion, +0x14 cCmcInfo*}`. Flags
(traced; messages from `0x006d7ba0`):

| Flag | Set when | On summon |
|---|---|---|
| 0x1000 | level 0 (no data) | 263 "You cannot summon this pawn." (dropped from lists 0xd-0xf) |
| 0x2000 | network pawn while offline | 272 "You cannot summon pawns while offline." |
| 0x4000 | network pawn whose net id fails `0x00d00f20` | 263 |
| 0x8000 | network pawn failed to load (`0x0047ad50`) | 263 |
| 0x800 | already summoned into the Rift (`0x0073b880`) | 269 "This pawn is already in the rift." |
| 0x400 | after a successful summon | |

**Summoning and hiring** (traced):

- **Summon.** `0x0073bae0` places a chosen pawn in the Rift room.
  - The room has 7 slots at uRim+0xf0, 0x70 bytes each. Slot state +0x14:
    1/3 present, 2 hired, 4 released.
  - At most 5 called pawns stand there at once (uRim+0x410 > 4 refuses). The
    text "up to ten pawns" (message 60) does not match this code.
- **Hire.** The player talks to a summoned pawn: `0x0073cca0` checks the
  cost, then `0x0073c2c0` hires. Return codes:
  - 5: no pawn;
  - 2: not enough Rift Crystals ("You don't have enough rift crystals to
    enter this covenant.");
  - 1: two hired pawns already (uRim+0x40c >= 2);
  - 0: hired.
- **Target slot.** The slot freed by the last dismissal (uRim+0x45c), else
  party slot 1, else 2.
  - `0x0043e5b0` installs the `cSAVE_DATA_CMC` into the party `cCmcInfo` at
    sGameSys+0xa76d0 + slot × 0x1660. Slot 0 is the Arisen (its +0x700 is
    the player level at sGameSys+0xa7dd0); +0x7f4 marks the slot occupied.
  - Which party slot holds the main pawn is not settled.
- **Swap.** Hiring when full asks which pawn to release (`0x0073cca0`
  checks the cost first, then releases, then hires).

**Hiring cost** (`0x0043eec0`, translated in `ddpawn::rift::hire_cost`;
traced, tested). The same formula appears in the list's "Base Cost"
(`0x0043edf0`) and in the level-select preview (`0x006d0f50`, `0x006d10f0`,
`0x006d13a0`). With P = player level and L = pawn level:

```
0                                         if the owner is a friend (0x0043ef90; network pawns only)
0                                         if L <= P
trunc(m * ((L + P) * (L - P)^2 + 18.0))   otherwise, single precision, clamped to 0..=99,999,999
```

- 18.0 is the f32 at `0x0150a2fc`.
- m is sNetwork+0x5ab0:
  - 1.0 from the sNetwork constructor `0x004676c0` and whenever the online
    setting fails;
  - online, a downloaded integer × 0.001 (`0x004704e0`);
  - reset to 1.0 when it is outside 0.009..100.001.
- Example: P = 10, L = 20 costs 3018.
- There is no level cap on hiring.

**Rift Crystals.**

- The player's balance is sGameSys+0xa7a1c, saved as the Arisen's
  `cSAVE_DATA_PARAM.OnlinePoint` (+0x1bc).
- Spending: `0x004433f0` subtracts and floors at 0.
- Gaining: `0x00443310` adds, capped at 99,999,999.
- The GUI shows the balance clamped to 99,999,999 (`0x006cd880`).
- (traced. Compared live: sGameSys+0xa7a1c = 114 = the manual save's
  `OnlinePoint`. Offline multiplier read live: 1.0.)

## 5. Dismissing, death and history

- **Voluntary release** (Riftstone `0x0073c480`, pawn talk `0x00b9cec0`,
  `0x00498a50`) goes through `0x0043e8b0(slot)`. It takes back the items
  tagged with that pawn as owner (`0x0045f850`), then calls
  `0x0043ec60(slot, died = 0)`. (traced)
- **Death or loss** (`cPlActDmgInvisibleDead`/`cPlActDmgStoneDead` via
  `0x00b87cc0`, and a scene step `0x005278d0`) goes through `0x0043e9b0`
  and then `0x0043ec60(slot, died = 1)`. (traced)
  - If the party slot's +0x7f4 is 2, the pawn is only flagged and stays.
    The meaning is unknown; it may be "retaining" (message 273).
- **What `0x0043ec60` records** for a hired pawn (traced):
  - `mWithTime` (play time since hire), `mEmployCnt` += 1, `mDieCnt` +=
    died, and `mClearQuestCnt` (quests cleared while hired). These are added
    to the pawn's Favorites entry if it has one, otherwise to its Past
    Summons entry (`0x00448da0`).
  - The pawn is appended to Past Summons: FIFO, 100 entries, oldest dropped
    (`0x00448130`). Its "knows your pawn" entry is updated if present
    (`0x00448470`).
  - It enters `mCloseFriendPawn`, a top-4 list of full pawn snapshots sorted
    by `mWithTime` (`0x00448500`).
  - A `cPawnHistoryInfo` (edit, online info, level, vocation, skills,
    equipment) is appended to `mPawnHistory`. That list holds 100 entries;
    when full it evicts the first entry not in the lent list (`0x00449380`).
- No rule was found that sends a hired pawn back when the player outlevels
  it. (traced, absence not proven)

## 6. Ratings, gifts and your own pawn's return

- **Evaluate screen.** `uGUIRimEvaluate` starts with Appearance, Battle and
  Helpfulness at 2 and two comment slots at 0xff (`0x006c9060`). The comment
  texts are `pawn_revcom_comment0..107`. Where the values go is not traced;
  it is probably the network reward record. (traced in part)
- **Your main pawn's ratings** (traced):
  - `0x00449670` adds received (app, btl, cont) to sGameSys+0xa764c.. and
    increments the count.
  - Shown and uploaded as `ceil(sum / num)` per axis, clamped to 1..5, or 0
    when there are none (`0x0077b990`, `0x0077b4e0`).
  - Saved as `networkGameData.mTotalVal{App,Btl,Cont,Num}`.
- **Return at the inn** (`0x00737900`, state 0xb; network only). For each
  reward record:
  - Gift items go to storage (at most 10). An item is skipped if its item
    list flags (+0x58) have bit 27 (`mUnused`) or bit 19 (`mRimShop`).
  - RC: each record is capped at 1,000,000 and the sum is added.
  - Ratings are applied, and the hirer joins the lent list.
  - `mInnPlLevel`, `mYMD` and `mHMS` are stamped (`0x00737030`).
  - Offline nothing comes back: there are no simulated rewards. (traced)
- **Sending a gift** when dismissing ("You may send it away with a gift"):
  the rule that refuses an item ("You cannot gift that item") and the writer
  of `cSAVE_DATA_PARAM.mPresentItem` are not found.
- **`mPresent0` / `mPresent1`** (u64[12] each, low 16 bits = item number,
  65535 empty) map to sItemManager+0x27d0/+0x39b8 (`0x00494d80`).
  Guess: the hired pawns' original equipment, for "Return to its previously
  equipped items?".

## 7. Network protocol (client side only; no server contacted)

All of this is traced unless marked.

- **Transport.** Steam only. WININET is used only for
  `InternetGetConnectedState` (`0x00d2a390`, together with
  `ISteamUser::BLoggedOn`: 1 offline, 2 online). There are no HTTP URLs.
- **Cloud files.** `uNetStorage` writes a Steam Cloud file (`FileWrite`,
  read back, `FileShare` → UGC handle; `0x00da3950`, `0x00da3ae0`).
  - It posts the 8-byte UGC handle as the 2-int "details" of an entry in
    leaderboard `"leader_23" + (file name + 1)`, with a random score
    (`0x00da3c10`, `0x00da3300`).
  - Readers fetch that entry by SteamID and call `UGCDownload`
    (`0x00da3210`).

  | Cloud file | Size | Board | Contents |
  |---|---|---|---|
  | `0` | 0x2000 | `leader_231` | your main pawn (`cNetStoragePawn::cSaveData`) |
  | `1`..`7` | 0x2000 | `leader_232..238` | reward records you wrote for pawns you hired (20 × 0x594 + 0x830 header) |
  | `8` | 0x800 | `leader_239` | `cNetStorageDiff`: 100 × 0x6c counters |

  The 8 KiB files `0`-`7` next to `DDDA.sav` are these files.
- **Pawn file.** `cNetStoragePawn::cSaveData` (0x1578 bytes: `mCmc`,
  `mPlEdit`, `mPlWeapon[12]`, `mPlJob`). Before upload the per-enemy study
  arrays are zeroed.
  - Encoding: MtDTI binary serialisation (`0x00d0ca00`), zlib (≤ 0x1fd8),
    a 0x20-byte header {magic, length, 0, SHA-1}, Blowfish-ECB, then
    `u32 0x12121300, u32 length`, ciphertext, 0xdd padding to 0x2000
    (writer `0x00c9e700`, reader `0x00c9ee90`).
  - The reader also accepts the old magic 0x11090800.
  - The Blowfish key is a literal in the exe (passed at `0x00c9edd0`); it is
    deliberately not reproduced here.
- **Search and ranking boards.** All named `"leader_%d"`:
  - level boards (1-98 → level, 99-199 → level + 26, 200 → 226);
  - decade boards `0x65 + 2*((min(L,99)-1)/10)`;
  - 0xe3 "top ranked" and 0xe4/0xe5 by month parity (meanings guessed).
  - Details are 7 × 8 bytes, each Blowfish-encrypted with a second key, and
    hold a bit-packed summary: nickname, gender, voice, personality,
    vocation and level in one word, plus weapon/skill categories
    (`0x00c98040`, `0x00c999a0`).
  - The client filters downloaded entries with `cNetPawnFilter`.
  - Results enter the pool in `0x0046d7d0`: your own id is skipped,
    `mPawnOpenType` 2 is skipped, and 1 requires a friend.
- **Rewards.** `cNetRewardStock::cData`: `mItemId,
  mInitialWeaponEquipFlag, mPresentWeaponEquipFlag, mFragmentNum` (RC),
  `mUserValue` (rating), `mUserComment, mStudyFlag, mStudyData.*` (knowledge
  learned away), `mFlag, mItemCtr, mNetUniqueId`. How an owner finds the
  rewards addressed to it is guessed (downloading hirers' files).
- **Fallback.** When not online, every network count is 0 and the nora
  generator supplies all pawns. This is the full offline game.

## 8. The save file

`ddsave` reads it (read-only; traced, tested, compared). The default save
is Steam `userdata\<id>\367500\remote\DDDA.sav` (524,288 bytes).

**Header.** Writer `0x0040f1a0`, verifier `0x0040f400`. Eight
little-endian u32:

| Offset | Value |
|---|---|
| +0x00 | version (21 in the observed save, equal to the root's `mCreateNo`; the expected constant passed by the caller is not traced) |
| +0x04 | inflated length |
| +0x08 | compressed length |
| +0x0c | `0x334d234d` |
| +0x10 | 0 |
| +0x14 | `0x334d4044` |
| +0x18 | checksum: CRC-32 of the compressed bytes without the final inversion (init 0xffffffff, table `0x014075c0`) |
| +0x1c | `0x40565235` |

- The zlib stream follows, then zero padding.
- The verifier finds the markers by search. It also accepts byte-swapped
  (console) headers and a legacy 12-byte header.
- Results: 3 = OK; 1 = checksum mismatch, mapped to error 2; 2 = bad header,
  mapped to error 5 (`0x0040f880`).

**Payload.** UTF-8 XML. The root is `<class name="dd_savedata1018"
type="sSave::saveDataAllDA">`.

- Elements are typed (`class classref array bool u8..u64 s8..s32 f32 string
  vector3 time`) and carry the reflected field names.
- One real save inflates to 20,362,849 bytes and 689,908 elements.

**Pawn fields.** `mPlayerDataManual` and `mPlayerDataBase` are two
`sSave::playerData` slots, apparently the manual save and the checkpoint;
which is which is guessed from their play times. Each has:

| Path | Meaning |
|---|---|
| `mPlCmcEditAndParam/mPl/mParam/OnlinePoint` | Arisen's Rift Crystals |
| `mPlCmcEditAndParam/mCmc[3]` | party pawns (slot 0: `mPawnType` 1, the main pawn; 1-2: hired) |
| `mPlGameData/mCmcPos, mCmcAng, mCmcBeFlag, mCmcIsDead, mCmcIsNearDeath` | party pawn placement/state |
| `mPlGameData/mPresent0, mPresent1` | see §6 |
| `mNetGameData/mFavoritePawn[100]`, `mBorrowPawn[100]`, `mLentPawn[100]` (`cSAVE_DATA_ONLINE_PAWN`) + counts | Favorites, Past Summons, Pawns Your Main Pawn Knows |
| `mNetGameData/mCloseFriendPawn[4]` (`cSAVE_DATA_CMC`) | top-4 by time together |
| `mNetGameData/mPawnHistory[100]` (`cPawnHistoryInfo`) | summon history |
| `mNetGameData/mTotalVal*`, `mInnPlLevel`, `mYMD`, `mHMS`, `mNetworkDownloadCount`, `mLastUploadPawnLevel` | own pawn ratings and return bookkeeping |
| `mAdventureLog/mLogPawnRental{Man,Woman,Rom}`, `mLogMyPawnBeRented`... | statistics |
| `mAdventureLog/mLogQuest[64]` | quests with the two hired pawns' IDs and names |

`mSystemData` also keeps `mEditPawn`, `mLastClearPawnData` and
`mLastClearCloseFriendPawnData[4]` for New Game+.

The `mStudyFlag[322]` and `mStudyData` arrays are the pawn knowledge
(enemies, quests, areas). Their decoding belongs to the AI work.

## 9. Memory layouts

From the reflected property tables (`research/dti/dti_props.py`, vf03
registrations) and the code above.

**`cSAVE_DATA_CMC`** (0x14e8; vf03 `0x00787f30`):

| Offset | Field |
|---|---|
| +0x04 | `mEdit` (name +0x08, 25 bytes) |
| +0x58 | `mParam` |
| +0x680 | `mStatus` |
| +0x684 | `mStudyFlag[322]` |
| +0xb8c | `EncountFrame[72]` |
| +0xcac | `KillCnt[72]` |
| +0xdcc | `UniqueCnt[116]` |
| +0xe40 | `mLocalStudyFlag[322]` |
| +0x1348 | `mInfo[9]` {type, f32, status} |
| +0x13b4 | `mInfoEx[2]` |
| +0x13c4 | `mUseSceneTalk[6]` |
| +0x13dc | `mPawnType` |
| +0x13e0 | `mEmployGamePlayTime` |
| +0x13e4 | `mOnlinePawnInfo` |

**`cSAVE_DATA_PARAM`** (0x628; vf03 `0x00785790`):

| Offset | Field |
|---|---|
| +4 | `mLevel` |
| +5 | `mJob` |
| +6 | `mJobLevel[10]`, indexed by vocation number |
| +0x18c | `mHpMax` |
| +0x19c | `mStaminaLv` |
| +0x1a0..+0x1ac | attack/defence |
| +0x1b8 | `mGold` |
| +0x1bc | `OnlinePoint` |
| +0x488 | `mEquipItem[12]` |
| +0x608 | `mPresentItem` |

**`cOnlinePawnInfo`** (0x100; vf03 `0x0077ad50`):

| Offset | Field |
|---|---|
| +0x04 | `mNetUniqueId` (64-byte data) |
| +0x80 | `mWithTime` |
| +0x84 | `mEmployCnt` |
| +0x88 | `mDieCnt` |
| +0x8c | `mClearQuestCnt` |
| +0x90 | `mLentCnt` |
| +0x94 | `mRomPawnUniqueID` |
| +0xb8 | `mArisenName` |
| +0xdc | `mPawnOpenType` |
| +0xe0 | `mLanguage` |
| +0xe4.. | Arisen play time, level, vocation, vocation level |
| +0xf4..+0xf6 | `mTotalVal*` |
| +0xf8 | `mValuationCommentPawnID` |
| +0xfc | `mDDVersion` |

**Other records:**

- `cSAVE_DATA_ONLINE_PAWN` (0xa8): net id at +4, counters at +0x44, ID at
  +0x5c.
- `cPawnHistoryInfo` (0x1c8).
- `cCmcInfo` (0x1660, the runtime party/list record): level +0x700,
  occupied +0x7f4, `cOnlinePawnInfo` +0x1550 (ID +0x15e4).

**sGameSys** (`0x018fa4bc`):

| Offset | Field |
|---|---|
| +0x16d8 | `mLanguage` |
| +0x16e0 | `mCmcStock[120]` |
| +0x9e3ec | Favorites (MtArray) |
| +0x9e404 | Past Summons (MtArray) |
| +0x9e41c | Lent (MtArray) |
| +0x9e468 | close-friend count |
| +0x9e470 | close friends |
| +0xa3830 | history |
| +0xa764c | own ratings |
| +0xa76d0 | party `cCmcInfo[4]` |
| +0xa7a1c | Rift Crystals |
| +0xa7dd0 | player level |

Save writer `0x0048ed00`, loader `0x0048f080`.

**sPawnManager** (`0x018fa4e8`):

| Offset | Field |
|---|---|
| +0x1524..+0x1530 | search request |
| +0x1544 | status |
| +0x154c | category |
| +0x1564 | pool ready |
| +0x1568 | fill callback |
| +0x156c | load mode |
| +0x1574 | `cRomPawnManager*` |

**sNetwork** (`0x018d0cc4`):

| Offset | Field |
|---|---|
| +0x545 | mode |
| +0x5aa8 | downloaded pawn count |
| +0x5ab0 | hire-cost multiplier |

## 10. Co-op notes

`COOP_DESIGN.md` gives P2 the second hired-pawn slot, so a co-op party
can hire one pawn. The exe ties the following to "the player"; each needs a
decision:

- **Pool generation level.** P = the Arisen's level at load. Proposal: the
  host Arisen, since the pool is host-owned world state.
- **Hiring cost.** It uses the hiring player's level against the pawn's.
  Proposal: the level of the player who hires.
- **Rift Crystals** are per Arisen (`OnlinePoint` is in each player's
  `cSAVE_DATA_PARAM`). Each player pays from their own balance.
- **The hired-pawn limit** (uRim+0x40c ≥ 2) becomes 1 in co-op.
- **History and favourites** are per save: host only, unless saves become
  per player (an open question in COOP_DESIGN).
- **Pawn rental between the two players' Riftstones** could reuse the
  `0xFFFFFFFF` + net-id path with a LAN id instead of a Steam one.

## Open questions

Live checks for the oracle (claude-4); all are read-only memory reads
unless stated:

1. Done (#358): the pool after a load follows the level table (§3). Still
   open: a pool built with load mode 0 (new game), which should be centred
   on level 5.
2. The hire cost of a pawn above the player's level, read from the list's
   "Base Cost": `(L+P)(L-P)^2+18`.
3. Whether the vocation/gender filters change offline "Pawns by Level"
   results.
4. The seed of the xorshift state at `0x018d017c`, for a bit-exact pool.
5. Whether a sixth summon into the Rift is refused (5-pawn cap).

Code still to trace:

- The constant passed as the save version.
- The gift-refusal rule and the `mPresentItem` writer.
- Where the evaluate screen's ratings go.
- Party `cCmcInfo+0x7f4 == 2` ("retaining"?).
- Which party `cCmcInfo` slot is the main pawn.
- The mode values of `0x0047cc00`.
- The story stock overrides in `0x00796420`.
- Categories 2, 5, 8 and 0x11.
- `mKaiouData` / `mKaiouPornData`.
- The full pawn builder `0x00795610`: the weapon-skill slotting
  `0x00796080` and the level accumulation details.

## Implementation briefs (proposed)

1. **pawn-builder** (rules, `crates/ddpawn`): translate `0x00795610` and
   `0x00796080` to build a full `cSAVE_DATA_CMC`-equivalent pawn (stats,
   skills, augments, equipment, inclinations, knowledge) from a nora
   template and a level. It needs typed `rPlLevel`/`rPlStamina` (reuse
   player-params), `rSkillList`, `rAIPresetCharaInfo` and `rAIPresetStudy`
   readers.
   *Done when* a nora pawn rebuilt from a save's ID reproduces that save's
   `mParam` stats, skills and equipment for all 8 nora pawns in the private
   save, with the translation marked and tested.
2. **story-stock** (rules + data): translate `0x00796420` (stock slots
   0..19 from `romPawnInfo`, the overrides for slots 10..16, NPC-table
   names). *Done when* slot 14 rebuilds as the saved Rook record
   (`0x2000000E` in a save) field for field.
3. **rift-sim** (core, headless): sPawnManager/uRim as a host-owned
   resource: pool refill on load, the category list builder (`0x0047a550`)
   with offline semantics, summon (5 in the room), hire (cost, RC, slot
   choice), dismiss/death bookkeeping (`0x0043ec60`: past summons, close
   friends, history). Player-scoped calls take a player id.
   *Done when* unit tests cover each traced rule and an oracle session
   confirms open questions 1, 2 and 5.
4. **save-pawns** (reader, `crates/ddsave`): typed knowledge
   (`mStudyFlag`/`mStudyData`) and equipment decoding, plus a JSON/CLI dump
   (`ddtool save-pawns`) for comparisons. *Done when* both playerData slots
   and `mSystemData` pawn records round-trip into typed structs without
   loss of the fields the rift-sim needs.
5. **rift-ui** (presentation): the Rift menus from `pawn_search_<lang>` and
   the GUI resources `id\DDN\pawn2\*`, `id\pawn2\pawn_review`. *Done when*
   the offline menu tree (categories 1, 0xd, 0xe) is navigable and shows
   the same rows and Base Cost as the original for a given pool (oracle
   screenshot).
6. **lan-rental** (co-op, later): exchange main pawns between two LAN
   players as `cSAVE_DATA_CMC` records, with rewards that mirror
   `cNetRewardStock`. It depends on the COOP_DESIGN decisions in §10.
