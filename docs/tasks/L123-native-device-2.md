# L123: native device phase 2

Owner: codex-9. Branch: `task/L123-native-device-2`, based on L110 with
`task/fps-line` fast-forwarded to `da9036d4` before changes.

## Brief

Toward < 1 us/draw on the native backend: delta constant uploads, persistent
texture/sampler groups by object identity, repeated render bundles, and a
5,000-draw / 40-pipeline / 200-set benchmark with realistic register churn.
Coordinate the L119 N2 transport with claude-2. No game or shard builds/runs.

Scope: `ddgpu/src/native.rs`, its internal constant arena module,
`ddgpu/tests/native.rs`, `ddgpu/examples/native_draw_cost.rs`, and narrow
task/results documentation. Runtime and shader translator APIs stay compatible.

## Implementation

- `Recorder::with_resident_constants(device, bytes, true)` retains GPU
  snapshot storage across submissions. Each draw still sees its own immutable
  offset. CPU snapshots are compared against the preceding submitted contents
  in 16-byte units. A single packed upload contains destination indices and
  changed registers; one compute scatter applies it before the render encoder.
  All operations use wgpu queue ordering. No CPU wait is necessary for reuse.
- Sequential draw churn and cross-frame snapshot-slot churn are different
  quantities. This implementation reduces the latter; it does not assume the
  cap1 sequential ~250 KB measurement proves ~250 KB of cross-frame transfers.
  It still copies/compares full CPU snapshots. Register payload uploads cost
  20 bytes per dirty 16-byte register plus a 16-byte header.
- `Recorder::enable_bundles(true)` retains the most recent submission's exact
  draw sequences. Keys include pipeline, constant offsets, texture/sampler
  sets, buffers/ranges, indexed/base-vertex/instance arguments, attachment
  formats, depth format and sample count. Equality compares full packets;
  there is no hash-only correctness decision. Dynamic viewport, scissor,
  stencil reference and blend constant changes split sequences. A mismatch
  rebuilds the sequence; unchanged contents use the cached wgpu bundle.
- Existing texture groups already use persistent layout + view/sampler
  identities. Both new paths reuse those groups; neither creates them per draw.
- New cumulative stats: uploaded bytes, dirty registers, bundle builds/hits.
  Existing default recorder stays on the direct/full upload path.
- Board API proposals: #10951, #10954, #10975. Owned NativeCommand messages
  in the same DeferredBackend FIFO remain the agreed direction from L110;
  backend-local handles require prewarming and persistent recorder ownership.

## Verification and measurements

Optimized GPU gates: all **6 tests pass**, including 32 full/compact
state cases against the D3D9 backend/host driver, eight additional
multi-snapshot frames against the host driver, full/compact texture and
ring checks, bundle reuse and invalidation. **11 CPU tests pass**.
Runtime light suite on da9036d4: **171 passed / 1 inherited fullscreen
gamma-reset failure / 3 ignored** (`--no-fail-fast`, serial test threads).
Builds use `C:/ddda-targets/L123`, incremental
off and four cargo jobs. GPU runs use Vulkan, `heavy.py` with `HEAVY_BIG=1`,
an unpaused machine and a separate hard 60-second deadline per case.

Benchmark modes `churn`, `resident`, `resident-bundles` share full shader
constant layouts, 5,000 draws, 40 pipeline identities, 200 sampled sets and
1,700 changed draws/frame. Synthetic changes total 8,200 VS + 7,300 PS
float4 registers (248,000 bytes) per frame. Values change across frames;
resource/draw order repeats. This matches aggregate cap1 churn, not an actual
game command sequence. Minimal shaders/16x16 triangles are not FPS evidence.

## Results

Core checkpoint: `e062d80a`. Three alternating optimized repeats (20 measured
frames each, 3 warmups) on RTX 4060 / Vulkan / NVIDIA 610.62:

| Mode | Repeat medians, us/draw | Median |
|---|---|---:|
| full upload (`churn`) | 1.962 / 1.559 / 1.644 | 1.644 |
| resident | 1.699 / 1.264 / 1.238 | 1.264 |
| resident + bundles | 0.868 / 0.843 / 0.822 | **0.843** |

CPU timings include updates, upload, encoder finish and queue submit; GPU
completion waits are outside timing. Warm resident uploads are **340,016
bytes/frame** (17,000 dirty snapshot registers plus indices/header), versus
**15,666,960 bytes/frame** for the identical full-upload workload. Sequential
setter payload is 15,500 registers / 248,000 bytes: retained fifth-register
values explain the larger cross-frame snapshot count. Warm bundles rebuild
zero / hit one sequence per frame. Two initial builds include startup offset
alignment changes. The <1 us synthetic target is met; cold misses, changing
sequences, larger shaders/targets and tiny-pass workloads need separate data.

N2 API agreement: head decision **#11087** adopts the owned NativeJob FIFO
proposal. The worker adapter owns Device/Recorder; Claude-2 never touches
wgpu. Head assigns that adapter/transport as L126 to codex-9 on this branch.
L126 now retains closed RecorderState storage across jobs and hands render
encoders to the FPS line finisher. Persistence source `e3599f42`, comparison
submission-order follow-up `542111eb`; final 16 GPU / 14 CPU tests pass.
See `docs/tasks/L126-native-worker.md` for worker integration evidence.
No game image or gameplay/FPS comparison claimed. Private logs:
`C:/ddda-targets/L123-final-gate.log`, `C:/ddda-targets/L123-runtime-all.log`.
A rebuild was externally terminated with exit 0xffffffff and no diagnostic;
retry under the heavy slot succeeded. Clippy completes with existing/style
warnings; no clean-clippy claim. No game, shard, installation writes or push.
