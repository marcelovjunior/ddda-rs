# L166: Vulkan shadow TDR investigation

Owner: codex-9. Branch: `task/L166-vk-tdr`, base `01c7f74d`
(`task/fps-line`). Direct assignment: investigate c30shadow's NVIDIA TDR,
stress both plain raw Vulkan and the shadow oracle, and fix an evidenced
cause. Do not flip the default before head acceptance. No game/shard runs.

## Results

**Investigation checkpoint, not a TDR fix.** No hang reproduced and no
specific synchronization/lifetime defect established. Production code and
the Vulkan default are unchanged. `run_c32shadow.log` was not available
during these checks; the next action is to inspect that diagnostic run and
correlate its device-loss message with the Windows GPU event. Passing the
bounded tests below does not prove the plain path cannot hang.

Added two ignored headless tests in `crates/ddgpu/tests/vk_encoder.rs`:

- `tdr_pass_mix_stress`: alternating 900 small float-target passes and 15
  1920x1080 color/depth/stencil passes per frame, four encoding workers,
  asynchronous submit, 127-draw submission boundaries, periodic image
  readbacks. Target sizes/counts follow the cap1 census in
  `docs/NATIVE_RENDERER.md`; shaders and contents are synthetic. Every run
  compares its wgpu and raw Vulkan output hashes.
- `tdr_private_capture_replay`: file-backed capture replay with a 128 MiB
  decompression cache, layer enumeration, optional prefix-upload retention,
  optional repetition of the last frame's rendering/query commands, and
  final image hash. Requires an explicitly supplied private capture.
  Repeated frames do not replay uploads/releases or model game evolution.

### Evidence and audit

c30shadow's last complete stats report: 1,487,159 raw passes, 6,279,922 raw
draws, 1,642 matched aspects, zero mismatch/error/skip/pending. The final
aspect receipt is pass 1,496,498, followed by `encode fallback: not Vulkan`
and the stopped backend worker. Prior investigation correlated NVIDIA
`nvlddmkm` event 153, record 2788921, local 11:34:21.022. This establishes
a device/driver failure correlation, not the offending command or its cause.
L164 diagnostic commit `106a53cd` is separate and was not in this base.

Audit of the current source:

| Concern | Observed contract | Remaining limitation |
|---|---|---|
| Shadow ordering | `shadow::prepare` seeds attachments and runs wgpu on shadows before raw; compare commands follow raw in the same ordered submission. | No validation-layer trace of the failing game run. |
| Layouts and dependencies | `Pass::raw_batch` submits a wgpu resource-transition preamble, uses HAL image barriers between raw uses, and restores first-use layouts before returning to core tracking. Whole-texture incompatible usages fall back. | Static review and tested cases are not an exhaustive synchronization proof. |
| Descriptor/resource lifetime | `Ready::submit` holds each `Pass` through `on_submitted_work_done`; draws own binding Arcs, pipelines/layouts, groups, samplers, views and buffers. HAL objects are destroyed at last ownership. | Device-loss teardown was not reproduced. |
| Waits | Raw recording adds no queue submissions or semaphore/fence waits. Mapped-ring reuse drains the CPU finisher then waits for GPU completion before overwriting bytes. Shadow mapping is asynchronous and capacity is bounded. | The existing mapped-ring `wait_indefinitely` can remain blocked if the driver never completes/reports loss; no evidence identifies it as the cause of a GPU TDR. |

The Vulkan loader enumerated NVIDIA and overlay layers but **no
`VK_LAYER_KHRONOS_validation`**. No Vulkan SDK was found in standard SDK
locations or explicit-layer registry entries. Native Vulkan validation was
therefore unavailable; wgpu validation was enabled by the debug build.

### Completed stress gates

Every GPU process was launched only while `pause.py status` reported
`not paused`, checked again while running, with a 60-second kill limit.
Times are shared-host test times, not performance comparisons.

| Fixture | Result |
|---|---|
| Synthetic 64 frames, plain VK | Pass, 3.3 s; wgpu/raw hashes equal. |
| Synthetic 64 frames, shadow every pass | Pass, 15.5 s; 58,550 raw passes/draws, 11,514 matched aspects; 47,400 capacity skips explicitly unverified. |
| Synthetic 512 frames, shadow every 997 | Pass, 15.2 s; 468,470 raw passes/draws, 486 matched aspects, zero skips/mismatches/errors/pending. |
| Synthetic 1,792 frames, shadow every 997 | Pass, 50.0 s; 1,639,670 raw passes/draws, one mapped-ring wrap, 1,697 matched aspects, zero skips/mismatches/errors/pending. |
| Private gameplay slice, 21 frames + 32 repeats, shadow every 997 | Pass, 48.6 s before final hash instrumentation; 50,848 raw passes / 252,840 raw draws, four mapped-ring wraps, 53 query fallback passes, 60 matched aspects, zero skips/mismatches/errors/pending. |
| Final gameplay slice + 32 repeats, plain / shadow 997 | Both pass, 43.9 / 40.9 s, same counters as above and identical final 8,294,400-byte image hash below. |

Existing mapped-constant wrap, mixed raw/query fallback, resource
release/recreation, and 900 dependent sampled float-target passes also
pass in isolated shadow-enabled processes. The first uses interval 997;
the others use interval 1 (capacity skips remain explicit).

Final captured-slice BLAKE3 image hash, identical with plain Vulkan and
shadow enabled:
`88e018e65b5399e24c689006b832892c64cd600ea64b1cce4cd5859c1ec12c59`.
Mapped-ring additive regression hash, identical for wgpu and Vulkan:
`f0ab5a6f57ace5127864caa78176c46c05a03e1c393ba68ea2b83509fa064952`.

Synthetic BLAKE3 image-stream hashes, identical for wgpu and raw Vulkan:

- 64 frames: `b84b609142edc8ad1a7012acc579c9c23010a2891a1f11203ae53555db30260a`
- 512 frames: `4a6f571223d231e79c60a990383bb14b6cba964322f37dc4d845e3d8c884d831`
- 1,792 frames: `c8d98665947af1b92dbcb362e3f53856d266f67c74d1fea74a80fbb9e43c8bb8`

The complete cap1 replay timed out at 60 s during pre-gameplay frame 1,941;
no GPU fault was observed. Two cold slice attempts also hit the timeout
(one while parsing the prefix, one after two rendered frames). These are
incomplete runs, not passes or reproduced TDRs. Subsequent warmed-driver
slice runs finished within the bound.

Private source: `E:/ddda-re/captures/cap1.ddframe`, SHA-256
`003258514187cea1163e9e73505a7c09aafa3bdab3379b6e6e8935514841d1a2`.
Private derived slice: `C:/ddda-targets/L166/cap1-slice7535.ddframe`, SHA-256
`f5ebd06143f4f8f9b36e7847ca1e6629fbb1e8fe08ce8b0fde4f1e78ee0eee6e`.
The CPU-only private helper `C:/ddda-targets/L166-slice.py` retains blob
records without modification, the latest 4,971 surviving uploads before
zero-based frame 7,535 (respecting releases), and commands through frame
7,555 inclusive: 112,326 draws, 318 clears, 168 copies, and 126 query pairs.
Earlier rendering is omitted. This exercises captured shaders/resources
and pass ordering but **does not reconstruct the original frame load
state**. No capture/data bytes are committed. Same-run shadow equality and
plain/shadow final hash equality are separate from original-game equality.

Light validation: ddgpu library **30 pass / one inherited strip-range test
failure / two ignored**; the strip failure is fixed separately by
`ef80de30`, not changed here. Runtime no-default-features suite **241
reported passes / one render-tail timing failure / seven ignored**; the
isolated render-tail retry passes. Reported counts include child-process
summaries. No production runtime changes.

### Reproduction

Set `CARGO_TARGET_DIR=C:/ddda-targets/L166`, `CARGO_INCREMENTAL=0`, and
`CARGO_BUILD_JOBS=4`. Build without launching GPU tests:

```text
cargo test -p ddgpu --test vk_encoder --no-run
cargo test -p ddgpu --lib
cargo test -p ddruntime --no-default-features --no-fail-fast
```

Launch each GPU test executable separately with `--exact --include-ignored
--nocapture`, only when not paused, under an external 60-second watchdog.
Private watchdog/logs are `C:/ddda-targets/L166-run.py` and
`C:/ddda-targets/L166-*.log`. Test controls:

```text
tdr_pass_mix_stress
  DDGPU_TDR_FRAMES=64|512|1792
  DDGPU_VK_SHADOW unset (plain), 1, or 997

tdr_private_capture_replay
  DDGPU_TDR_CAPTURE=<private capture or prepared stress slice>
  DDGPU_TDR_FIRST_FRAME=<optional zero-based prefix skip; default 0>
  DDGPU_TDR_LAST_FRAME=<optional inclusive frame limit>
  DDGPU_TDR_REPEAT=32
  DDGPU_VK_SHADOW unset (plain), or 997
```

The 1,792-frame test approached the hard limit on this machine. A timeout
must remain an incomplete result; do not extend the GPU process lifetime.
