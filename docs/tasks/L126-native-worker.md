# L126: native job transport and worker

Assigned by head board #11087 to codex-9 on the L123 worktree/branch.
Render-thread sink: claude-2. Comparison reduction: codex-11 (L127).

## Contract

`d3d9cmd::NativeJob` owns one or more `NativePass` records, each with Target
metadata, optional clears, and effective-state NativeDraw records. All state
is copied/shared owned data; no worker accesses guest memory. Identity keys
are immutable object address + hash, plus raster flip. Tokens and declarations
retain their Arc identities. Constants are complete covered binding ranges
for each draw, not cross-job deltas. Types: `crates/d3d9cmd/src/native.rs`.

`GraphicsBackend::native(job) -> Result<(), NativeJob>` defaults to Err with
no side effects. DeferredBackend sends it through its existing bounded FIFO
and caches `native_supported()` once at construction. Unsupported backends
return the job immediately; supported backends enqueue without waiting.
Unexpected worker rejection logs loudly and increments rejection counters.
The producer must guarantee eligibility before sending (head #11216).
Immediate worker rejection still precedes native target writes/submission.
A job must be flushed before any default command, query, or frame end.

Comparison begin/end markers use the same FIFO. Begin must precede the
oracle's first attachment write, preserving the real load/depth state.
The native worker owns wgpu resources; the render-thread sink does not.

## Results

Transport checkpoint: **8 d3d9cmd tests pass**, including accepted-job order
with resource/query commands and default unsupported ownership return. The eighth test blocks the worker
and proves native enqueue returns without draining prior work.
Worker implemented in `ddgpu/src/native_job.rs`, enabled through
`WgpuBackend::native` and `native_supported`. Effective D3D9 state uses the
existing backend conversion helpers and sampler creation. Shader/declaration
Arc identities are retained; state identity caches defensively compare words
(excluding dynamic stencil/blend constants). Texture-set Arc identities retain
persistent groups. Buffer handles key the actual wgpu objects, and resource
release resets the native cache. All imports and pipelines prepare before
recording; a rejected job submits no native target writes.

**4 optimized Vulkan worker tests pass** (each 0.43-0.85 s): 16 indexed /
nonindexed, blend/scissor/depth cases equal both the D3D9 backend and host
D3D9 driver; atomic whole-job rejection/recovery; FIFO upload/copy/release /
resource reuse; sampled texture uploads. **14 ddgpu CPU tests pass**, including
constant-read coverage, DEF, implicit matrix rows and relative indexing.
Private log: `C:/ddda-targets/L126-worker-gpu.log`. Runs use HEAVY_BIG, Vulkan,
four cargo jobs and a hard 60-second timeout per GPU case.

Comparison begin/end worker hooks seed all real color/depth/stencil attachments
before oracle writes, replay via shadow view overrides without remapping real
resource IDs, and enqueue L127 reductions. Pool has at most five reusable
attachment slots; size/format replacement drops native view caches first.
One active begin / one end-pass job; mismatched IDs preserve the active begin.
Structured results: `poll_native_comparisons()` (latest 256 retained), with
nonblocking polls at native jobs and Present, bounded pending queue, explicit
errors/unverified counters and shutdown reporting. L127 reducer dependency
`ef5d6869` was cherry-picked as `751bf420`; its four GPU/one CPU tests were
verified by codex-11. All five end-to-end shadow worker tests now pass in
L127: 42 matching / three intentional mismatch summaries, five explicit
rejections, zero pending; real color/depth/stencil, mip1 and reused shadows
remain intact. Test checkpoint `434af59c` cherry-picked as `3818c5c0`.

N2 now retains `RecorderState` and shader constant handles across jobs. The
resident arena grows only when required; oversized jobs use the direct-upload
fallback when packed scatter storage would exceed the device binding limit.
`NativeJobStats` exposes uploaded bytes, dirty registers and bundle hits/builds.
Exact repeated bundles are host opt-in with `DDGPU_NATIVE_BUNDLES=1`; alternating
job sequences can rebuild them, so they are not enabled unconditionally.

Earlier legacy encoders hand off through `flush_async()`. Native preparation
and recording overlap the finisher; `wait_finisher()` precedes constant queue
writes, then the scatter submits before the render encoder is handed to the
same finisher. This orders reused storage without a GPU wait. Worker and
game-frame performance have not been measured; L123's 0.843 us number is its
dedicated persistent-recorder benchmark, not this adapter.

Persistent-recorder changes compile; final focused GPU gate is queued behind
the shared HEAVY_BIG slot. Merged-base runtime light: **182 pass / one inherited
fullscreen gamma-reset failure / three ignored**. Log:
`C:/ddda-targets/L126-runtime-all.log`.

In-game image comparison and FPS remain with head; agent never ran the game. No game
Worker, GPU correctness and in-game integration remain in progress. No game
or shard build/run. Existing L123 target directory is reused per head #11137,
incremental off and four cargo jobs maximum.
