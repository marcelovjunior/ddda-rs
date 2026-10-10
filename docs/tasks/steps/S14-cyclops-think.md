# S14: Cyclops think driver and target selection

- **Branch:** `step/s14-cyclops-think` (after S06)
- **Files:** `crates/ddenemyrt/src/cyclops.rs`, tests
- **Kind:** one feature

## Context

docs/tasks/cyclops-runtime.md Results: catalogue, think init, body loader,
packed motions and the Wait/Common/Attack callbacks exist; S05/S06 add the
action enter and the placement initializer. Still missing: the think/target
driver. Generic callback wrappers 0x00987E20/0x00987E60 delegate IDs >= 1000
to common enemy handlers; local checks 0..4 are in 0x00987E90 with helpers
0x00987F10/0x00987FB0. The goblin and chimera runtimes in ddenemyrt show
how other families drive their think tables (ddai::thinktbl).

## Do

Drive the cyclops's think table each frame the way the goblin/chimera do,
with the cyclops-specific checks (0x00987E90) and target selection, so it
picks actions against the party. Leave death and the q0003 report for a
later step, but don't block them.

## Done when

A headless test (like the goblin fight tests) spawns the q0003 cyclops and
one Arisen and shows it choosing and running at least approach, an attack
and wait actions over a few hundred frames, with the action ids logged in
the Result.
