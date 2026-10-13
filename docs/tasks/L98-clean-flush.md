# L98: skip clean resource flushes

Owner: codex-10. Branch: `task/L98-clean-flush`, based on `task/fps-line`.

## Scope

Keep dirty upload behavior and order while removing per-read object lock
accesses and texture mip walks for clean resources. Scope: d3d9draw reads /
flush, d3d9dev Objects dirty index and lifecycle, d3d9res mutation tracking,
d3d9copy mutation tracking, focused tests and documentation. No game runs,
GPU tests or shard builds. Coordinate snapshot relocation with codex-7.

## Implementation

- `Objects.dirty` indexes buffers, surfaces, and owning textures. Surface
  aliases are indexed as well as parents so direct StretchRect / Present
  flushes retain their original resource IDs and mip indices. A directly
  flushed child may leave a conservative parent entry until its next read.
- Resource accessor returns cover Image::unlock, buffer_unlock, and image
  take/restore sequences; copy mark_dirty and surface_shadow restoration
  explicitly update the index. New object insertion tracks dirty images;
  final release and child destruction remove index entries. The safety
  argument listing the writers is beside Objects::track_dirty.
- `flush_reads` uses one objects closure for all reads, collecting upload
  descriptors in original order, then submitting in that order. Clean
  resources never reach the live object map or mip walk.
- Snapshot uses 42 stack slots: 21 texture samplers (16 pixel, displacement,
  four vertex), 16 streams, one index buffer, four render targets.
- Debug `DDRUNTIME_VERIFY_DIRTY_FLUSH=1` scans each skipped resource using
  the old walk's dirty predicate, including dirty images without shadows.
  Any false negative asserts; selected resources use the unchanged clearing
  walk. Thus skipping cannot change any image dirty flag or emitted upload.
- Existing AddDirtyRect/AddDirtyBox and readback behavior is unchanged.
  They do not newly set image dirty flags in the pre-L98 implementation.

## Results

Implemented and tested; no original-game/FPS comparison.

- CPU quiet respected until claude-9 board #10458 ("build B done").
  Head #10445 authorized shared-file hunks. Codex-7 #10435 will switch
  relocated snapshot to `super::flush_reads(m,
  reads[..read_count].iter().copied())` when integrating.
- Final `cargo test -p ddruntime --no-default-features --no-fail-fast --
  --test-threads=1`, with `DDRUNTIME_VERIFY_DIRTY_FLUSH=1`, target
  `C:/ddda-targets/L98`, incremental off, jobs 4: **152 passed, 1 failed,
  1 ignored**. Both new dirty-flush regressions pass, as do existing
  resource/draw/copy tests and draw_path's two tests. No dirty audit failed.
- The one failure is `d3d9_device` fullscreen gamma: Reset returns
  INVALIDCALL at line 135. It also fails with untouched HEAD versions of
  all four modified runtime files. This pre-existing base/host failure
  is not weakened or changed by L98. Full-suite green remains blocked by
  it; head can investigate display/Reset separately.
- Three alternating debug `draw_path::a_draw_costs_the_services` samples:

  | Sample | Before (us/draw) | After (us/draw) |
  | --- | ---: | ---: |
  | 1 | 39.51 | 28.51 |
  | 2 | 34.92 | 29.79 |
  | 3 | 35.03 | 27.09 |
  | Median | **35.03** | **28.51** |

  Median drops **18.6%**; allocations **12 -> 10/draw**, including test COM
  argument buffers. This is service-side synthetic timing on a shared
  machine, not gameplay FPS. Baseline uses original d3d9draw.rs with new
  setter dirty bookkeeping still installed (outside measured draw loop).
  Baseline and final executable hashes differ. An earlier identical-binary
  timestamp-reuse sample was discarded, and source rebuilt before the
  final suite and these measurements. Audit disabled for timing.
- Logs/binaries are local under `C:/ddda-targets/L98`: `light-final.log`,
  `base-gamma.log`, `paired-final.log`. No game, GPU test, or shard build;
  no game data added. Cargo's unrelated ddlift-hot lockfile refresh reverted.

Next action: head integrates the flush_reads call into codex-7's relocated
snapshot and measures gameplay against the current FPS line. The standalone
L98 branch retains the original snapshot outside its reads/flush block.
