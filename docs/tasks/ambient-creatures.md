# ambient-creatures: Cassardis's ambient creatures (uEm8300, uEm8602)

- **Owner:** codex lane
- **Branch:** `task/ambient-creatures`
- **Board area:** `fight/ambient`
- **Status:** open

## Goal

The doors runtime (codex-32) needs the enemy manager's real population:
fresh Cassardis GPL groups 1/3 admit two uEm8300 (around (404, 0, -1885)
and (301, 0, -2025)) and seventeen uEm8602 on the beach. The door's
auto-close occupancy test excludes kind 0x6B (uEm8600) but not 0x65
(uEm8300). docs/FRAME.md lists uEm8300 in the Obj move line; an oracle
session saw a static uEm8300 15 m from the party (docs/ORACLE.md).

Identify what these creatures are (models, motions, AI: uEm8300's think
interpreter is the shared one, docs/AI.md), spawn them from the GPL like
other enemies (ddspawn / ddenemyrt), run their behaviour (idle, wander,
flee, despawn) as the original does, and expose their positions to the
host so door occupancy and anything else reading the enemy manager see
them. Build it through the generic enemy runtime, not as a door special
case.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks follow the safety rules in `docs/CODEX.md` (the original is in
Cassardis right now).

## Scope

`crates/ddenemyrt`, `crates/ddspawn`, `crates/ddhost` enemy feed, the
viewer's enemy drawing, `docs/ENEMIES.md`, this brief's Results.

## Done when

In a fresh opening, Cassardis's ambient creatures appear where and as the
original shows them (oracle check), and the door occupancy reads their
positions. Fill in Results and report to `head`.

## Results

(filled in by the agent)
