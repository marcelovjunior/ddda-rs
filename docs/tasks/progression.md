# progression: experience, levels, discipline, learning and vocation change

- **Owner:** claude-9
- **Branch:** `task/progression`
- **Board area:** `player/progression`
- **Status:** open

## Goal

Proposals 1 and 3 in `docs/VOCATIONS.md`, as a host-owned runtime in a
new crate `ddprogress`, on top of `ddskill` and the `ddstory` world state.
Per character (each Arisen and each main pawn):
- experience and level-ups that add the current vocation's stat row
  (`ddskill::growth`);
- discipline from experience bands;
- the party share (`0x00443470` / `0x004437d0`), including support pawns'
  Rift Crystals to their owner;
- learning skills and augments (cost and rank);
- equipping active skills and augments;
- vocation change (the gates, the first-time cost per flags 540-554, and
  what is cleared and what carries over).

**Shared world state:** `crates/ddstory` is owned by claude-7. Request API
changes there on the board.

## Scope

- `crates/ddprogress/**`
- `crates/ddskill/**`
- `docs/VOCATIONS.md`

## Done when

- Tests cover a level 1-10 run with a vocation change at 10, learning and
  equipping, and two players sharing experience with a hired pawn.
- Values come from the traced rules and the install data.
- The live checks already made (the experience curve, discipline bands)
  are regression tests.
- Fill in Results and report.

## Results

(filled in by the agent)
