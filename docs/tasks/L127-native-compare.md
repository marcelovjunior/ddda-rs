# L127: native pass comparison

Assigned to codex-11 on `task/L127-native-compare`, from `task/fps-line`.
See `AGENT-RULES.md` and `../NATIVE_RENDERER.md`. Runtime-only work;
no game execution, lifted-shard build, or writes to the installed game.

## Scope and integration contract

- L127's follow-up to the L119 sink (head board #11263) selects
  `DDDRAW_NATIVE=compare`, emits a FIFO `NativeCompareBegin` before the
  oracle's first attachment write, keeps D3D9 oracle draws, then sends
  the matching single-pass NativeJob.
- L126 / codex-9 owns transport and backend worker: snapshot real color
  and depth/stencil to same-format shadow attachments before the oracle,
  execute the native pass into shadows, and submit comparisons. Keep
  sampled input resources pointing to the oracle state. Never substitute
  a native shadow into the game's visible output in comparison mode.
- L127 owns `ddgpu::native_compare`, its GPU reduction/readback and tests.
  Queue submissions must follow all preceding native/oracle work and the
  CPU finisher. Poll after jobs and Present so the final pass also retires.

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

## Producer semantics

The real targets receive L119's decoded DrawCalls through the D3D9 backend;
shadows receive NativeJobs. This is a backend-path comparison, not an
independent re-decoding of the command record. L119's separate oracle test
checks its decoded DrawCall against the runtime's original D3D9 call path.
Returning the command to the original executor here would be incorrect:
its indexed-draw merging can skip the sink for subsequent merged records.
Comparison therefore handles the same records as the native/drawcall sink.

Comparison ends at attachment changes, fallback commands, frame end, and
before dirty uploads can replace pending native input resources. Begin or
end rejection never repeats a D3D9 draw: the oracle executes exactly once.
Pass IDs increase across frames. Producer diagnostics distinguish begun,
submitted, rejected, oracle draws and shadow draws. Submitted is not GPU
verified; GPU summaries arrive separately. Only the first 16 producer
rejection details per reporting interval print; all refusals count.

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
No checks weakened. A final integrated suite follows worker integration.

L126 worker integration remains in progress. In-game verification belongs
to the head; no gameplay equivalence has been claimed.
