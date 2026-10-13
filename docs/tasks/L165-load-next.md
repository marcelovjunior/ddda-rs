# L165: next loader cost and autoplay cadence

Owner: codex-6. Branch `task/L165-load-next`, worktree L165.
Assigned by head: merge L163; rank combo32 save-load and boot intervals,
then attack the next measured exact cost on decode threads 1028/102C/1030
and backend texture creation/upload. Separately propose prompt Enter
input in `ddplay/src/input.rs` for head review. No game or shard runs.

## Autoplay proposal

The previous loop waited 1,500 ms and two 80 ms key phases, so its input
attempt period was approximately 1,660 ms. The proposal retains 80 ms
down/up phases but separates them from the stage probe cadence. It attempts
Enter every approximately 160 ms when frames are arriving, beginning after
the first new Present. A new Present is required between edges to avoid
collapsing them into one host-input drain during a stalled frame. This is
a host scheduling safeguard, not proof that the guest consumed a key.

Only normal `Event::Key` events are emitted. Original screen, animation,
storage and input acceptance gates decide what advances. The change does
not detect exact acceptance instants: latency also includes host scheduling,
queue consumption and guest frames. 80 ms is the existing host automation
setting, not a recovered game constant. A head run must verify that faster
attempts advance the intended Continue/Load route without extra selections.

New presses pause immediately when the latest sampled frames average at
least 1,000 draws/frame. The existing two-probe gameplay confirmation remains;
an outstanding key is released even when gameplay is detected. The draw
threshold remains a heuristic, not an input-readiness or gameplay proof.
Probe FPS uses actual elapsed time instead of assuming exactly 1,500 ms.

## Loader/backend source review

- Read-only Ghidra `00DB9F40` shows a leave/reenter-critical-section loop
  when the current queue entry has not passed producer frontier `+151C4`.
  Its inclusive ownership is not exclusively decoding. New profile stacks
  must separate this polling from virtual resource decode work before a
  scheduling change is justified.
- `00E6B1C0` is the texture resource reader and calls buffered skip
  `00CFEEC0` before selected image/mip reads. L163's exact skip opt-in
  already targets that loop; its combo32 effect is still to be measured.
- `d3d9draw::upload_reads` records producer shadow copy/enqueue duration.
  `ddgpu::create_texture` lazily creates the GPU texture and drains pending
  uploads. `write_image` performs layout/conversion, `wait_finisher`, and
  `queue.write_texture`. Those backend costs are outside
  `texture_enqueue_ns`; its value cannot establish backend upload cost.

## Backend texture attribution

Under existing `DDRUNTIME_LOAD_LOG=1`, the ddplay snapshot provider adds
nine `gpu_texture_*` fields to transition intervals:

| Field suffix | Meaning |
|---|---|
| `creates`, `create_failures`, `create_ns` | Device creation attempts, failed validation scopes, host duration including scope push/pop; excludes texture cache hits, map insertion and pending writes |
| `writes`, `bytes` | Completed `queue.write_texture` calls and prepared data slice lengths, including any retained row padding |
| `prepare_ns` | Image layout and conversion before finisher wait |
| `wait_ns` | `wait_finisher` host duration |
| `queue_ns` | `queue.write_texture` host duration |
| `write_ns` | Inclusive write duration spanning the preceding three phases |

These are host wall durations, not GPU execution or transfer-completion
times. Pending images count only when actually written. Unsupported images
that return before writing are not counted. Writes completing across a
transition are assigned to the completion interval; atomic snapshots are
approximate across concurrent operations. Do not sum total write duration
with its phases, or these durations with thread CPU/producer enqueue time.
Absent providers omit these fields (older frontends remain compatible).
No per-image logging, waits or backend behavior changes are added.

## Results

- L163 merged as `5c26cbb0`; additive MILESTONES/RUNTIME conflicts retain
  both lanes' findings.
- Real ddplay frontend compiled through a private manifest with runtime
  default features disabled: autoplay key-edge regression **1 passed**.
  No window/game code executed. Initial runtime library: **99 passed,
  1 render-tail timing failure, 5 ignored**. Complete light rerun:
  **249 reported passes, 1 Present timing failure, 7 ignored**; render-tail
  passed that run. Final serial library with load/boot diagnostics enabled:
  **100 passed, 0 failed, 5 ignored**, including both timing tests.
- Backend phase partition CPU regression: **1 passed**. Final focused
  load-log suite with the new snapshot provider covered by the MXCSR test:
  **10 passed**. Final real frontend no-shard compile and autoplay test:
  **1 passed**, including the new provider hookup. No GPU tests were run
  for this telemetry-only renderer change; no image/performance claim.
- Test logs are private under `C:/ddda-targets/L165/`: `light-complete.log`,
  `final-lib.log`, `final-load.log`, `texture-cpu.log`, `frontend-final.log`.
  All Cargo commands used four jobs and disabled incremental compilation.
- Autoplay route, image and boot-time acceptance remain with head.
- Combo32 files were not available at initial inspection. No new ranking,
  loading speedup or game acceptance is claimed without those captures.

Next action: head reviews isolated autoplay commit `f6a2d2af`; consume
head's combo32 ARCHIVE_SKIP=0/1, BOOT_DETAIL and load-window raw stacks.
