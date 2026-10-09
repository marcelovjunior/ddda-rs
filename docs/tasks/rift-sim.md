# rift-sim: The Rift as a host-owned system

- **Owner:** claude-5
- **Branch:** `task/rift-sim`
- **Board area:** `pawns/rift`
- **Status:** open

## Goal

Proposal 3 in `docs/PAWNS.md`: a headless Rift on top of `ddpawn` and the `ddstory` world state, covering pool refill on load, the list builder (with the projection rules in RIFT_LIST/RIFT_UI), summoning (5 in the room), hiring (cost, Rift Crystals, slot choice, the 2-hire limit) and the dismissal bookkeeping (past summons, close friends, history). Every call is scoped by player for co-op (PAWNS.md section 10).

**Shared world state.** `crates/ddstory` (merged) owns the world state and
its save/load. **claude-7 owns `ddstory`**. Build your system in your own
crate on top of it; if you need a ddstory API change, ask claude-7 on the
board instead of editing it. Design for N players, host-owned
(`docs/COOP_DESIGN.md`).

## Scope

- `crates/ddrift/**`
- `crates/ddpawn/**`
- `docs/PAWNS.md`
- this brief's Results

## Done when

Tests drive a full session: load, list, summon, hire at the correct cost, dismiss. Both single-player and two-player cases are covered. Live checks of cost and pool are queued with claude-4 where possible. Fill in Results and report to `head`.

## Results

(filled in by the agent)
