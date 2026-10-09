# m2-finish: the last M2 gaps

- **Owner:** claude-1
- **Branch:** `task/m2-finish`
- **Board area:** `player/movement`
- **Status:** open

## Goal

Close the remaining M2 items after m2-fixes (`docs/MOVEMENT.md`; 40 of
44 acceptance rows pass):

1. **The character's collision shape:** the body capsule or sphere and how
   it rests and slides on edges and walls. This fixes the 4 failing drop
   rows (the wall slide that breaks free at frame 195, and the rest on a
   strip edge). It replaces the `BODY_RADIUS` / `BODY_HEIGHT` placeholders.
2. **The landing hold** with the stick held (about 0.1 s in the original).
3. **Walk/run start-frame sync** (`0x00B80260`, claude-2's finding) and
   the two-frame delay before the 1.15 motion speed applies.
4. **Load and body class from carried weight and body** (`docs/PARAMS.md`,
   0x00B81120 / 0x00B81270), feeding stamina and motion speed. The inputs
   come from the inventory; until `ddeconomy` exists, take them as
   parameters.
5. **Exhaustion:** the tired action, if the trace is short.

Also update `docs/ACCEPTANCE.md`'s results table to the current output.

## Scope

- `crates/ddsim/**`
- `crates/ddworld/**`
- `crates/ddaccept/**`
- `crates/mtlmt/**`
- `docs/MOVEMENT.md`
- `docs/ACCEPTANCE.md`

## Done when

- 44 of 44 acceptance rows pass, or the remaining ones have a traced
  reason.
- Each item is translated with its source and tests.
- Fill in Results and report.

## Results

(filled in by the agent)
