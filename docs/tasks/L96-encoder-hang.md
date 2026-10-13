# L96: encoder clear/copy hang

Owner: codex-9. Branch: `task/L96-encoder-hang` (base `563be874`).

## Brief

Find why the L88 whole-target clears and L95 same-size StretchRect copies
recorded in the open encoder make `ddgpu/tests/cache.rs` spin. Preserve
batched clears/copies and their `DRAWS_PER_SUBMIT` accounting. Scope:
`crates/ddgpu/src/lib.rs`, focused cache regression, this brief and milestones.

Use the light runtime suite and ddgpu tests in `C:/ddda-targets/L96`, four
build jobs and no incremental compilation. Never run the game or build
lift shards. Check the shared pause state before GPU tests; cache tests
have a 300-second hard timeout, other GPU tests 60 seconds. Per head board
#10360, serialize GPU suites with `HEAVY_BIG=1` through `heavy.py`.

## Results

### Cause and change

The cache hang reproduces with unchanged production code and both original
cache tests running concurrently. After 175.44 seconds the process was
stopped deliberately (300-second watchdog installed); neither test finished.
An eight-second stack profile followed by an all-thread sample found the
busy thread named `wgpu-hal WGL Instance Thread` in driver thread shutdown,
repeatedly querying a thread's exit status. The two test threads were blocked
in Vulkan device creation and render-pipeline creation, respectively.
There was no sampled encoder/readback loop. Host: NVIDIA GeForce RTX 4060,
Windows driver 32.0.16.1062.

`InstanceDescriptor::new_without_display_handle()` defaults to all backends,
so every fresh headless backend also initializes OpenGL/WGL even when the
selected adapter is Vulkan. `headless()` now defaults to `Backends::PRIMARY`
(Vulkan/DX12/Metal on native hosts), retaining `WGPU_BACKEND` as an explicit
override, including GL. Validation flags and adapter selection within that
set are unchanged. The stack evidence identifies driver initialization/
teardown concurrency, rather than establishing an L88/L95 encoder defect.

Whole-target clears and same-size copies still use the open encoder and
count toward `DRAWS_PER_SUBMIT`. A focused private-state regression checks
two submit-limit crossings, pending resource tracking, and clear/copy/upload
pixel ordering. The two existing cache tests remain unchanged and run with
their normal concurrent test execution.

### Verification

- Light `cargo test -p ddruntime --no-default-features`: exit 0, all suites
  passed, one ignored microbenchmark. No shard build or game run.
- GPU validation of the fix: pending the serialized machine slot.
- Existing dead-field/unused test-helper compiler warnings remain.

No game FPS or comparison against the original was performed by this agent.
Head owns those measurements. Next: complete the GPU gate and deliver the
minimal headless fix, preserving the L83 `WGPU_BACKEND` integration hunk.
