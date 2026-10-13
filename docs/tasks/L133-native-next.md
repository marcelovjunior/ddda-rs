# L133: native run fixes and Vulkan interop

Owner: codex-9. Branch: `task/L133-native-next`, from `task/fps-line`.

## Brief

First priority: head's combo17 `DDDRAW_NATIVE=compare` and `=1` runs;
fix worker mismatches, refusals, crashes and performance with claude-2.
Second: prototype raw Vulkan recording on the wgpu device/queue, sharing
resources, and measure the bridge toward the 240 fps target.

Scope: native ddgpu device/worker, native tests and benchmark; narrow
results documentation. The head/claude-2 coordination expanded transport
scope to `d3d9cmd` for atomic fallback. Never run the game, build shards, or push. GPU
checks require an unpaused machine, HEAVY_BIG slot and 60 s case timeout.
Builds use `C:/ddda-targets/L133`, incremental off, at most four cargo jobs.

## Prototype

`native_draw_cost <frames> <draws> ash-interop <passes>` records raw Vulkan
render passes/draws into `CommandEncoder::as_hal_mut`. A preceding wgpu
encoder declares vertex/index and color-target usage using
`transition_resources`; both command buffers submit through `wgpu::Queue`.
The native render pass preserves `COLOR_ATTACHMENT_OPTIMAL`. A normal
wgpu readback follows every frame, exercising COPY_SRC -> COLOR_TARGET
and COLOR_TARGET -> COPY_SRC repeatedly. All borrowed resources and raw
objects remain alive until GPU completion.

The target and vertex/index buffers belong to wgpu. Pipelines,
descriptors and the uniform allocation belong to the benchmark's Vulkan
code. wgpu 29.0.4 exposes no public `as_hal` for pipeline/bind-group handles,
and its Vulkan buffer's raw handle is private; HAL vertex/index binding
methods accept the borrowed buffers. A production recorder needs its own
pipeline/descriptor cache or a larger HAL implementation. This benchmark
mode does not enable a raw path in gameplay.

wgpu initialization tracking does not see raw writes, so an ordinary wgpu
pass initializes the target first. Resource HAL guards hold a device snatch
read lock: they are scoped to recording, not held across finish/submit/poll.
No raw queue submission is used for shared work, preserving wgpu's queue
locking and completion tracking. Separate raw/normal encoders respect
wgpu 29's encoding-mode restriction.

The benchmark triangle now spans two pixels on each axis instead of one,
ensuring the image oracle tests rasterized pixels as well as clears. All
backends use the same geometry. Rerun controls; do not silently compare
these numbers with earlier L110/L123 floor measurements.

## Results

Combo17 native run fails worker acceptance: **63,496** asynchronous rejection
messages, **69,390 passes / 3,488,189 draws**; first 16 reasons are
`native indexed strip restart unsupported`. Largest rejected job: 724 draws.
Producer "draws taken" and zero producer
rejections count enqueue success, not worker execution. The gameplay rate
cannot establish native correctness or performance with these missing draws.

`GraphicsBackend::native_or_draw(job, original_calls)` now carries both
representations through one DeferredBackend FIFO command. The immediate
worker tries native and, on atomic rejection, executes the original calls
exactly once. No producer round trip; unsupported strips retain the guard.
The API accepts draw-only jobs (clear/copy commands stay in FIFO separately).
**12 d3d9cmd CPU tests pass**, four new: rejection ordering, nonblocking
acceptance without duplicate draws, unsupported backend fallback and the
draw-only contract. Producer integration landed in claude-2's `4bcef3fa`
batch (fallback cherry-pick `119fa79c`). Combo19 has no asynchronous
lost-job ERROR lines. Worker diagnostics `ad244ca4` now count accepted and
rejected jobs, draws and passes, cache sizes, uploads and bundle use, plus a
bounded histogram of exact refusal strings. Counters reflect submissions,
not GPU completion or image verification.

Combo17 Compare mode records **zero** begin/submit/oracle/shadow operations.
Root cause: `g.native` initialized only for Native, then `!g.native` selects
the ordinary DrawCall branch before Compare. Reported to claude-2/codex-11;
the head reran after their producer fix. Combo19 Verify has zero mismatches
in all 11 summaries (last 430,660 compared/equal); final Compare totals are
**42,880 matching attachment/aspect summaries / zero mismatches / 34,112
unverified / zero pending**. Both Compare and Native's first 16 logged
worker reasons remain the blanket indexed-strip rejection. Later reasons
were suppressed, so a complete historical refusal histogram is unavailable.
The new diagnostic histogram resolves this in the next run. Sampler
MAXANISOTROPY 16/4 corrections belong to claude-2.

### Bounded triangle-strip support

Commit `ab4318c9` replaces the blanket indexed-strip refusal with a proof for
triangle strips: D3D9 requires all used indices inside
`[MinVertexIndex, MinVertexIndex + NumVertices)`, before adding BaseVertexIndex.
Accept only nonempty intervals whose exclusive end is at most the fixed
restart sentinel, computed in u64. Then no valid D3D9 index can trigger
wgpu's restart behavior. Zero, overflowing and sentinel-containing ranges
continue to refuse. Source: Microsoft's
[DrawIndexedPrimitive contract](https://learn.microsoft.com/en-us/windows/win32/api/d3d9/nf-d3d9-idirect3ddevice9-drawindexedprimitive).

Read-only cap1 metadata census: 7,559 Presents / 1,852,360 draws;
956,539 indexed strips, all TriangleStrip/u16. **956,101 (99.954%)** have
restart-excluding ranges, **438** remain unproven. Reproduce with
`python crates/ddgpu/examples/native_strip_census.py <capture>`; the script
skips all compressed blobs and emits only aggregate metadata. A synthetic
metadata fixture checks boundary, wide arithmetic, blob skipping and Present
counting. The committed script reproduces those totals on cap1 and explicitly
reports its one truncated tail record; totals cover the complete command
prefix. No index contents were scanned; eligibility uses the API contract.

The initial GPU control fixture incorrectly combined inline indices (the
UP transport, with start zero) and start index one. This caused legacy/host
mismatches before native drawing. Corrected to uploaded index buffers on
both paths; the exact assertions remain. Earlier attribution to line
rasterization was retracted. Indexed LineStrip remains outside this change's
validated subset. The final native GPU gate passes
six triangle-strip cases (u16/u32, nonzero start, negative base vertex, all
three cull modes) against both legacy and host D3D9, plus four atomic
fallback cases with blending and active-query ordering. All **seven worker
GPU tests pass**, including the five existing resource/order/residency cases,
in 0.34–0.57 s per test (HEAVY_BIG, unpaused, hard 60 s each). Log:
`C:/ddda-targets/L133-worker-gpu.log`. No triangle-strip gameplay acceptance
claimed yet; requested head Compare/Native run with `ab4318c9`.

Combo20's actual worker histogram (before the strip fix): **62,805** indexed
strip refusals, **3,502** active queries, **3,069** render-target sampler
channel-fill cases, **2,213** depth-bias state 195 cases. The latter three
remain explicit legacy fallback. Accepted subset: 475,136 submitted draws,
7,997,432 uploaded bytes (**16.83 B/draw**), maximum reported 33 pipelines /
25 texture sets. Five decreases in pipeline count establish at least five
worker cache resets across the 101 snapshots. Added `worker_instances` counts
reconstruction exactly in the next run. This accepted subset does not support
an upload-volume bottleneck claim; full scene performance awaits strip uptake.

**16 ddgpu CPU tests pass**, including range proof/overflow and bounded
histogram accounting. Full runtime light suite (`--no-default-features
--no-fail-fast`): **187 pass / one inherited fullscreen-gamma Reset failure /
three ignored**, across 28 test/doc-test binaries. No shard build or game run.

### Vulkan interop benchmark

Prototype CPU check/build pass. **Two 14-case optimized Vulkan gates pass**,
each including **eight full-image comparisons** (1,024 RGBA bytes each, with
rasterized pixels required). The eight shared-resource cases cover one
draw, 43 draws / seven passes, and three repeats each of 5,000 draws / one
pass and 900 draws / 900 passes. Each shared-resource frame is followed
by ordinary wgpu readback. HEAVY_BIG, unpaused status checked before each
case, 60 s hard timeout per process. Private log:
`C:/ddda-targets/L133-gpu.log`; final confirmation:
`C:/ddda-targets/L133-gpu-confirmation.log`.

Final medians of three 20-frame repeats (three warmup frames each),
RTX 4060 / Vulkan / driver 610.62 / optimized play profile:

| CPU measure | raw ash | ash + wgpu resources/submit | wgpu |
|---|---:|---:|---:|
| 5,000 draws, one pass (us/draw) | 0.315 | 0.333 | 0.966 |
| 900 one-draw passes (us/pass) | 1.435 | 1.321 | 8.549 |

Interop ranges: 0.331–0.341 us/draw and 1.274–1.400 us/pass. Raw ranges:
0.315–0.325 and 1.401–1.532; wgpu ranges: 0.948–1.030 and 8.299–8.723.
The confirmation started with all heavy slots free and no head game pause,
then held the required GPU slot; both orders alternated over three repeats.
It supersedes the noisier exploratory medians (0.354 interop / 1.129 wgpu).
This remains a shared host, not a controlled production benchmark.
The bridged path is feasible without raw queue submission, but these are
prefilled-uniform backend floors, not the resident native workload or FPS.

The oracle requires every RGBA byte to equal the wgpu reference and at
least one rasterized pixel. Timing includes tracking/encoding, finish and
submit; GPU waits and readbacks are outside the timed interval. The native
path has an additional transition from the preceding frame's readback.

Unverified scope: sampled images, depth/stencil, MRT, MSAA, subresources,
resource destruction in flight, production cache integration and concurrent
finisher recording. No raw-backend gameplay correctness or FPS claim. Worker summaries
now distinguish native submissions/rejections from producer enqueue counts,
including upload bytes and pipeline/group cache sizes.

Next action: head reruns Compare/Native with `ab4318c9` to measure the larger
accepted subset. Further native coverage requires query, missing-channel
sampler and depth-bias support; the current FIFO fallback preserves these draws.
