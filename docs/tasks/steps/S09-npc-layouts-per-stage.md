# S09: Keep a layout set per stage in NpcAdapter

- **Branch:** `step/s09-npc-stage-layouts`
- **Files:** `crates/ddview/src/opening.rs` (`NpcAdapter` and
  `Session::sync_npc_intels` only)
- **Kind:** one refactor, no new behaviour

## Facts

- `NpcAdapter` (opening.rs, around line 141) holds one
  `ddhost::npc_placements::LoadedLayouts`, built for Cassardis by
  `NpcAdapter::cassardis` (stage 200, all cells).
- `sync_npc_intels` (around line 324) retires every NPC when the stage is
  not 200 and warns "waits for loaded-layout/archive readiness adapter".
- `Streamer::loaded_layout_cells()` (stream.rs:151) gives the field cells
  whose layouts are loaded. It is not used by this step.

## Do

1. Change `layouts` into `HashMap<u32, LoadedLayouts>`, keyed by stage.
   `NpcAdapter::cassardis` inserts stage 200 as today.
2. In `sync_npc_intels`, and in the door coverage check that reads
   `npc_adapter.layouts` (search for it), look up the current stage in
   that map. If it is missing, keep today's behaviour (warn once, retire,
   return).
3. Everything else stays as it is: same candidates, same admission.
4. Existing ddview tests must pass unchanged.

This prepares the next step, which head will write: building stage 100's
layouts from `loaded_layout_cells()`.

## Done when

`cargo test --release -p ddview` passes, the release build passes, and
the Result shows the diff stat.
