# L155: save-load critical path

Owner: codex-6. Branch: `task/L155-saveload`, from `task/fps-line`.
Assigned by head in the maintainer prompt, 2026-10-13. L151 merged as
`10afc815`, retaining both L148 and L151 documentation additions.

Scope: repair the ddgpu example build separately; map Continue through
gameplay using read-only original-code evidence; rank head's combo25
timeline and select an exact, opt-in improvement. No game or shard runs.

## Example repair

`module_cost` is configured at `src/module_cost.rs`, not `examples/`.
It compiles `shader_cache.rs` as an example-local module; that module now
uses `crate::pipeline_log`. Declare the existing `pipeline_log` module in
the example root too. No renderer behavior changes.
Committed separately as `5da79cfb`. All ddgpu examples build successfully
in 3m14s with four jobs, incremental off, fresh `C:/ddda-targets/L155`.

## Source map

Addresses refer to DDDA.exe 2017-12-13 Steam. Inspected through read-only
Ghidra HTTP on 2026-10-13; no translated game behavior is introduced here.
Archive names below are path families, not a measured per-save request log.
Runtime tables, saved stage, party, quest and world-cell state select the
actual list. Earlier background: `GAMEPLAY_PATH.md`, `TITLE_PATH.md`.
Supplementary title entries not recognized as functions by Ghidra were
cross-checked in the existing original-instruction lifts (no shard edits).

| Order / dependency | Original boundary | Work and execution context |
|---|---|---|
| Historical title load callback | `00532110`, dispatched by `005317B0` | Substate 2 issues storage kind 9 only when storage `+648` is idle. Substate 9 waits for that byte to clear; substate 8 installs `005330E0`. **PC dispatcher `004B7870` does not handle kind 9**, so this callback is not established as the actual PC Continue route. L149's label is callback identity only. |
| PC menu / handover callbacks | `00531A20`, `00532DC0`, `00532AE0`, `00533030` | Menu results 1/0x13 set title `+4C=2` and enter `00532DC0`; that route selects `00532AE0`, starts/polls pawn-manager preparation (`0047CC00`), then `00533030` and `00531E60`. Other results have different routes. Confirm the displayed choice against head's observed input; do not equate all title callbacks with save reads. |
| Save read / parse | `004B7870`, `004B8280`, `004B7CA0`; `004999C0` | Supported storage commands run the PC state machine, including `DDDA.sav` FileRead and save-record application (`00496410`). Kind 0x12 explicitly applies loaded records in `004B8280`; kind 9 is absent. Save stream inflate uses `00410180`. These are distinct from ARC I/O and may occur before the eventual menu choice; no evidence here that resource decode workers parse saves. |
| Area handover | `005330E0` -> `00DB4C10`; `00DB4F10` | Request records target DTI, then area update changes the stack. `aGameMain` DTI is `0198CDF0`; parent area initialization precedes child readiness. A request marker is not completion. |
| Per-area shader gate | `00533750` -> `00DA63F0` | Countdown first; then `sa\\DX9\\<area>.arc` requested with flag 2, polled for ready/error. If `00419ED0` selects append handling, next request is `sa_append\\DX9\\<area>_append.arc`, also polled, then `004AB440`. Main area initialization cannot proceed through this gate early. |
| Parent common resources | `004FA5C0` | After shader gate: `system\\TexDetail\\TexDetail` with flag 1; queue `rom\\bbsrpg_core` (24146), wait through `004011A0`; later queue `rom\\bbs_rpg` (1), wait again. Existing residents may avoid fresh I/O. |
| GameMain initialization | `004FB050` | Shader gate and party/setup readiness precede `rom\\dl1\\game_dl1` (24148), submitted by `00418810`/`00418610(2)`, followed by readiness gates. The older `rom\\game_main` id 3 association remains an inference: its requester is not established by this trace. |
| Saved stage request | `004FB7B0` -> `004FBD50` | Once GameMain is initialized, save/new-game flags select setup; stage area is requested by its formatted name. No hardcoded stage assumption for Continue. |
| Stage base request list | `004FFBA0`, case 3 | After shader and party cleanup gates, optional GUI timer (33795), base stage (`index+5`), conditional variants/enemies, object archive (`index+0x8448`), then nav archive (`index+0x8407`), further stage/character-specific dependencies. This corrects the earlier unordered nav/om listing: the code requests om before nav. |
| Stage cells / party | `004FFBA0`, cases 4..16 | Wait for base/nav group; conditional world-cell and split ARC loops enqueue lists, followed by completion gates; party restoration (`004A18F0`/`004A1D80`) and further resource preparation follow. Cell/weapon/NPC/event filenames are data-dependent; this is not a fixed manifest. |
| Post-load activation | `00504330` -> `0050ADC0` | Initialization substates include original countdowns, unit/GUI/readiness checks and fades. Case E waits on `0043F620`, calls `0050AD10`, then writes mode 2. Next mode-2 update dispatches `0050B870`; L149 first gameplay Present is a producer boundary, not confirmed player control/display completion. |

`00531E60` goes directly to `005330E0` when save `+20E77B` bit 3 is
set; otherwise it completes `00531EF0`, then selects `00532110` only when
`00DE2E70` returns 4. Other return values go to `005330E0`. The original
entry lift explicitly proves the bypass, even without assuming which input
device/status returns 4. In pre-menu `0052FC30`, supported storage kind 0x12
can be issued; whole-save file work need not begin at the later menu choice.
Head's button-time correlation and recorded callback chain must decide the
measured start. Raw state IDs remain available in existing L149 intervals.

### Resource scheduling and serialization

1. `sArchiveManager::vf06` (`00417B50`) selects load groups. In the normal
   branch, `004181C0` iterates one archive request at a time and polls its
   resource status before advancing. Its alternate branch (game flag or no
   active area condition) queues the group's requests before polling the
   group. `00418610` can set a five-update delay; preserve original delays.
2. `sResource::ctor` (`00DBB690`) creates one `00DBA9A0` resource worker
   and 1..3 `00DB9F40` decompression workers, selected from the original
   system processor count. These are separate from DBDEF0/DBDF10 gameplay
   job workers. L149's `cpu_loader_io_ns` combines **both** resource worker
   types; it cannot establish disk saturation on its own.
3. `00DBA9A0` takes the first queued resource, shifts the FIFO under its
   critical section, opens its stream, calls resource vf+28 (load/parse),
   then vf+24 (finalize), and marks status. Thus queued outer archive loads
   on this worker remain serial, even when multiple requests are queued.
4. `rArchive::vf10` (`00E0D940`) has both serial and worker-fed paths.
   The worker path calls `00DB9C30` (reset shared stream/worklist),
   `00DBC310` per entry (make descriptor/resource), then `00DB9C80`.
   Ghidra's C output wrongly removes much of this function; disassembly
   confirms calls at `00E0DB62`, `00E0DC10`, `00E0DC58`. Do not use the
   truncated C output to claim archives have no entry decode path.
5. `00DB9C80` starts the decode workers, submits stream reads in 128 KiB
   chunks, waits for each submitted read, advances the shared ring's
   published byte count, and keeps feeding until completion or failure.
   Full/blocked ring uses `Sleep(1)`. It tracks a contiguous completion
   frontier, waits for **all** worker completion events, then runs vf+24
   finalizers and marks loaded resources **in descriptor order**.
6. `00DB9F40` claims available descriptors under the shared lock. Each
   worker has its own zlib state, calls the entry's vf+28 using
   `sResource::DecompressStream`, and records completion. `00DB92C0`
   consumes up to 128 KiB input slices, waiting with `Sleep(1)` for bytes
   not yet published, and invokes `00CF02E0`. Decode and resource parsing
   are interleaved by stream reads, not a global decode-all/build-all pass.

Consequently I/O already overlaps entry decode, while archive batches and
ordered resource finalization form barriers. Parallelizing arbitrary outer
archives would race the one shared `sResource` workspace and alter resource
lookup/finalization ordering unless redesigned and proven. More workers or
shortened sleeps are not established exact optimizations by this evidence.

### Rendering work after resources

Shader-object creation does not fully specify a pipeline. Exact target
formats, layout, paired shaders and state variants become known at draw
derivation on `d3d9-backend`. Cold `WgpuBackend::derive` misses create shader
modules/pipelines there, potentially filling the producer FIFO. L151's
opt-in creators prepare previously recorded exact variants from backend
startup; shader-created hints prioritize them. Draws retain the original
creation path on absent/in-flight/failed/mismatched recipes. See L151 for
seven CPU, 22 transport/capture and six image-test receipts and controls.

## Results

Implemented/tested: separate example build repair `5da79cfb`; all examples
build. Source scheduling map complete at the static-evidence level above.
The merged L151 light build compiled in 1m06s; tests are held by head's
combo25 pause, not reported as passed. Combo25
`run_c25z0.log`/`run_c25z1.log` not yet present at the initial inspection.
No save-specific ordered manifest, interval ranking, end-to-end gain or
original-game image comparison claimed yet. Next: rank completed head logs
before selecting the next exact intervention.
