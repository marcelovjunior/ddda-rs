# shops-economy: shops, prices, forging and the item economy

- **Owner:** claude lane
- **Branch:** `task/shops-economy`
- **Board area:** `research/shops`
- **Status:** open

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

(filled in by the agent)
