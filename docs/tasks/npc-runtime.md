# npc-runtime: NPC schedules, talking and the interaction lock

- **Owner:** claude-6
- **Branch:** `task/npc-runtime`
- **Board area:** `world/npc-runtime`
- **Status:** open

## Goal

A host-owned NPC runtime in a new crate `ddnpcrt`, on `ddnpc` (merged: ledger, MSS/MSL, the Quina chain), `ddstory` (flags, SceNo, clock) and `mtfsm`:
- schedule selection per stage, SceNo and hour (codex-7 traced `0x00bbdaa0` and the GameHour ranges);
- where each NPC stands now;
- talking: picking a conversation by flags and quest state, the lines (`mtgmd`), the results (flags, quests, opening a shop session in `ddeconomy`);
- the co-op interaction lock: one player per NPC; the other player sees the NPC as busy (`docs/COOP_DESIGN.md`).
The open question from claude-7, how a script's `Message` id picks its line, belongs here.

**Shared world state.** `crates/ddstory` is owned by claude-7; request API
changes on the board. The game host is being designed by claude-10
(`ddhost`, `docs/adr/0002-game-host.md`): keep your crate's interface as
inputs → state → requests, so the host can call it each frame. Design for
N players.

## Scope

- `crates/ddnpcrt/**`
- `docs/NPCS.md`
- this brief's Results

## Done when

Tests: Quina's position across a day; talking to one opening NPC picks the right conversation for the flag state and applies its results; a second player is locked out while the first talks; a shopkeeper's talk opens the `ddeconomy` session. Fill in Results and report to `head`.

## Results

(filled in by the agent)
