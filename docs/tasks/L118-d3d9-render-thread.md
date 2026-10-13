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
- Standard draw_path: four tests pass. Step 1 full light run: 156 passed,
  two failures (fullscreen gamma Reset, interlocked timing), one ignored.

Private logs/binaries: `C:/ddda-targets/L118-*`; build jobs 4, incremental off.
Next: reduce reentrant D3dCell bookkeeping and snapshot temporary allocations;
head owns gameplay/FPS and original-image comparison.

### Step 2: scoped device access and snapshot construction

- Indexed draw validates scene/index binding in one device lookup instead of
  two. DeviceLock checks deferred objects through its already retained runtime.
- D3dCell borrows under an existing owning guard retain RefCell checks without
  repeating depth/HELD TLS updates. Slow acquisition/release and lock ordering
  remain unchanged. Regression covers nested mutable-borrow rejection, unwinding,
  outer guard depth and later successful access; concurrent rendering/resource
  destruction test also passes.
- Dirty stream/texture lists use bounded stack staging and exact-size Arc
  construction, removing temporary Vec growth. Resource flush order, validation,
  dirty groups, immutable queued payloads and public command types are unchanged.
- Three new paired dirty-sampler samples, base / final microseconds:
  49.71 / 29.97, 46.49 / 27.76, 75.06 / 52.47.
  Medians **49.71 -> 29.97 us (39.7% lower)**, allocations **8 -> 6**.
- Clean draw benchmark is noisy: six pairs (three reversed) span base
  14.16..27.30 us and final 12.54..26.15 us. Combined medians 19.68 / 19.19 us;
  do not claim a clean-state speedup. Approximately 3 allocations in both.
- Final `cargo test -p ddruntime --no-default-features --no-fail-fast --
  --test-threads=1`: **158 passed, one failed, one ignored**. The failure is
  `a_gamma_ramp_is_shown_on_a_full_screen_swap_chain_only`: fullscreen Reset
  returns 0x8876086c. Independently rebuilt pristine pre-L118 base reproduces
  the same failure (`L118-baseline-gamma.log`). All 32 active unit tests pass,
  including timing tests.
  Dirty-flush audit (`DDRUNTIME_VERIFY_DIRTY_FLUSH=1`) also runs the four draw
  regressions unchanged. No GPU validation, game/FPS or original-image comparison.
- This batch reduces repeated runtime access, but does not replace every
  per-helper CURRENT lookup or the live-object hash map. Those remain potential
  follow-ups driven by head's next profile; no unsafe cached device pointer added.

Next action: head integrates both commits, compares combo4 gameplay/image, and
profiles remaining CURRENT/object access before choosing further work over L112.

### Step 3: clean snapshots, resource reads and service dispatch

Final base: `d769d695` on `task/fps-line` (combo9/L101). Snapshot commit:
`5ffd94f0`. Head assigned dispatch ownership in board #10933; COM and DrawCall
APIs remain unchanged for claude-2's L112/L119 caller.

- Check L98's resource dirty index during the existing device-state borrow.
  Empty index skips reads-array population and the resource walk. Dirty reads
  are collected under that same borrow, eliminating a separate objects access.
  Upload order, duplicates and owner semantics remain unchanged. Debug audit
  still scans resources even when the index is empty.
- Unchanged state groups construct the draw directly from the retained cache:
  no take/restore device lookup and no duplicate declaration/texture Arc clones.
  Constants, render states, viewport and scissor still come from current state;
  resource Lock/Unlock remains independent of state dirtiness.
- Dense resolved COM method-token entries avoid hashing in Registry::call and
  L101's direct-binding cache misses. L101's per-thread ext-call cache remains
  unchanged. Rebinding and instruction-lookup replacement invalidate the dense
  cache, including interior step entries. Regressions cover holes, unaligned
  addresses, step precedence, replacement, table growth and registry isolation.
- L101 landed during this work and already covers runtime pinning, combined
  lock TLS and state-service entry costs. Final rebase retains those production
  implementations unchanged, replacing an overlapping L118 prototype. Two new
  tests prove helper access adds no Arc retains, replaced runtimes remain alive
  during nested access, and retirement/depth bookkeeping survives unwinding.
  Necessary D3dCell acquisition and scheduling/backend TLS remain.

Three final alternating-order debug pairs against the actual L101 base,
microseconds per indexed draw:

| Mode | L101 baseline | L118 on L101 | Medians |
|---|---|---|---|
| Clean | 10.54, 18.72, 12.29 | 11.00, 6.85, 13.38 | **12.29 -> 11.00** |
| Changed sampler before each draw | 42.20, 20.82, 24.00 | 26.84, 20.39, 19.31 | **24.00 -> 20.39** |

Allocations remain **3 clean / 6 changed-sampler**, including COM test buffers.
Ranges overlap considerably under shared-machine load; clean pairs are mixed.
These service-only debug measurements establish no gameplay/FPS or optimized
build speedup. A separate COM lookup microbenchmark measured hash
136.85/134.83/139.75 ns versus dense 56.90/41.35/47.33 ns (medians
**136.85 -> 47.33 ns**); this excludes L101 ext-cache hits.

Final validation:

- `cargo test -p ddruntime --no-default-features --no-fail-fast --
  --test-threads=1`: **177 passed, one failed, three ignored**. All **48** active
  unit tests pass. Only the previously base-reproduced fullscreen gamma Reset
  failure remains. The first integrated run ended with a truncated log around
  the head's pause/memory-pressure interval; the complete retry is reported here.
- **Six** draw/resource tests pass with `DDRUNTIME_VERIFY_DIRTY_FLUSH=1`:
  all 42 reads, duplicate bindings, repeated writes, surface aliases, cube/volume
  resources, state invalidation and immutable queued snapshots.
- `cargo test -p d3d9cmd -p d3d9frame`: **5 + 5 tests pass**, plus doc tests.
- No shard build, GPU execution, game run or original-image comparison.

Private receipts under `C:/ddda-targets/`: `L118-step3-L101-{base,final}.exe`,
`L118-step3-integrated-pairs.log`, `L118-step3-integrated-retry.log`,
`L118-step3-integrated-audit.log`, `L118-step3-integrated-command-tests.log`.
Earlier `L118-step3-final-*` logs are pre-L101 prototype receipts and must not
be used as incremental performance claims for the integrated branch.

Next action: head integrates the two final commits, compares combo9 gameplay
and images, and profiles snapshot/flush plus remaining runtime/lock overhead.
