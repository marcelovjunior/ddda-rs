# L159 — Vulkan default image gate

Owner: codex-9. Branch: `task/L159-vk-default`, base combo27 FPS line.
Scope: ddgpu Vulkan pass oracle/fallback audit, separate default selection commit.
No game execution, lifted shard build, private data or installation writes.

## Head image check

Merge the **oracle commit only** first. In the head's usual autoplay command:

```powershell
$env:DDGPU_VK='1'
$env:DDGPU_VK_SHADOW='997'
# Head runs its normal ddplay/autoplay command and captures stderr privately.
```

Unset `DDDRAW_NATIVE` (the default drawcall path). `DDGPU_VK_SHADOW=N`
selects every Nth **eligible raw pass**, beginning with the first. `1`
checks all eligible passes and is deliberately expensive; `0`/unset disables.
Sampling is fixed at first use. `997` spreads checks across the many tiny
simulation passes and scene passes; inspect `pixels`/aspect receipts to confirm
that large scene targets, small float simulation targets and depth/stencil
actually occurred. A zero mismatch count with zero checks is not acceptance.
This diagnostic changes CPU/GPU load; do not use its FPS as a performance result.

Each selected pass copies its real attachment load state, including depth AND
stencil, before either replay. The ordinary wgpu encoder executes the exact
same retained draw records/bindings/constant offsets on independent shadows.
The production Vulkan encoder writes the real targets. GPU reductions then
compare every color/depth/stencil aspect, before any later pass may overwrite
those targets. The normal real render continues unchanged. Queue order and
owned pass/resource lifetimes cover asynchronous parallel finishing.

There is no cross-run gameplay determinism assumption: both versions consume
one run's same inputs. This checks Vulkan versus wgpu, not original D3D9,
and not a separate gamma/OS presentation oracle. Selected passes are isolated
from neighboring raw batches to insert the snapshots/reductions, so this does
not independently establish equivalence of every unsampled batch boundary. Gamma/present remains on the
same existing wgpu path in both configurations.

Receipts: `ddgpu-vk-shadow pass=... slot=... aspect=... pixels=actual/expected
 differing_pixels=... max_diff=... nonfinite=... summary_hash=...`.
The hash is BLAKE3 of the **16-byte reduction**, not the frame; equality uses
all compared image values, not hash equality. Float comparisons use textureLoad
bits (including signed zero); NaN payload storage preservation is not promised.
Cumulative `matched_aspects`, `mismatched_aspects`, `skipped_passes`,
`pending_aspects`, and `errors` accompany normal Vulkan stats every ~2 seconds.
Counters are process-wide; pass IDs identify sampling order, not guest frames.

Acceptance: nonzero checks covering gameplay target classes, zero differing
pixels/mismatched aspects, zero errors and Vulkan encode_errors. Pending work
is explicitly unverified until its receipt arrives; account for final pending
work rather than treating it as matched. Capacity/unsupported-view skips are
reported separately and must be reviewed for coverage. Capture a normal route
with diagnostic off afterward for FPS. Only after head image acceptance merge
the separate default commit; `DDGPU_VK=0` remains the explicit escape hatch.

## Fallback audit

`fallback_passes` includes, rather than excludes, `initialization_passes`.
New counters separate `query_passes` and `resource_fallback_passes`.

- **Queries:** active guest occlusion queries allocate/query wgpu query sets;
  the raw pass contract has no query begin/end/results support. Preserve wgpu
  sample counts and guest visibility decisions. These are the only frontend
  `unsupported_pass` calls on the default drawcall path.
- **Initialization:** first use of buffers/views runs the retained wgpu pass
  so wgpu-core establishes zero-initialization and resource tracking. Publish
  this only after ordered submission; worker finish order is not submission
  order. Removing it without integrating core initialization is unsafe. New
  resources, release/recreation and parallel chunks can all incur it.
- **Resource validation:** missing raw shader/pipeline/group/view, multisample
  images/pipelines, duplicate attachments or attachment/sample usage conflicts,
  invalid or insufficient vertex/index ranges, dynamic uniform alignment/range,
  and unsupported viewport/scissor bounds stay with validated wgpu. See
  `vk/pass.rs::resources`; these guards are safety/semantic constraints, not
  an optimization TODO to bypass. Unsupported Vulkan object creation already
  logs the reason at the source. Whole-texture feedback is conservatively
  refused even when subresources might not overlap.
- **Encode failure:** raw HAL construction failure increments `encode_errors`
  and falls back to the retained wgpu records. Image acceptance requires zero.
- Clears, copies, compute, gamma/present and other non-draw helper encoders
  already remain wgpu chunks; they are not counted as raw draw-pass fallbacks.

The earlier ~4% is supplied in the assignment, not a new measured breakdown.
Head's in-game log with the added counters is needed to apportion it. No
fallback class was removed without its semantic support and image evidence.

## Results

Implementation: same-run selected-pass image oracle, all attachment aspects,
bounded outstanding diagnostic work, per-aspect receipts, and fallback census.
Validation (2026-10-13): **30 CPU tests and 19 focused GPU tests pass**.
The GPU gate is 13 existing Vulkan cases with shadow enabled, four existing
comparison tests, the new same-run gate, and the new negative readback gate.
All processes completed within 5 seconds with a hard 60-second kill bound,
pause status checked before launch and while running. Observed **568 matching
aspect receipts**, zero unexpected mismatches. The tiny-pass saturation case
explicitly skipped checks at the bounded capacity; skips are not matches.
The dedicated gate covers four raw passes / 14 aspects with **zero skips,
pending, errors or mismatches**, including nonzero stencil load and replacement.
Negative fixtures produce exactly one mismatch, one incomplete-count error,
and one match; the existing reduction tests also inject actual texture-value
mismatches, signed zero and nonfinite values.

Image hashes (synthetic, not gameplay):
- MRT/depth/blend, 192 bytes:
  `230bba4cd1f9aa979eeb94e992546e6105fd547ee7ff9b009e06c77836fdd17f`.
- Scissored stencil load/write, 64 bytes:
  `f729d3ca1043bb30cb0c09a925fd28e3a0119237c4a77771cb8fbb3c1cf46ebc`.
- Matching 16-pixel reduction (16 bytes):
  `2aad9a79e7ec28638f3d53231d2e010408c8df24a42fe8980c79a9f13a76b760`.

Runtime light no-fail-fast suite: **221 reported passes, two failures,
six ignored** (the reported sum includes child-process test summaries).
Failures are inherited fullscreen gamma and Present timing; isolated Present
retry passes (one test). No changes to those runtime files/checks.
Private receipts: `C:/ddda-targets/L159-{build,runtime,runtime-retry,gate-0,
gate-1,gate-2}.log` and named per-test logs. First negative fixture attempt
omitted queue submission for initial contents; corrected fixture uses ordered
write/submit and passes. No production comparator relaxation.

Reproduce with `CARGO_TARGET_DIR=C:/ddda-targets/L159`,
`CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`:

```text
cargo test -p ddgpu --lib
cargo test -p ddgpu --test vk_encoder --no-run
# Only when not paused; each GPU process needs the 60-second timeout wrapper:
cargo test -p ddgpu --test vk_encoder shadow_same_run_mrt_depth_stencil -- --exact --ignored --nocapture
cargo test -p ddgpu --lib vk::shadow::tests::readback_rejects_mismatch_and_incomplete_counts -- --exact --ignored --nocapture
cargo test -p ddruntime --no-default-features --no-fail-fast
```

No in-game acceptance claimed. Oracle commit leaves `DDGPU_VK` opt-in;
separate default-on commit is for head to adopt only after image acceptance.
Next: head runs the oracle on combo27 route; default commit stays staged until
that gate passes.
