# shops-economy: shops, prices, forging and the item economy

- **Owner:** claude lane
- **Branch:** `task/shops-economy`
- **Board area:** `research/shops`
- **Status:** review

## Goal

Find how shops work, so the rewrite can reproduce them 1:1:
- which NPC sells what, where and when (stock lists, changes with story
  progress or time);
- buy and sell price formulas;
- item stats and categories;
- forging and enhancement (materials, costs, results, the dragonforged
  path);
- storage and the inn.

This is research. The output is documentation and readers.

## Starting points

- **DTI classes:** `Shop`, `Item`, `Enhance`, `Forge`, `Craft`, `Storage`,
  `Inn`, `Money`/`Gold`.
- **Data:**
  - item and shop tables in `nativePC`: find them with `paths_of_type`
    over `mtarc::RESOURCE_CLASSES` and by path (`item`, `shop`). XFS dumps
    via `ddtool xfs-get`;
  - item names through `mtgmd` (`id\message\item\itemName_eng`; see
    `docs/TEXT.md`).
- **Exe:** the price computation (sell price as a fraction of value, and
  any modifiers), stock refresh rules, and the forging recipe check.
- **Live checks:** only through the oracle agent (board area `live/game`).
  Never attach to the game yourself.
- Coordinate with `codex-1` (asset-survey covers items broadly) and
  `codex-3` (player-params) on the board.

## Scope

- `docs/SHOPS.md` (new)
- `crates/dditem/**` (new, optional): typed readers for item and shop
  tables, with install tests

## Deliverables

`docs/SHOPS.md`:
- data locations and formats;
- every rule with its exe address, marked traced or guessed;
- a table of a few real shops (keeper's name from the game's text, a
  sample of stock and prices);
- a proposed implementation plan split into briefs.

## Done when

SHOPS.md covers stock, prices and forging, with addresses, or names
exactly what is still unknown. Fill in Results and report to `head`.

## Results

Branch `task/shops-economy`. Docs: `docs/SHOPS.md`. Crate: `crates/dditem`
(`cargo test -p dditem`: 9 synthetic + 4 install tests pass; clippy clean
for the crate).

**Found (traced in the exe, addresses in SHOPS.md):**
- Shops are per NPC: `uNpc::vf32` `0x00bace50` loads `etc\shop\n%03dShopList`
  (dl1 copies for NPCs 784, 156). 18 lists; `mShopType` picks the screen
  (`0x00510d70`): Item, Weapon, General, Junk (Black Cat buy-back), Peddler,
  Rim (Rift Crystals, Johnathan), Rarities (Fournival).
- Buy price = `buy * rate / 100`; rate (`0x00cde010`) = GSF discount, else the
  first owned discount item (Gold/Silver/Bronze Idol or forgery 70/80/90%,
  Badge of Merit 92%, Wyrm Hunt License 95% at Caxton and Camellia), else 100.
- Sell price = `trunc(sell * F) * P[level] / 100`, P = 100/110/120/145/270/
  320/370 (`0x0158640c`), F = product over 4 party slots of 1.15 (augment
  0x54, probably Suasion) or 1.0 (`0x0044b1f0`, `plAbilityParam[0x54]`).
- Open and stock conditions: 16 judgment commands (`0x00cde1b0`), AND/OR
  combination (`0x00cddf60`, `0x00cdde40`), plus DLC, one-time-cosmetic and
  metamorphosis gates.
- Stock: `cSAVE_SHOP` x 17 in sNpcManager; restock one unit per
  `mItemRearrival` days, one tick per day change (`0x00477cb0`, `0x00475ad0`).
- Enhancement: `rEquipLvUp` (`qlv`) layout and names (LV1/2/3/5/6), offers
  and currency (`0x006f3350`): gold to level 3, Rift Crystals for 5 and 6 on
  the extended screen, only from dragonforged (4).
- Inn prices by NPC (`0x0153a694`: 50/300/500/100), storage 7,000 slots,
  bags 256 (`0x00465190`).

**Live:** the layout of gold/RC/day, item prices, the stock save (Aestella's
record matches the data) and the Suasion factor were confirmed read-only
through claude-4's oracle. No transaction compared yet.

**Unknown:** the buy/sell "special" flag (`0x006e86c0`), the augment id
mapping, dragonforging and the enhancement commit, the curse-purification
roll, `rItemRandParam` meaning, the transaction itself, inn rest options,
which dialogue opens the extended enhance screen.

**Next:** briefs in SHOPS.md section 10 (economy-core, enhance,
shop-live-check, inn-storage, curse-purify).
