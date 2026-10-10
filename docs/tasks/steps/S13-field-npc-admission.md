# S13: Admit field NPCs from the loaded layout cells

- **Branch:** `step/s13-field-npcs` (after S09)
- **Files:** `crates/ddview/src/opening.rs` (NpcAdapter,
  `sync_npc_intels`), `crates/ddhost/src/npc_placements.rs` if the reader
  needs a per-cell entry point
- **Kind:** one feature

## Context

After S09 the adapter keeps layouts per stage, but only Cassardis (stage
200) has any, so every NPC is retired once the Arisen walks onto the field
(stage 100). The route to the goblin scene (q0002) and the Encampment
needs field NPCs.

codex-35's notes (docs/tasks/road-to-encampment.md, "Stand-down handoff"):
generalise the installed layout reader and the canonical lifetime/pool
admission to completed stage/cell loads; never infer model readiness or
Guard/Goto completion from elapsed time. Sources: NPC common placement
gate 0x0075FAA0, materialization 0x00762000, pool 0x004A76A0; field
collision residency 0x00C5F630 / 0x00C5F680.

## Do

Build stage 100's `LoadedLayouts` from `Streamer::loaded_layout_cells()`
as cells finish loading (and drop cells that unload), then run the same
candidate/admission path as Cassardis. Keep the 200-slot pool limit check.

## Done when

An `--opening` route run past the Cassardis gate keeps the NPCs the
field's loaded cells authorise (log the admitted list), ddview tests pass,
and a test covers a cell loading and unloading.

## Result (codex-1: implementation checkpoint, route gate open)

Implemented in **7f52d97**, then integrated the current main/S16 API in
**26cb9af**. The adapter now rebuilds stage100 NPC layouts from the
streamer's completed cells, normalizes duplicate cell names, drops unloaded
cells, and uses the existing candidate gates and canonical 200-slot pool.
Old-stage intel owners retire before new-stage admission so reused NPC IDs
cannot retain the previous stage's owner. No Guard/Goto completion or model
readiness is inferred from a parsed archive.

The installed field also has authored `(0,0)` global NPC layouts, separate
from its split grid. Source **0x004a21b0**, NPC loop
**0x004a277d..0x004a27cb**, scans both cell words for zero and calls
**0x004a37a0(type=3)**; that helper allocates `cLayoutSetNpc` via
**0x00761ca0**. These layouts remain admitted until stage unload. Without
this bootstrap, the real arrival cell `(64,54)` omits global NPC304.
The viewer therefore includes global00 after the streamer enters stage100,
while retaining separate physical actor readiness requirements.

Validation:

- Final `cargo test --release -p ddview` against the exclusive shared M5
  target: **56 passed, 2 existing ignored**, exit0, 9.09s.
- Final `cargo build --release -p ddview`: exit0, 37.29s.
- Installed admission regression derives the arrival cell from stage100
  start-position2 and the actual field grid. With explicit authored-gate
  fixture observations, it admits **NPC304/group4/record0** and
  **NPC827/group250/record2**. Duplicate cells preserve the same owner;
  split-cell unload retires827 while retaining304; stage exit retires304;
  reentry creates a fresh lifetime. Both player seats remain present.
- Ordinary `--opening --perf-route --players 2 --unattended` ran under the
  required **60-second watchdog**. It reached the RoomExit step, Sce1100,
  after the weapon table; it did not reach the Cassardis gate. The watchdog
  stopped the private viewer. Logs are private at
  `E:/ddda-re/notes/steps/s13/route-60s.{out,err}.txt`.

The original Done route gate is **open**. The admission test supplies
fixture story state and establishes ownership behavior, not completed
opening progression or physical NPC model readiness. Board question
**3386** requests a route timeout override/checkpoint from head. No game
install/process writes or captures were committed. Next: authorize enough
time for the existing ordinary route to cross the gate, then check its
logged completed-cell admission list and unload behavior.
