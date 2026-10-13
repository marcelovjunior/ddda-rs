# L133: native run fixes and Vulkan interop

Owner: codex-9. Branch: `task/L133-native-next`, from `task/fps-line`.

## Brief

First priority: head's combo17 `DDDRAW_NATIVE=compare` and `=1` runs;
fix worker mismatches, refusals, crashes and performance with claude-2.
Second: prototype raw Vulkan recording on the wgpu device/queue, sharing
resources, and measure the bridge toward the 240 fps target.

Scope: native ddgpu device/worker, native tests and benchmark; narrow
results documentation. Never run the game, build shards, or push. GPU
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
messages; first 16 reasons are `native indexed strip restart unsupported`.
Jobs include six passes / 253 draws. Producer "draws taken" and zero producer
rejections count enqueue success, not worker execution. The gameplay rate
cannot establish native correctness or performance with these missing draws.

`GraphicsBackend::native_or_draw(job, original_calls)` now carries both
representations through one DeferredBackend FIFO command. The immediate
worker tries native and, on atomic rejection, executes the original calls
exactly once. No producer round trip; unsupported strips retain the guard.
The API accepts draw-only jobs (clear/copy commands stay in FIFO separately).
**12 d3d9cmd CPU tests pass**, four new: rejection ordering, nonblocking
acceptance without duplicate draws, unsupported backend fallback and the
draw-only contract. Producer integration is coordinated with claude-2.

Combo17 Compare mode records **zero** begin/submit/oracle/shadow operations.
Root cause: `g.native` initialized only for Native, then `!g.native` selects
the ordinary DrawCall branch before Compare. Reported to claude-2/codex-11;
the head must rerun after their producer fix. Verify also reports sampler
MAXANISOTROPY 16/4 mismatches; decoder correction belongs to claude-2.

Prototype CPU check passes; GPU comparison pending.
The oracle requires every RGBA byte to equal the wgpu reference and at
least one rasterized pixel. Timing includes tracking/encoding, finish and
submit; GPU waits and readbacks are outside the timed interval. The native
path has an additional transition from the preceding frame's readback.

Unverified scope: sampled images, depth/stencil, MRT, MSAA, subresources,
resource destruction in flight, production cache integration and concurrent
finisher recording. No gameplay correctness or FPS claim. Head's combo17
logs are pending; GPU work is held during its scheduled runs.

Next action: finish CPU check, run the bounded image/performance gate when
head releases the GPU, and handle combo17 findings first.
