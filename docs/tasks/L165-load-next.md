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

## Results

- L163 merged as `5c26cbb0`; additive MILESTONES/RUNTIME conflicts retain
  both lanes' findings.
- Real ddplay frontend compiled through a private manifest with runtime
  default features disabled: autoplay key-edge regression **1 passed**.
  No window/game code executed. Initial runtime library: **99 passed,
  1 render-tail timing failure, 5 ignored**; complete light rerun pending.
- Autoplay route, image and boot-time acceptance remain with head.
- Combo32 files were not available at initial inspection. No new ranking,
  loading speedup or game acceptance is claimed without those captures.

Next action: validate and hand off the isolated autoplay proposal; consume
head's combo32 ARCHIVE_SKIP=0/1, BOOT_DETAIL and load-window raw stacks.
