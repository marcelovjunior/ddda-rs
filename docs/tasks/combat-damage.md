# combat-damage: damage rules as pure functions, and hit-volume activation

- **Owner:** claude lane
- **Branch:** `task/combat-damage`
- **Board area:** `research/combat`
- **Status:** open

## Goal

Proposals 1 and 2 in `docs/COMBAT.md`.

1. **combat-damage:** headless, pure, N-player-safe functions in
   `ddcombat` for:
   - attacker value (players and pawns `0x0088AA20`, enemies
     `0x0088B530`, with the critical roll);
   - enemy defence (`0x00AAA490`);
   - player defence (`0x00B7AD90` / `0x00B83EE0`);
   - region pools, regeneration and weak-point classes (`0x00770910`,
     `0x00776F50`, `0x00777110`, `0x007781A0`);
   - the per-frame hit merge.

   Each one is translated with its source comment, and the worked example
   (Iron Sword on a goblin: 105 HP) is the regression test.
2. **combat-hit-volumes, trace part:** how motion frames switch hit groups
   on (the motion event side, `mRelation`). Make it implementable, so the
   placement of volumes on joints can follow.

## Scope

- `crates/ddcombat/**`
- `docs/COMBAT.md`

## Done when

- The functions and tests are in place.
- The hit-group activation is traced with addresses, or the gap is named.
- The live checks (goblin hit, head hit, magick hit) stay queued with
  claude-4: list them in ORACLE.md's open checks.
- Fill in Results and report.

## Results

(filled in by the agent)
