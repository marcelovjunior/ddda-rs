# skill-catalogue: skills and augments read from the installed exe

- **Owner:** claude lane
- **Branch:** `task/skill-catalogue`
- **Board area:** `research/vocations`
- **Status:** open

## Goal

Proposal 2 in `docs/VOCATIONS.md`: a complete catalogue of skills and
augments in `ddskill`. It joins three sources:
- `etc\skill\skill` (costs and ranks);
- the exe's skill table (0x014f3658: vocation and weapon masks, action
  ids);
- the name tables (0x01520278, 0x015214a0) with the game's text.

Following the constants policy in AGENTS.md, the exe tables are read **at
run time from the player's installed DDDA.exe**, like `ddquest::events`
(PE timestamp check, VA-to-file mapping). They are never committed as
constants.

Also map the action ids to the `cPlActWpn*` classes and their motions,
where the exe allows it. claude-8 asked for this.

## Scope

- `crates/ddskill/**`
- `docs/VOCATIONS.md`

## Done when

- Every skill and augment resolves to its name (rebuilt from game text),
  vocations, weapon, cost, rank and action id.
- Install tests skip without the game.
- Fill in Results and report.

## Results

(filled in by the agent)
