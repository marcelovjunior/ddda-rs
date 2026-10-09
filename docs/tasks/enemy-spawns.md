# enemy-spawns: the world's own enemies by area, time and story

- **Owner:** claude-9
- **Branch:** `task/enemy-spawns`
- **Board area:** `world/enemies`
- **Status:** open

## Goal

Replace `--goblin` with the original's enemy placement. This takes over
the Codex brief `enemy-placement.md`; read it for the evidence pointers.
- Enemy-group layouts per stage cell.
- Layout-flag gating: claude-7 found the flags at `sGameSys+0xadee8`; it
  is open which group field names its flag.
- Day and night variants, respawn after days, story-progress variants.
- The spawner (`sSetManager`, which FRAME.md lists in aGameMain's
  managers).

Readers plus a headless runtime that, given the stage, cell, clock, day
and flags, yields the enemies to spawn. Then wire them into ddhost's
fight for the enemies we can run (goblins first; others as placeholders
with their ids).

## Scope

- `crates/ddspawn/**` (new)
- `crates/ddhost/**`, `crates/ddenemyrt/**`: additive
- `docs/SPAWNS.md`

## Done when

- One area near Cassardis is fully explained, with exe addresses: which
  groups, which conditions, the respawn timing.
- `ddhost` spawns that area's goblins from the data in a test.
- `--play --save` shows them, gated by the save's clock and flags.
- Fill in Results and report.

## Results

(filled in by the agent)
