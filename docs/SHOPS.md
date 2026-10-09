# Shops, prices, enhancement and the item economy

Research for a 1:1 rewrite of DDDA's shops. Addresses refer to the
2017-12-13 Steam build (`METHODOLOGY.md`). Each rule is **traced** (read in
the exe, address given), **data** (read from the install), or **guessed**.
Nothing here has been compared against the running game yet except where a
**live** note says so.

**Live** (claude-4's oracle, read-only, 2026-10-09, request #537 / answer
#542): gold at sGameSys+0xa7a18 (6,500), Rift Crystals at +0xa7a1c (114),
day counter u64 at +0xb8760 (3); item 301 in the loaded item list has buy
1,000 / sell 400; `sNpcManager` stock records hold NPCs 10, 304, 720 then
-1 x 14; record 0 (Aestella) lists items 64, 75, 0, 67, 68, 69, 43, 48 with
stock -1, exactly the first eight lineup entries of `etc\shop\n010ShopList`;
last restock day (+0x48038) = 3 = current day; `plAbilityParam[0x54]` in
memory = 1.15 / 1.0. No purchase, sale or price display has been compared
yet.

Readers and translated rules: `crates/dditem` (`cargo test -p dditem`;
`cargo run -p dditem --example dump -- <nativePC> shops|lvup|curse|mix|items|npcs`
prints the tables with English names).

## 1. Data

| Resource | Path | Loader | Layout |
|---|---|---|---|
| rItemList | `etc\item\itemList` | `0x00cbfb30` | `ITL2`, stamp `0x01330611`, count, u32; 1,901 records of 128 bytes; record N is item N |
| rEquipLvUp | `etc\item\equipLvUp` | `0x00cb8a40` | `qlv\0` v3, 900 records of 40 bytes (section 5) |
| rItemLevelParam | `etc\item\LvParam{Wepon,Armor,Accessory}` | `0x00cbd280` | `ite\0` v`0x1400`; 260/423/43 rows of 36 kinds, 36 f32 rates, 36 u32 "direct" flags |
| rItemMix | `etc\item\itemMix` | `0x00cbff80` | `imx\0` v4; 435 x (`mBaseItemNo0`, `mBaseItemNo1`, `mNewItemNo`, `mNewItemNum`) |
| rItemSetTbl | `etc\item\itemSetTbl`, `ItemEmListSetTbl` | `0x00cc04f0` | `ist\0` v0; 2,800 / 1,116 x 20 u16 (`mSetId`, `mSetType`, `mGetCount`, `mDropCount`, 8 `mItemNo`, 8 `mItemGetProbability`) |
| rItemCurseCnv | `etc\item\itemCurse` | `0x0040e560` | `cit\0` v0; 30 x (`mLevel`, 20 `mGetItemNo`, 20 `mGetItemRate`) |
| rItemRandParam | `etc\item\itemRandParam` | `0x0040e980` | `irp\0` v0; 24 x 7 u32, no property names |
| rNpcLedgerList | `etc\item\NpcList` | `0x00ccac90` | `nnl\0` v8; 727 x (8 u16, 6 u8, `mName`, `mNameJ`) |
| rShopList | `etc\shop\n%03dShopList` (16), `dl1\etc\shop\n156ShopList`, `n784ShopList` | XFS | section 2 |
| rPlAbilityParam | `param\pl\other\plAbilityParam` | XFS (rTable) | 115 x `cPlAbilityParam` (`mFVal0`, `mFVal1`, `mSVal0`, `mSVal1`), indexed by augment id |

Property names come from each class's `vf03` (`cLvupData` `0x00cb8440`,
`rShopList` `0x00cdd610`, `cLineupData` `0x00cdcf20`, judgment
`0x00cc8280`, NPC ledger `0x00cca670`, set table `0x00cc0310`, mix
`0x00cbfdb0`, curse `0x0040e430`).

`sItemManager` (global `0x018fa4d4`, loader `0x0045d730`) keeps the tables:
`+0x110` item list, `+0x114` mix, `+0x118` curse, `+0x11c` rand,
`+0x120/+0x124` set tables, `+0x128` equipLvUp, `+0x12c/130/134` level
params; `mItemBagTbl[5]` (0x11e8 bytes each) from `+0x178`.

### Item record fields used by the economy (traced)

| Offset | Field | Evidence |
|---|---|---|
| +0x00..+0x13 | stat bitfields (attack 0-10, magick 11-21, ...) | `0x006f04b0` passes them to the stat lookup `0x0045b620` |
| +0x34 u16 | `mLevelUpType`: row of the level-stat table | `0x0045b65b` |
| +0x38 bits 0-4 | `mKind`: picks the level-stat table; shop sort kind | `0x0045b620`, `0x006e14b0` |
| +0x38 bits 5-9, 10-11 | category type / kind, copied by the shops | `0x006e14b0` |
| +0x3c bits 0-12 | item id (= index, all 1,901) | data |
| +0x44 f32 | weight | `0x006e14b0` (`*1000`) |
| +0x48 u32 | base buy price | `0x006e14b0`, `0x006e1b40`, `0x004621e0` |
| +0x4c u32 | base sell price; Rift Crystals granted when a rift-point item (flag bit 26) is used | `0x006e17c0`, `0x0045f250` |
| +0x58 | flags; bit 25 = using it adds gold, bit 26 = adds `sell` Rift Crystals | `0x0045f250` |

In the shipped data `sell = 0.4 * buy` for most items, but not all (e.g.
Gold Idol 125,000 / 30,000). The game never derives one from the other: it
reads both fields.

## 2. Who sells what

`uNpc::vf32` (`0x00bace50`) loads the shop list of NPC number N as
`etc\shop\n%03dShopList` into `uNpc+0x6f54`; NPCs 784 and 156 load the
`dl1\` copies instead, so `etc\shop\n156ShopList` and `n784ShopList` are
unused in Dark Arisen. The number is the NPC ledger's `mNo`.

`rShopList` fields: `mShopType` (s32), `mDiscountOkItem0..7` (s16),
`mDiscountRate0..7`, `mDefaultRate`, `mDiscontGSF` (sic), `mDiscountGSFRate`
(u32), `mShopJAnd[10]`, `mShopJOr[10]` (shop open conditions,
"ショップオープン条件"), `mShopLineup` (array of `cLineupData`: `mItemNo`,
`mItemNum` s8, `mItemRearrival` u8, `mLineeupJAnd[10]`, `mLineeupJOr[10]`,
the item open conditions "アイテムオープン条件"). A judgment is `mCommand`
+ `mParam00..03`.

### Shop screen by `mShopType` (traced, `0x00510d70`)

| mShopType | Screen | Currency | Shops (NPC no., ledger name, in-game place) |
|---|---|---|---|
| 0 | uGUIShopItem | gold | 151 Camellia (Camellia's Apothecary) |
| 1 | uGUIShopWeapon | gold | 149 Caxton (Caxton's Armory) |
| 2 | uGUIShopGeneral | gold | 10 Aestella (Aestella's), 11 Iola |
| 3 | uGUIShopJunk | gold | 156 Mountebank (The Black Cat) |
| 4 | uGUIShopPeddler | gold | 7 Madeleine (Madeleine's Shop), 304 Reynard, 398 Alon, 399 Jayce, 400 Mathias, 431 Joye, 432 Delec, 433 Akim, 784 Barroch |
| 6 | uGUIShopRim | Rift Crystals | 720 Johnathan |
| 7 | uGUIShopRarities | gold | 133 Fournival (Fournival Manor) |

Place names are from `id\DDN\message\common\map_placelist_eng`; the ledger
`mName` is an internal romanisation (e.g. `"Caxton "` with a trailing
space). The Rim screen overrides the currency virtuals (`0x006eb7f0`..
`0x006eb850`) to read and spend Rift Crystals (sGameSys+0xa7a1c) instead of
gold (sGameSys+0xa7a18, `cPlayerInfo.mGold` at player+0x348).

The Junk screen (`0x006e45d0`, `0x006e1b40`) first lists the player's sold
items (`sItemManager` junk list: count `+0xe8`, array `+0xf4` of
`cJunkData {u16 mNum; s16 mItemNo at +4}`) at **base buy price, no shop
rate**, then the normal lineup.

### Talk options (traced)

`sIdMessage` (global `0x018fa4d0`, ctor `0x00454f80`) registers 19 talk
command handlers at `+0x44..+0x8c`. The chosen answer is
`[[0x018fa4d0]+0x20]+0x38c`. Shop handlers store the NPC's list at
`[[0x018d099c]+0x3834]+0x5c4`, its number at `+0x5c8` and a mode at
`+0x5cc`, then open GUI 7 (`0x004373c0`):

| Mode | Screen |
|---|---|
| 0 | buy |
| 1 | sell |
| 2 | enhance (`uGUIShopUpgrade::ctor(0)`) |
| 3 | extended enhance (`uGUIShopUpgrade::ctor(1)`, levels 5-6; handler 17 `0x00457150`, no shop list) |
| 4, 5 | equipment screen (`uGUIEquipment`) |

Handler 7 (`0x00456ee0`) offers enhance / sell / buy; handler 4
(`0x004567f0`) buy / sell. Which NPC dialogue invokes which handler is in
the message/scenario data (not traced here).

### Shop open (traced, `0x00cddf60`)

NPC 783 (Olra)'s shop is closed while `sGameSys.mStageNo` (+0x34) is 200.
Otherwise the open conditions are evaluated as in section 3. The NPC being
talked to is `sGameSys.mpTalkUnit` (+0x54); its number is `uNpc+0x5858`.

### Sample stock (data)

Prices are base prices before the shop rate. `num -1` = unlimited.
Conditions: `A`/`O` = AND/OR list, command number, params.

**Caxton's Armory (149)**, mShopType 1, discounts below, open while GSF 23
is clear:

| Item | Buy | Sell | Condition |
|---|---:|---:|---|
| Trusty Sword | 2,700 | 1,080 | |
| Broadsword | 14,600 | 5,840 | |
| Cutlass | 23,600 | 9,440 | progress >= 4800 |
| Eden's Warden | 36,200 | 14,480 | quest 54 flag 18, or flag 17, or flag 16 |
| Tight Cinquedea | 59,400 | 23,760 | progress >= 6400 |
| Caladbolg | 87,800 | 35,120 | quest 54 flag 17 or 16 |
| Almace | 189,000 | 75,600 | quest 54 flag 16 |
| Crossed Cinquedea | 333,000 | 133,200 | progress >= 7800 |

**Camellia's Apothecary (151)**, mShopType 0: Lantern 1,500, Pickaxe 250,
Airtight Flask 320, Empty Flask 20, Flask of Water 90, Flask of Oil 175,
Poison Flask 80 (progress >= 6800), Greenwarish 70, Mushroom 50, ...

**The Black Cat (156, dl1)**, mShopType 3: Hoe 320, Spade 350, ...,
Skeleton Key 4,220 (1 in stock, 1 day per restock), Ferrystone 2,000,
Wakestone Shard 30,000 (1 in stock, 14 days per restock).

**Madeleine's Shop (7)**: tiered stock gated by quest 80's flags 16-18,
with limited counts and restock days (Desiccated Herbs: 3, every 5 days;
Foreign Medicament: 1, every 12 days).

**Barroch (784, dl1)**: everything limited (Balmy Incense 3 / 2 days,
Panacea 3 / 2 days, ...).

**Johnathan (720)**, Rift Crystal shop: pawn inclination elixirs 250 RC,
face pigments 1,000-3,500 RC (1 each).

**Fournival (133)**: periapts, Veteran's and Banker's Periapt need progress
>= 7800 **and** Fournival's affinity >= 850; 10% off while GSF 0x20d is set.

Condition use across all 18 lists (lineup entries): command 11 (story
progress >=) 1,065, 1 (quest flag set) 231 AND + 82 OR, 3 (GSF set) 66 + 30,
12 44, 4 42, 14 (affinity >=) 37, 2 15, 31 5, 10 2.

## 3. Conditions (traced, `0x00cde1b0`)

| Command | True when | Source |
|---|---|---|
| 1 / 2 | quest `p0` flag `p1` set / clear | `0x004356c0`: bit at sGameSys+0xac7a4, quest < 0x80, flag < 0x60, three u32 per quest |
| 3 / 4 | game-state flag `p0` set / clear | `0x00435690`: bit at sGameSys+0xac6e4, flag < 0x600 |
| 10 / 11 / 12 | story progress (sGameSys+0x30, no reflected name) == / >= / < `p0` | inline |
| 13 / 14 / 15 | the NPC's affinity == / >= / < `p1` | NPC record `+0x8b8` (0x110 each) in sNpcManager (`0x018fa4e0`); 0 for a negative NPC, 500 for >= 900 |
| 16 / 31 | player owns / does not own `p0` x max(`p1`, 1) | `0x00460350` |
| 20 / 21 / 22 | party slot `p0`'s level == / >= / < `p1` | record `+0x700`; level per claude-4's live notes |

Unknown commands are false but count as present. A list "has entries" when
any command is non-zero. **Open** = no entries in either list, or (AND has
entries and all are true) or (OR has entries and any is true). Same rule
for shops (`0x00cddf60`) and lineup entries (`0x00cdde40`).

A lineup entry is also hidden (`0x00cdde40`) when:
- its item is in the platform-unlock table at `0x018227f8` (14 items:
  1845-1849, 1856 Liftstone (DL) etc., 1160-1165, 880, 1723) whose state,
  filled from a platform query in `0x004676c0`, is 2;
- it is one of the one-time cosmetics 1613-1633 (pigments, dyes) already
  used (byte flags sGameSys-relative `+0x20e72d..+0x20e741`, set on use by
  `0x0045f250` → `0x00499560`);
- it is item 1634/1635 (Art / Secret of Metamorphosis) and the counter at
  `+0x20e728` is -1;
- with the flag `+0x20e718` set, the entry's state (`+0xa`, computed by
  `0x00cde670` from the shop's seen-bits) is 2; state 1 shows it
  unconditionally.

When an entry opens, its seen-bit is set (`0x00cdcac0`, bit = lineup
index, in the shop's save record `+0x7d8`).

## 4. Prices (traced)

### Buy

`rate` (`0x00cde010`):
1. if `mDiscontGSF != -1` and that game-state flag is set: `mDiscountGSFRate`;
2. else the first `mDiscountOkItem<i>` the player owns (>= 1): `mDiscountRate<i>`;
3. else `mDefaultRate`.

Data: every gold shop except Black Cat, Johnathan and Fournival lists Gold
Idol / Gold Idol Forgery 70%, Silver 80%, Bronze 90% (the forgeries count);
Caxton and Camellia also Badge of Merit 92% and Wyrm Hunt License 95%;
Fournival 90% under GSF 0x20d. All default rates are 100.

`price = base_buy * rate / 100` (u32, `0x006e14b0`). The buy list can be
rebuilt with a "special" flag (`0x006e86c0`: buy mode and an item in the
cart with category kind 0); then items 66 (Ferrystone) and 155 (Wakestone
Shard) are a further 50% off. What this flag represents in play is
**unknown**; a live check is needed.

`0x004621e0` returns `trunc(buy * 1.5)` for another caller
(`0x00b9e0a0`, NPC code, not a shop screen; purpose untraced).

### Sell

`price = trunc(base_sell * F)` (x87, truncating, `0x006e1a02..0x006e1a32`),
then `* P[level] / 100`, then, with the "special" flag and the item tagged 4
in the per-item byte list at `0x017f1ae0`, `* 120 / 100`.

- `P` = 100, 110, 120, 145, 270, 320, 370 for enhancement levels 0..6
  (`0x0158640c`).
- `F` (`0x0044b1f0`) = product over the 4 party slots of `mFVal0` (slot has
  augment 0x54, or a u16 at its bag `+0x1142` is non-zero) or `mFVal1`
  (otherwise or absent) of `plAbilityParam[0x54]`: 1.15 / 1.0. Augment 0x54
  is probably Suasion ("Enables you to negotiate higher prices for items
  and equipment you sell", abilityName index 64): **guessed** mapping
  (id = index + 20), not traced.

Sellable count per bag slot (`0x006e17c0`): `num`, minus 1 when flag bit 1
of the slot (+0x20) is set, minus 2 when bits 1 and 2 are both set (copies
in use; equipped is a guess). Items for which `0x004647e0` is true are not
listed. Sources: player, 3 pawns, and storage (owner 4).

### Gold and Rift Crystals

Add gold `0x004431e0` / spend `0x00443280`; add RC `0x00443310` / spend
`0x004433f0`. Both clamp to 0..99,999,999.

## 5. Enhancement (traced)

Cost table `rEquipLvUp` (one row per enhanceable item; no row = cannot be
enhanced, lookup `0x00cb8d60`). Row: `mItemNo`, unnamed u16 (0 in all 900),
`Lv1_Gold`, then `LvN_Gold`, `LvN_0` (material), `LvN_0_num` for N = 2, 3,
5, 6. There is no LV4 and no level-1 material.

Levels on an item instance are bits of its flag word: 0x8 → 1, 0x10 → 2,
0x20 → 3, 0x40 → 4 (dragonforged), 0x200 → 5, 0x400 → 6 (`0x00462840`).

Offers (`0x006f3350`):
- normal screen (mode 2): targets 1-3 only; an item at 4+ gets nothing;
- extended screen (mode 3): also 5 and 6, but only from level 4+, one step
  at a time;
- level 4 is never bought (dragonforging is elsewhere, **untraced**);
- targets 1-3 cost gold, 5-6 Rift Crystals;
- cost row index: target-1 for 1-3, target-2 for 5-6;
- materials (`0x006f3230`) are counted across the screen's owner list
  (party bags, storage last), one less for a slot with flag bit 1 or bit 8
  set.

Example (data, Iron Sword): 70 G; 220 G + 1 Cassardite; 690 G + 4 Hunk of
Ore; (level 5) 4,000 RC + 3 Lava Rock; (level 6) 5,000 RC + 5 Cyclops Fang.

New stats per level: `rItemLevelParam` row `mLevelUpType` of the table
chosen by `mKind` (7-18 weapons, 19-24 and 27 armour, 25 accessories),
walked by `0x0045b620` (Riftstone's description, confirmed by the callers in
`0x006f04b0`).

## 6. Stock and restock (traced)

`sNpcManager` (`0x018fa4e0`) `mSaveShop`: 17 records of `cSAVE_SHOP`
(0x7f8 bytes) at `+0x3c4b0`: `mNpcId` +4 (-1 free), `mItemNo[250]` +8,
day counters +0x1fc, `mInitDay[250]` (= `mItemRearrival`) +0x2f6,
`mNowNum[250]` +0x3f0, `mInitNum[250]` (= `mItemNum`) +0x5e4, 256 seen-bits
+0x7d8.

- On load (`0x00477ad0`, from `uNpc::vf32`) the record is rebuilt
  (`0x004752d0`) when the lineup's item ids, rearrival or counts differ from
  the saved ones; a new shop takes the first free record. Stock starts full.
  NPC 11 (Iola), items 19, 28, 29 (Berry, Twigbean, Carrot): forced to 99
  with 1-day rearrival.
- Stock left: `0x00477930`; -1 = unlimited. Fournival (133) item 140
  (Maker's Finger): 1 if the player has none, else 0.
- Buy: `0x00477a40` adds the delta, clamped 0..999.
- Restock (`0x00477cb0`), run by `sNpcManager::vf06` (`0x00475ad0`) when the
  in-game day counter (sGameSys+0xb8760, +1 when the clock passes midnight,
  `0x0043f070`) exceeds the last seen day (`+0x48038`), unless sGameSys
  `+0xb8844` bit 0x80000 is set: for each slot with stock < max and
  rearrival > 0, the day counter goes up by 1 and at `rearrival` resets and
  restocks one unit. One tick per update that sees a new day, however many
  days passed.
- The same update runs the daily NPC affinity drift (`0x00477db0`), which
  is not a shop rule (NPC/quest owners).

## 7. Inn and storage (traced)

Inn rest (talk handler 0, `0x004565d0`): price from the table at
`0x0153a694` by innkeeper NPC (`0x004573d0`):

| NPC | Price |
|---|---:|
| 6 Pablos (Pablos' Inn) | 50 |
| 404 Ecbal | 300 |
| 147 Asalam (Gran Soren Union Inn) | 500 |
| 475, 476, 477 (Ser Octavio, Ser Alfonso, Ser Nathaniel) | 100 |

Gold < price → message 0x68; else gold is spent (`0x00443280`), the rest is
started (`0x005188b0`: sets sGameSys+0xb883c bit 0x80000, counts rests at
+0xb8864), and answer 0 / 1 sets `+0xf4` to 0 / 1 (two rest options, not
yet identified).

Storage: owner 4 of every shop list (`0x006e0ff0`), 7,000 item slots
(`0x00465190`; 256 per character bag); saved as
`playerGameData.mStorageItemCount` / `mStorageItem`. The storage screen
(`uGUIStorage`) itself was not traced.

## 8. Bitterblack curse conversion (data + loader)

`rItemCurseCnv`: 30 levels, each a weighted list (weights sum to 100) of up
to 20 items, e.g. level 9: Dread Aspis 17, Devilsbane 17, Bitter End 18,
Devil's Nail 18, Diabolic Shield 15, Virge of Madness 15. The loader stores
running sums. Which level a cursed item rolls and how the seed
(`sItemManager.mItemCurseRandSeedTbl`, `+0x5b04`) is used are **untraced**
(lead: `uGUIRemoveCurse`, `0x004f3520`).

## 9. Unknown

- What the buy/sell "special" flag (`0x006e86c0`) means in play; the
  tag-4 item list at `0x017f1ae0`.
- Augment id 0x54 → Suasion (guessed mapping); the second `F` condition
  (bag u16 at `+0x1142`).
- Dragonforging (level 4), the enhancement commit (payment, flag update),
  and which dialogue opens mode 3.
- Curse purification roll; `rItemRandParam` meaning.
- The buy/sell transaction itself (cart → bag, `cSAVE_DATA_NPC.mBuyAndSellCnt`).
- Inn answer `+0xf4`; the second inn path in handler 14.
- `0x004621e0` (buy x 1.5) caller.

## 10. Proposed implementation briefs

1. **economy-core** (headless crate, depends on `dditem`): per-player
   wallets (gold, RC, clamps), shop session (open check, stock list, rate,
   buy/sell with the traced formulas), `cSAVE_SHOP` stock with restock tick
   on the day counter. N players: each player has a wallet and bags; stock
   is world state owned by the host. Done when the dditem rules drive a
   scripted buy/sell/restock test and match oracle readings for 3 shops.
2. **enhance**: upgrade offers, cost rows, material counting across party
   bags and storage, level flags, stat recompute through
   `rItemLevelParam`; trace the commit function and dragonforging first.
3. **shop-live-check** (claude-4 oracle): read stock, rates and prices
   with and without an idol, with Suasion, and across a day change; decode
   the "special" flag.
4. **inn-storage**: rest price/time (with events-quests for the clock),
   storage capacity, buy-to-storage routing.
5. **curse-purify**: trace `0x004f3520` and the seed table; implement the
   weighted roll from `rItemCurseCnv`.
