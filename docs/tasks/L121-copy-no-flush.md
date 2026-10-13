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
  eight-copy frames: equal size A8R8G8B8/G16R16, scaled, format conversion,
  D24S8 and MSAA.
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

Implemented: all accepted StretchRect paths record into the shared encoder.
Blits, MSAA colour resolves and the existing MSAA depth/stencil shaders retain
their operations and view formats, without private encoder submissions.
Single-sample depth/stencil surfaces of matching format use a full-subresource
`copy_texture_to_texture` with `TextureAspect::All`. Local wgpu-core 29.0.4
`command/transfer.rs` validates matching formats/full depth-stencil extents and
splits the depth and stencil aspects for the native copy.

Both copy targets are retained in the upload-hazard set, including blit and
depth paths. Later CPU uploads submit pending uses before overwriting data.
The 4,096-command batch limit remains; eight-copy frames do not reach it.
ColorFill uses the shared blit helper too; its existing pre-fill flush stays.
There is no change to readback/query/Present or ring-capacity flush policy.

Verified on Vulkan: **38 passing GPU test invocations / 35 distinct tests**,
including all four target-format shards; **three existing exploratory tests
ignored**. Breakdown: copy 3, MSAA 3, draw 16, targets 10 invocations (7 distinct),
frame 3, cache 3. No internal adapter/D3D9 skips and no timeouts. Longest
target test 31.13 s; cache 20.44 / 6.85 / 0.57 s. All **nine CPU unit tests pass**.
The final expanded MSAA-destination chain passes too (all three focused tests
rerun). Runtime light suite: **171 passed, 0 failed, 3 existing ignored**,
serial `--no-fail-fast`, exit 0; fullscreen gamma passed on this desktop run.

Final release benchmark: three alternating before/after pairs, ten measured
frames each, eight copies per frame, one excluded warmup frame per path.
Both executables use identical instrumentation; the baseline executable was
preserved before modifying the copy helpers. Medians:

| Path | CPU us/copy before → after | Submits/frame before → after | Waited ms/frame before → after |
| --- | --- | --- | --- |
| Equal-size A8R8G8B8, 1920×1080 | 4.781 → 4.322 | 1 → 1 | 3.399 → 3.214 |
| Equal-size G16R16, 192×128 | 2.688 → 2.650 | 1 → 1 | 0.306 → 0.301 |
| Scaled A8R8G8B8, 1920×1080 → 960×540 | 138.211 → 17.572 | 9 → 1 | 2.023 → 1.339 |
| A8R8G8B8 → A16B16G16R16F, 1920×1080 | 145.956 → 26.789 | 9 → 1 | 6.867 → 6.339 |
| D24S8, 192×128 | 990.469 → 2.720 | 9 → 1 | 8.155 → 0.319 |
| 4× MSAA A8R8G8B8 resolve, 1920×1080 | 186.156 → 12.054 | 17 → 1 | 4.543 → 3.654 |

The waited frame time includes clear/Present/readback; submission counts exclude
the explicit readback used to wait. Copy timings exclude Present, so reducing
submit work in `copy` also moves final submission to Present; the waited column
shows the complete synthetic-frame effect. Identical-size paths were already
batched and show no material claimed improvement. Shared-machine load changes
absolute timings: an earlier baseline measured D24S8 at 1,669.596 us/copy, so
the final matched pairs above are the comparison, not that earlier baseline.
No gameplay/FPS or original-game-image claim: head must integrate and compare.

Build caveats: broad `cargo test --no-run` reaches an inherited `module_cost`
example error (`shader_cache.rs` references `crate::pipeline_log`, absent from
that example). Explicit library and assigned integration targets compile.
An initial prepared-source copy retained an older timestamp, so Cargo reused
the baseline library; all three no-submit/hazard regressions rejected it.
Touching and rebuilding the source corrected the stale artifact; only the
subsequent passing suite above verifies the implementation. The local runner
also needed colon-safe filenames for CPU unit names; those nine tests were
run directly instead. No repository checks were weakened.

Private logs and binaries: `C:/ddda-targets/L121/`. Step 1 (counter and benchmark)
is `3c9624a7`. No game, shard builds, Ghidra mutations or game-data writes.
