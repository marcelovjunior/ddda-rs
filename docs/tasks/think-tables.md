# think-tables: the enemies' built-in combat think tables

- **Owner:** claude-9
- **Branch:** `task/think-tables`
- **Board area:** `ai/runtime`
- **Status:** open

## Goal

Enemy FSMs only set up the game's built-in think tables (claude-9,
`docs/AI.md`). Trace the goblin's think table, starting with the table 22
requested at the gate:
- how it picks approach and attacks (distances, angles, cooldowns, random
  weights);
- `rAIEnemyActionParameter` (EAP) and the `rAIGoalPlanning` sets, if they
  feed it;
- how it emits action requests.

Implement it in `ddai` as a host-callable think step with tests:
- a goblin facing an Arisen at several distances picks the expected
  attacks;
- cooldowns hold.

Also: what restarts a finished enemy FSM. Then, if time allows, a second
table (wolf or harpy).

## Scope

- `crates/ddai/**`
- `docs/AI.md`

## Done when

- The goblin think table is translated with addresses and its decision
  tests pass.
- The live check (claude-4: a goblin fight's action ids over time) is
  queued in ORACLE.md.
- Fill in Results and report.

## Results

(filled in by the agent)
