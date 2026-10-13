# L121: copies in the open encoder

Agent: codex-11. Branch: task/L121-copy-no-flush, based on task/fps-line.
Scope: ddgpu copy helpers, focused regressions and copy benchmark. No game runs
or lifted-shard builds; the head owns gameplay and FPS comparisons.

## Baseline audit

`GraphicsBackend::copy` in `crates/ddgpu/src/lib.rs` selects these paths:

| Path | Existing submission boundary | Reason |
| --- | --- | --- |
| Equal-size, equal-format single-sample colour | Ends pass; records copy in open encoder; flushes only at batch limit | L95 already batches this path |
| Scaled colour, format conversion, multisampled destination | Flushes open encoder; `blit` creates and submits its own render pass | Separate encoder must execute after prior draws |
| Multisampled colour source | Flushes; `resolve_texture` submits a load/store pass with `resolve_target`; then copy or blit above | Resolve precedes consumption; sRGB views preserve driver gamma behaviour |
| Depth/stencil | Flushes; `copy_depth` submits its own render pass | Depth write plus eight stencil-bit draws; nine pipelines rebuilt each call |

None of the copy paths reads results on the CPU. They require GPU command
ordering, not a queue submission. Uploads, readbacks, queries, Present, ring
capacity and the existing batch limit remain separate submission boundaries.

## Validation plan

- `copy_cost` reports measured queue submissions and CPU copy cost for synthetic
  eight-copy frames: equal size, scaled, format conversion, D24S8 and MSAA.
  Its readback wait is outside the reported submission count.
- New copy regressions cover command ordering, no intermediate submission and
  subsequent upload hazards. Existing driver comparisons cover depth/stencil,
  MSAA, filtering and formats.
- Vulkan driver tests through heavy.py, HEAVY_BIG=1, only while not paused:
  targets with all four TARGETS_SHARD=i/4, draw, msaa, frame, cache. Each test
  has a 60-second deadline, except cache's requested 300-second deadline.
- Runtime light suite; own target C:/ddda-targets/L121, incremental off,
  CARGO_BUILD_JOBS=4. No game or lifted-shard execution.

## Results

Pending implementation and verification. GPU baseline queued behind the head's
2026-10-13 04:14–04:24 game-run hold.
