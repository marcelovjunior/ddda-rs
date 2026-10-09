# encampment: Arrival at the Encampment (opening slice step 9)

- **Owner:** codex lane
- **Branch:** `task/encampment`
- **Board area:** `story/encampment`
- **Status:** open (after or alongside road-to-encampment)

## Goal

Opening slice step 9 (docs/OPENING_SLICE.md): the Arisen arrives at the
Encampment (stage 210; `st210_door` leads to stage 100 positions 1 and
3; event st210 ev00 at SceNo 2000; q0003 itself happens there,
`quest\q0003_b00`). Make the arrival playable end to end in the viewer:
the stage handoff from the field (`Streamer::enter`, door scenarios),
the Encampment's stage models, layouts, lighting and sky, its NPCs on
their schedules (ddnpcrt), the st210 ev00 event, and q0003's start
state, so the next quest step can follow.

Build the system, not the step: whatever the handoff, NPC placement or
event start needs belongs in the shared runtime (ddhost, ddnpcrt,
ddquest, stream.rs), not in Encampment-only code.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs use `--unattended`.

## Scope

`crates/ddhost/**`, `crates/ddquest/**`, `crates/ddnpcrt/**`,
`crates/ddview/src/{opening,stream}.rs`, `docs/OPENING_SLICE.md`
(step 9 row), this brief's Results. Coordinate on the board with
road-to-encampment.

## Done when

An `--opening` run can walk from the Cassardis gate to the Encampment
and see st210 ev00 play, with q0003 in its original start state (checked
against an oracle save at the same point). Fill in Results and report to
`head`.

## Results

(filled in by the agent)
