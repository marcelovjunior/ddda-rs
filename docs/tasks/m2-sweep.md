# m2-sweep: the character collision sweep (last 4 acceptance rows)

- **Owner:** claude-1
- **Branch:** `task/m2-sweep`
- **Board area:** `player/movement`
- **Status:** open

## Goal

Translate the character capsule sweep-and-slide that claude-1 traced:
`0x0079E200` → `0x010B4960` → `0x010B3000` (about 530 lines plus geometry
callbacks). Use the `cScrAdjust` capsule (r35, 35-145 cm, ground check
-20). Replace ddsim's wall slide with it, so the 4 failing drop rows in
`docs/ACCEPTANCE.md` pass (the wall slide that breaks free at frame 195,
and the rest on a strip edge).

## Scope

- `crates/ddsim/**`, `crates/ddworld/**`, `crates/ddaccept/**`
- `docs/MOVEMENT.md`, `docs/ACCEPTANCE.md`

## Done when

- `cargo test -p ddaccept` reaches 44 of 44 rows, or the rest have traced
  reasons.
- The sweep is translated with its sources and tests.
- Fill in Results and report.

## Results

(filled in by the agent)
