# economy-core: Wallets, shop sessions, stock and enhancement as a runtime

- **Owner:** claude-6
- **Branch:** `task/economy-core`
- **Board area:** `economy/core`
- **Status:** review

## Goal

Proposal 1 in `docs/SHOPS.md`: per-player gold and Rift Crystals, inventories and storage (capacities from SHOPS.md), shop sessions (open conditions, stock with the AND/OR rules, buy and sell through the traced price functions, sell-back raising stock), daily restock on day change from `ddstory`'s clock, and the enhancement offers and commit (`dditem::forge`). The shop is a host-owned session with an interaction lock: one player at a time per NPC (COOP_DESIGN.md).

**Shared world state.** `crates/ddstory` (merged) owns the world state and
its save/load. **claude-7 owns `ddstory`**. Build your system in your own
crate on top of it; if you need a ddstory API change, ask claude-7 on the
board instead of editing it. Design for N players, host-owned
(`docs/COOP_DESIGN.md`).

## Scope

- `crates/ddeconomy/**`
- `crates/dditem/**`
- `docs/SHOPS.md`
- this brief's Results

## Done when

Tests drive buy, sell, restock and enhance sessions for one and for two players against the traced rules. The queued live buy/sell check is still listed in ORACLE.md. Fill in Results and report to `head`.

## Results

Branch `task/economy-core`. New crate `crates/ddeconomy` (wallet, inventory,
stock, sessions) on `dditem` and `ddstory::World`; `dditem` gains
`buyback_eligible` (`0x0045f040`) and `paired_item` (`0x004607a4`). Docs:
`docs/SHOPS.md` section 11. Tests: `cargo test -p ddeconomy` 7 synthetic
session tests (one and two players: buy, sell, buy-back, Rarities, restock
from `World::day`, NPC lock, enhance, bag eviction/overflow) + 1 install test
(Caxton buy with and without a Gold Idol, idol sale, Iron Sword enhance,
Madeleine 5-day restock); `cargo test -p dditem` 16 + 5. Clippy clean for both
crates.

Newly traced for the runtime: cart total `unit * count` (`0x005ea940`),
payment at confirmation (`0x006e3345`), buy-back eligibility (`0x0045f040`),
paired items for ownership checks (`0x004607a4`), has-item owner sets
(`0x00460350` modes 0/1), bag add rules (`0x0045e680`: 999 cap, level only on
equipment kinds, full-bag eviction order `0x0045dcc0`, redirect to main pawn
or storage, full storage to buy-back).

Co-op decisions (documented in SHOPS.md s11): wallet, bag and storage per
Arisen; shared main-pawn bag; stock, buy-back list and gates are world state;
NPC lock per session; a player cannot use another Arisen's bag.

Not compared live: the queued buy/sell check (ORACLE.md "Open live checks",
claude-6 #627/#694). Not modelled: per-instance item data, pickup-use items,
the arrival flag, the cap on a growing stack (guessed 999).
