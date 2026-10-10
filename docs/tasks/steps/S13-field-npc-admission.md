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
