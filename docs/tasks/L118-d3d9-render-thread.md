# L118: D3D9 render-thread costs

Owner: codex-7. Branch: `task/L118-d3d9-render-thread`, from `task/fps-line`.
User/head assignment: reduce combo4 render-thread snapshot, state lookup and
runtime access costs. Runtime only; preserve COM and DrawCall interfaces for
L112/L110. No game runs, shard builds, GPU tests, or game-data commits.

## Results

### Step 1: dense state banks

- Sampler state uses 21 x 14 values, texture stages 8 x 33, with separate
  presence bits and a spill map preserving existing arbitrary-key behavior.
  Pixel slots, displacement sampler 256 and vertex slots 257..260 remain
  distinct. Snapshot copies a sampler row instead of doing 14 hash lookups.
- Render states keep the immutable sorted draw payload; a dense index avoids
  binary searches for common state IDs. Unusual IDs retain sorted lookup.
- New tests compare dense banks against the former map across absent/zero,
  repeated writes, vertex slots and out-of-range keys. Render index regression
  checks 300 keys, sorted snapshots and retained old payloads.
- Optional `DDRUNTIME_BENCH_DIRTY_SAMPLERS=1` adds one changed sampler setter
  before each draw in `draw_path.rs`. Same test compiled against pristine base
  and changed runtime. Three paired debug samples, microseconds per iteration:
  baseline 49.88 / 52.43 / 42.47; dense 45.60 / 32.55 / 26.50.
  Median 49.88 -> 32.55 (34.7% lower); 8 allocations unchanged, including
  the test's COM argument buffers. Shared-machine ranges overlap; not FPS.
- Standard draw_path: four tests pass. Full light suite completion and
  known timing/fullscreen failure accounting below in final receipt.

Private logs/binaries: `C:/ddda-targets/L118-*`; build jobs 4, incremental off.
Next: reduce reentrant D3dCell bookkeeping and snapshot temporary allocations;
head owns gameplay/FPS and original-image comparison.
