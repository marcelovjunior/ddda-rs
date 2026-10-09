# economy-core: Wallets, shop sessions, stock and enhancement as a runtime

- **Owner:** claude-6
- **Branch:** `task/economy-core`
- **Board area:** `economy/core`
- **Status:** open

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

(filled in by the agent)
