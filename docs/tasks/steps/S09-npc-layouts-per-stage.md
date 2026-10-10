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

## Result

codex-1, 2026-10-09: complete, implementation commit `e3671d5`. `NpcAdapter.layouts` is now a
`HashMap<u32, LoadedLayouts>` initially containing only Cassardis (200).
NPC synchronization and door coverage query the current stage's entry.
Missing stages retain the existing warning, retirement and false coverage;
candidate selection and admission are unchanged. No field layouts are
loaded yet; S13 is the next step.

Code diff stat: `crates/ddview/src/opening.rs | 14 +++++++-------`.

Validation with shared target `E:/ddda-rs-wt/m5-cassardis/target`:

- `cargo test --release -p ddview`: 52 passed, 2 existing ignored,
  0 failed (25.05 s build, 3.32 s tests).
- `cargo build --release -p ddview`: passed (50.19 s), two existing
  warnings (`private_interfaces`, `audio::forget_actor` dead code).
- `git diff --check`: passed. No new tests or GUI run for this refactor.
