# combat-status: what a status effect does once it fires

- **Owner:** claude-8
- **Branch:** `task/combat-status`
- **Board area:** `research/combat`
- **Status:** open

## Goal

Finish proposal 3 in `docs/COMBAT.md`. The build-up is merged
(`ddcombat::status`). Now, for each of the 20 status types:
- its `rStatusParam` entry;
- its duration and how it ticks (per 1/30-s frame times the unit's
  `mDeltaTime`);
- its effect (damage over time, stat multipliers for the four "down"
  statuses, movement or action locks, special ones such as petrify, sleep
  and possession);
- its cure: items, time, rules.

Also how players and pawns build up statuses (the player side of
`0x007772B0`), the wall check (`0x00779270`), and the team IDs behind the
friendly-fire matrix. Item names for cures come from `ddeconomy` /
`dditem`.

## Scope

- `crates/ddcombat/**`
- `docs/COMBAT.md`
- `docs/STATUS.md` (new, optional)

## Done when

- Each status has its duration, tick and effect with addresses and tests,
  or a named gap.
- The live checks stay queued with claude-4.
- Fill in Results and report.

## Results

(filled in by the agent)
