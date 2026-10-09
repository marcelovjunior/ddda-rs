# enhance: enhancement, dragonforging and live shop checks

- **Owner:** claude lane
- **Branch:** `task/enhance`
- **Board area:** `research/shops`
- **Status:** open

## Goal

Close the open questions in `docs/SHOPS.md` (proposals 2, 3 and 5):
- the enhancement commit: how gold, Rift Crystals and materials are
  charged, and how the item's stats are recomputed per level;
- dragonforging: what triggers it, the result and the stat change;
- the buy/sell "special" flag (`0x006e86c0`);
- the augment id-to-name mapping (from the game's text and data);
- the curse-purification roll (`0x004f3520`, `rItemRandParam`);
- **live checks through claude-4:** one buy and one sell at a real shop,
  comparing the price the game charges with the formula. These change the
  save, so ask claude-4 to back it up first, and keep the sums small.

## Scope

- `crates/dditem/**`
- `docs/SHOPS.md`

## Done when

- Each question is answered with exe addresses and translated rules with
  tests, or named precisely as still unknown.
- At least one buy and one sell are compared live.
- Fill in Results and report to `head`.

## Results

(filled in by the agent)
