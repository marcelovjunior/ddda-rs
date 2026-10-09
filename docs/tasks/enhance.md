# enhance: enhancement, dragonforging and live shop checks

- **Owner:** claude lane
- **Branch:** `task/enhance`
- **Board area:** `research/shops`
- **Status:** review

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

Branch `task/enhance`, commit a1773cf (+ this Results commit). Code:
`crates/dditem/src/forge.rs`, `rng.rs`, `rules.rs` (Rarities, `RARITY_ITEMS`),
`itl.rs` (strength/magick accessors). Docs: `docs/SHOPS.md` sections 4, 5, 8,
9, 10. `cargo test -p dditem`: 15 synthetic + 5 install tests pass; clippy
clean for the crate.

- **Enhancement commit** (`0x006f29f0` → `0x006f3520`, `0x006f35f0`,
  `0x004612c0`): materials (bag owners from the second-to-last down, then
  storage), gold below level 4 / Rift Crystals for 5-6, then one copy of the
  stack is re-added with the new level. **Stats** are recomputed on every read
  (`0x0045b620`): per-level entries replace the base (`base*rate` or
  `base+rate`), last match wins. Iron Sword 50/66/82/98/210/560/700 checked
  against the data.
- **Dragonforging** (`0x0045cb30`, level 4): rolled on every enemy death
  (`uEnemy::vf72` `0x00aab3b0`) for the Arisen's 9 equipped items below level
  4, chance by dragon kind and the item's level (`0x0045ce50`, tables
  `0x01539d34..`): Drakes/Wyrms/Wyverns 0/2/6/18, Dire 5/15/25/40, Cursed
  Dragon 40/55/65/75, Ur-Dragon 100 (or 0/2/10/30 under two unnamed flags);
  The Dragon via a scenario command (`0x0078fbb0`, 100%).
- **Special flag**: it is the Rarities shop (Fournival), set in
  `0x006eb730`; Ferrystone/Wakestone Shard half price, rarities (item tag 4
  "A rarity.", list at `0x017f1ae0`, 62 items) sell for +20%. `0x006e86c0` is
  the "cart holds equipment" byte of the confirmation box (equip prompt).
- **Augment names**: table `0x015214a0` (115 ids → abilityName index);
  0x54 = Suasion, confirmed.
- **Curse purification**: cost = buy price in RC; deterministic roll from a
  per-item seed (incremented per unit) through the game's xorshift128
  (`0x00d006c0`/`0x00d00690`, translated, emulation-checked); rows by item and
  vocation group; first running sum >= `r % total`.
- **Live**: party read through claude-4 (#633): no augments, F = 1.0. The one
  buy and one sell are **QUEUED** at claude-4 (head #699): the party is in the
  field and long teleports are not allowed; they will run when the party is
  at a shop.

**Still unknown**: bag u16 `+0x1142` (second Suasion condition), the two
Ur-Dragon flags, the dragonforge slot bonus byte `+0x15`, augment table byte
1, the dialogue that opens the level 5-6 screen, `rItemRandParam`'s consumer.
