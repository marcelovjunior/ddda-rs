# S46: Route checkpoints: start the opening from a saved milestone

- **Branch:** head (main)
- **Files:** `crates/ddview/src/opening.rs` (write checkpoints, `--opening-from`),
  `crates/ddview/src/main.rs` (flag), `crates/ddhost` (load at a stage if needed);
  tests
- **Kind:** tooling (maintainer request 2026-10-10)

## Why

Every viewer check replays the opening from Cassardis (10-20 min). Headless
installed tests already cover the systems; the viewer needs a faster entry
for per-feature GUI checks. The full route stays as the integration test.

## Design

- The route (`--perf-route`, `DDVIEW_DEMO_FIGHT`) writes `Host::save` files
  at authored milestones where the original could save (no interaction
  open): after the vocation choice, at the gate (stage 100 entry), at field
  settle, after the rescue outcome. Directory from `DDVIEW_CHECKPOINT_DIR`.
- `--opening-from <file>` boots the opening the way the original loads a
  save: restore world flags/quests/SceNo, progression, economy, bags, NPC
  quest masks; place each profile's Arisen at its saved place on the saved
  stage; layouts, NPCs and enemies then admit from their gates as usual.
  No teleports beyond the saved place, no forced flags.
- Mid-fight state (enemy HP), running quest cameras and open dialogue are
  not saved (as in the original); a checkpoint is refused while busy.

## Done when

`--opening-from <field checkpoint>` reaches the rescue outcome in a few
minutes with the same result as the full route, and a test round-trips a
route checkpoint through save/load.
