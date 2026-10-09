# story-order: ddstory in the traced frame order

- **Owner:** claude-7
- **Branch:** `task/story-order`
- **Board area:** `story/runtime`
- **Status:** open

## Goal

Reorder `Story::tick` to the order traced in `docs/FRAME.md`:
1. quest manager and QCT;
2. the stage mode (events);
3. scenarios (trigger volumes);
4. scripts on line 0.

Rerun the opening ladder test. Add a read-only `Script::state_path()` (or
similar) that `ddhost` uses in its replay hash. This is claude-7's #970
proposal.

## Scope

- `crates/ddstory/**`
- `docs/QUESTS.md`

## Done when

- The opening test passes in the new order. Explain any change in the
  ladder.
- The accessor has a test.
- Fill in Results and report.

## Results

(filled in by the agent)
