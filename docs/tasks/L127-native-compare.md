# L127: native pass comparison

Assigned to codex-11 on `task/L127-native-compare`, from `task/fps-line`.
See `AGENT-RULES.md` and `../NATIVE_RENDERER.md`. Runtime-only work;
no game execution, lifted-shard build, or writes to the installed game.

## Scope and integration contract

- L119 / claude-2 selects `DDDRAW_NATIVE=compare`, emits a FIFO
  `NativeCompareBegin` before the oracle's first attachment write, keeps
  D3D9 oracle draws, then sends the matching single-pass NativeJob.
- L126 / codex-9 owns transport and backend worker: snapshot real color
  and depth/stencil to same-format shadow attachments before the oracle,
  execute the native pass into shadows, and submit comparisons. Keep
  sampled input resources pointing to the oracle state. Never substitute
  a native shadow into the game's visible output in comparison mode.
- L127 owns `ddgpu::native_compare`, its GPU reduction/readback and tests.
  Queue submissions follow all preceding native/oracle work and the CPU
  finisher. Polling after jobs and Present retires completed summaries.
  The worker reuses five shadow slots, reserves space in its 256-summary
  pending queue, and logs pending/unfinished counts on shutdown without
  waiting for the GPU. It imports shadow views directly; real resource
  maps and sampled inputs are not replaced.

Every successful target summary includes pass index, target identity,
aspect, compared pixels, differing pixels, maximum absolute channel
difference and differing nonfinite pixels. Color, depth and stencil have
separate shader variants; color differences are in shader-loaded channel
units (UNORM normalized to 0..1). Use linear views for sRGB storage.
The shader compares loaded f32 channel bits, including signed zero;
NaN payload preservation through texture loads is not guaranteed. This
is not a claim of bitwise equivalence of arbitrary float storage bytes.

Reduction uses one 8x8 workgroup per tile, with four global atomics per
tile. Only a 16-byte summary is read back per target. `map_async` and
`PollType::Poll` never wait for GPU completion. The pending queue is
bounded. Saturation, unsupported extents or mapping failures are explicit
unverified errors, never matches. Inputs must be identically sized,
single-sample D2 subresource views with texture-binding usage. Multisample
shadowing is outside the first worker adapter's supported set.

PNG output is optional in the assignment and is not implemented in this
batch. No game data or private captures belong in the repository.

## Results

Step 1: four focused Vulkan tests pass (0.39-0.94 s each), each with a
60-second external timeout and the pause gate checked. Fixtures cover
aligned/unaligned extents, BGRA8, RGBA8, RGBA16F, RGBA32F, pass snapshot
ordering, D24S8 depth/stencil, signed zero, infinities and queue exhaustion.
The CPU summary identity/metrics test also passes.

The scale fixture checks 128 pending target summaries (8 at 1920x1080,
120 at 64x8), each with a last-pixel mismatch: **16,650,240 pixels**, just
**2,048 readback bytes**. Observed 58.73 ms including pipeline initialization,
submissions and polling; this is a synthetic diagnostic, not a frame cost
or FPS claim. Production code never calls `PollType::Wait`.

Step 2: compare producer and **three pure transport tests pass**, covering
marker ordering, subresource splits, rejection without duplicate oracle
draws, and deferred FIFO ordering across uploads/fallbacks. The first
runtime light suite (before L119 integration) is **173 pass / one inherited
fullscreen-gamma failure / three ignored**, all 25 test targets reached.
The producer-inclusive suite is **179 pass / two failures / three ignored**;
the existing parallel timing failure passes an exact isolated rerun (0.31 s),
giving **180 distinct passes across runs**. The remaining failure is the
inherited fullscreen-gamma Reset check. No checks weakened. On the combined
L126 worker + L119 cache-2c branch, all **eight transport tests**, the **three
producer tests**, and the updated **DrawCall oracle** pass again.

Step 3: L126 worker `3079a961` and L119 cache `df8397bb` are integrated.
**All five end-to-end GPU shadow cases pass**, 0.61-0.85 s each with an
external 60-second timeout and both board/pause gates observed:

- Alpha blending proves the shadow starts from pre-oracle contents.
  A changed native constant reports 64 differing pixels and maximum
  channel difference 0.49803922; real color output remains byte-identical.
- Duplicate begin, mismatched end, unsupported instancing and MSAA return
  five explicit unverified results. The next valid pass recovers and matches;
  no rejected job changes the oracle.
- 32 successive jobs retain distinct pass IDs and match using reused shadows.
- D24S8 load state matches. Native-only depth/stencil clears report 64
  differences each (max 0.49999997 / 2), and a following known-value pass
  proves the real depth/stencil contents were preserved.
- A texture's mip-1 attachment compares the correct 8x8 subresource and
  leaves the real mip bytes unchanged.

The five tests produce **42 matched / three intentional mismatched target
summaries**, five intentional unverified rejections, and zero pending or
active comparisons on shutdown. Together with step 1, **nine GPU tests pass**.
Private logs are under `C:/ddda-targets/L127*`; no captured game data is in
the repository. Source checkpoints: `ef5d6869` (reducer), `ec32deb2`
(producer), integration merges `d9bdc5c5` (worker) and `2b30ca2d` (cache).

Next: the head runs `DDDRAW_NATIVE=compare` in gameplay and checks mismatch,
unverified and pending counters, then captures the visible D3D9 oracle.
Game/image/FPS equivalence remains untested by this agent. PNG dumping is
not implemented; multisampling and active-query comparisons are explicitly
unverified, matching the first worker adapter's eligibility limits.
