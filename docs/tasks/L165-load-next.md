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
- Read-only `01105240` includes D3D texture/volume/cube creation, image
  locking, row copies and unlocks. Its earlier six decoder samples belong
  to guest-side texture setup, distinct from deferred backend GPU creation.
  Its individual copy/creation shares remain unresolved; no port selected.
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
- Commits: autoplay proposal `f6a2d2af` (review independently), optional
  backend attribution `cc83715a` (can be taken without the autoplay change).
- Autoplay route, image and boot-time acceptance remain with head.
- Combo32 files were not available at initial inspection. No new ranking,
  loading speedup or game acceptance is claimed without those captures.

Next action: head reviews isolated autoplay commit `f6a2d2af`; consume
head's combo32 ARCHIVE_SKIP=0/1, BOOT_DETAIL and load-window raw stacks.

### Verification log SHA-256

| Private log | SHA-256 |
|---|---|
| `light-complete.log` | `ab2cff7b9160472fe0b5ff44c8908b22663d8e42126ae68b5331a652fa000199` |
| `final-lib.log` | `a2613cfa68430e975653c5fca0f2e442002cb13e73593767e1f170b700c8e5f9` |
| `final-load.log` | `3db33abb83ecc5b6b94b39ac34449de72b482285676872d99b4eec76aa77a07e` |
| `texture-cpu.log` | `7a2702a460a62ad200bd88ace78e541e1356e958bf589068f2d788cb01cf41f4` |
| `frontend-final.log` | `c07687690cc3a4ef7baa64b5eb683548212b7063ae3213c9691a19af0ff29bf2` |

## Composed archive skip check (head #13513 / #13551)

Head initially suspected missing resources after c32play averaged 3,397
draws/frame versus roughly 5,700 in c31. Head then confirmed the user had
moved through different scene/camera views during c32play (#13551), so the
draw-count difference is not a controlled skip A/B or evidence of a skip
bug. Loading work otherwise paused under head's game-code-speed redirection.

`scripts/check_archive_skip.py` compiles the actual production skip source
into a private DLL with `ddnative-base` only. The composed checker
`research/native/check_archive_skip_streams.py` compares original CFEEC0
instructions with that Rust skip. **Both sides execute original CFE9D0,
DB92C0, CF02E0 and decoder helpers**, over installed compressed ARC members.
The original reader CFEC00 also executes after selected texture skips.
No generated shards, game window, or changes to the skip implementation.

- **256 matching skip returns:** eight ARC members, capacities 257/4096/
  32768 and a fourth case with a physically wrapped compressed ring plus
  delayed publication at the first Sleep boundary. Includes zero requests,
  spans ending at buffer boundaries, refills, partial final buffers and 17
  failed EOF refill attempts per case.
- **129 matching texture skip/read pairs:** eight distinct shape/format
  samples from installed mip-bearing textures, each at all three buffer
  capacities. Subsequent selected mip bytes match the TEX payload decoded
  independently by Python zlib; no game data is committed.
- At each compared return: GPR/XMM, defined flags, all private scratch
  (reader/stream/state/window/workspace/queue entry), full stack and output
  buffers match. Refill entry registers and scratch hashes, and critical-
  section/Sleep call sequences and arguments, also match.

The oracle uses explicit deterministic Win32 critical-section stubs. The
delayed case publishes the already available fixture at the first Sleep;
it does not model live threads, timing, ring overwrite races or the entire
resource loader. The bridge uses the Mem fallback rather than page-backed
concurrent memory. These limits prevent a concurrency/image acceptance
claim. **No exactness mismatch was found; no production fix was made.**

Run, with artifacts private and the install read-only:

```
python scripts/check_archive_skip.py
E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/check_archive_skip_streams.py
```

Private final log `C:/ddda-targets/L165/skip-chain-final.log` SHA-256:
`6996d497a49a26cb1a9b326ba2c2c532c33ccef2f5b284aca596641998713d21`.
Private bridge DLL SHA-256:
`b3826ffecbe141c75c6d92b68d92b810a73b50c31ca0530e73b2fa0137ce3b78`.

Next: keep skip opt-in pending a controlled same-route head A/B. Resume
the separately assigned worker-code ranking/ports; do not claim a loading
gain or resource regression from c32play's moving-camera average.
