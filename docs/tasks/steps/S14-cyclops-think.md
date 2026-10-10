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

## Result (codex-47: partial review checkpoint)

Implemented source primitives in **de6af7e**, with a controlled installed
attack regression in **2425f4e**. The original Done gate is **still open**:
this is a table/target adapter, not a complete spawned cyclops actor or a
few-hundred-frame approach/attack/wait encounter.

- `ThinkDriver` runs the ordinary table phase of **0x0097a6a0**, called by
  vf136 **0x00979ba0**. It measures the sensor/rage target in slot0, accepts
  explicit measured slots1..6, sets their sourced category masks, prepares
  the mode/weapon flag bits, reloads the default table, and requests a new
  action only when its result differs or manager +0xea5 allows a restart.
  Canonical family flags, forced-action arbitration, controller activity,
  point targets, and world responses remain host inputs. Timer ticking is
  the separate vf08 **0x00979420 -> 0x008b7930** phase; elapsed-frame
  accumulation/reset is the tail of **0x0097e1a0**. The q0003 weapon type2
  suppresses that elapsed accumulator; no invented cooldown was added.
- Local checks **0x00987e90** cover attachment count, attached vocation
  groups (**0x00987f10**, including overlapping groups for vocations14/18),
  elapsed frames, the +0x62bc counter, and inclusive horizontal distance to
  the separate +0x614c target (**0x00987fb0**). Wrappers **0x00987e60** and
  **0x00987e20** delegate common IDs >=1000; local set1 resets elapsed
  frames. Missing common answers produce the provider's error, not false.
- Sensor selection **0x00ab52e0** examines the original16 slots, adds enabled
  score components, rejects protected/null/disabled candidates and chooses
  the highest strictly positive score. Ties retain the earlier slot. It
  does **not** select the nearest Arisen. Object tokens are nonzero and
  supplied by the host; no global player or party index is introduced.
- Small related change outside the listed file:
  `crates/ddai/src/thinktbl/mgr.rs` adds slot category metadata and command
  **0x42**, reached at installed record **0x0172da28** before any world
  callback. It uses the valid-first, stable distance ordering of slots0..7
  (**0x008b71a0**, second ordering array at manager +0xd8b), then the category
  match in **0x008bc3b0** and existing selection **0x008bc410**. A regression
  checks distance ties, invalid slots, unmatched masks and uncategorized
  slots. Commands0x40/41 remain unimplemented.

Installed table execution with an unsupported world provider now reaches
the explicit diagnostic `cyclops world set 1002(0)` rather than failing
at the shared interpreter command. A **controlled fixture**, with mode2,
a sensor target600 cm directly ahead, disabled navigation observations and
roll0, selects **action50 `cEm5000ActAttack`**. The installed descriptor
argument is **0x100**, resolving to **bank1, slot0, 238 frames**. The test
loads that actual body motion. It neither runs that attack against a live
Arisen nor certifies stage100's navigation services are disabled.

Validation used the exclusive shared
`E:/ddda-rs-wt/item-use-review-target`:

- `cargo test -p ddai -p ddenemyrt -- --test-threads=1`: **ddai34 passed;
  ddenemyrt78 passed, 1 existing ignored**, exit0, before the additional
  elapsed-accumulator regression.
- Final `cargo test -p ddenemyrt --test cyclops_think -- --nocapture`:
  **4 passed**, exit0, including that added per-actor/weapon2 regression and
  the controlled installed attack/motion query. There are79 unique passing
  ddenemyrt tests across the full and final focused runs.
- `cargo clippy -p ddai -p ddenemyrt --no-deps`: exit0, only the two existing
  combat.rs type-complexity warnings at243/245. `git diff --check` passes.
- Root independently ran the four focused regressions before the
  exploratory query was replaced with the asserted attack fixture.

No live original comparison, viewer, install/process writes, new target
directory, bulk table copying or raw decompiler export was committed.
Private source traces are under `E:/ddda-re/notes/steps/S14/`.

Scope question **board3390** asks head whether to expand S14 or take a
follow-up. The source dependencies are concrete: setters1002/1003 are
**0x008ab3b0 / 0x008ab5b0**, dispatched through optional FieldWayPoint and
NavigationMesh services by **0x0059f8e0**. The latter setter can install
slot7 while the interpreter is running, which the current generic `Host`
trait cannot express. Common checks1016/1017 read the resulting path flags;
check1027 reads actor +0x5e2c. The approach action85 is `cEm5000ActWalkTrace`
(**0x00980440 / 0x00980620**) with separate layer1 start/loop requests and
installed selector arrays read through **0x00980760**. Turn helper
**0x007975d0** schedules a coordinate interpolation rather than changing
yaw immediately. Next: add these navigation/target-update and layered
motion adapters, then run the actual q0003 placement over several hundred
frames and record approach, attack and wait transitions.
