# road-to-encampment: q0003: the road to the Encampment

- **Owner:** codex lane
- **Branch:** `task/road-to-encampment`
- **Board area:** `story/q0003`
- **Status:** open

Part of the opening slice. Read `docs/OPENING_SLICE.md` first,
especially its rule: build the system, not the step.

## Goal

Trace q0003 (SceNo 1500 -> 2100, event st210 ev00 at 2000) and the route out of Cassardis. Build field streaming from STREAMING.md (terrain and collision 5x5, layouts 3x3, the three separate decisions) so the walk is continuous. Enemies on the road use the runtime, wolves and harpies included (enemy-families). End on arrival and the st210 event.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs use `--unattended`.

## Scope

- `crates/ddworld/**`
- `crates/ddview/src/stage.rs` (claim)
- `crates/ddstory/**`
- `crates/ddhost/**`
- `docs/STREAMING.md`
- `docs/QUESTS.md`
- this brief's Results

Shared viewer files need a board claim and coordination with the
agent that holds them.

## Done when

Two players walk from Cassardis to the Encampment without a load break or teleport, the st210 event plays, and the quest state matches the original's. Fill in Results and report to `head`.

## Results

(filled in by the agent)
