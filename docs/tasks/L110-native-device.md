# L110: native device and backend floor

Owner: codex-9. Branch: `task/L110-native-device`, base `task/fps-line`.
Scope assigned in the session: ddgpu immutable state objects, handle-keyed
pipeline/texture caches, recorder, benchmark and D3D9 image equivalence.
No game or shard build/run. See AGENT-RULES.md and NATIVE_RENDERER.md.

## Head's first checkpoint (board 10652, 10685)

Measure wgpu's floor before selecting the native implementation: direct
recording, reusable render bundles, and raw Vulkan; also isolate pass cost.

`examples/native_draw_cost.rs` precreates 40 pipelines and 200 uniform
descriptor groups. Every draw binds pipeline, group plus dynamic offset,
vertex/index buffers, then draws one indexed tiny triangle. Constants are
prefilled with distinct values; copying/updating them is **excluded**.
Groups contain uniform bindings, **not sampled texture sets**. This floor
does not replace the requested game-shaped native API benchmark.

Each measurement includes encoder creation, recording, ending the pass,
finish, and queue submit. Fence/device completion waits are outside timing.
Three warmup frames precede 20 measured frames. Reported components come
from the same median-total frame. Bundle construction is separately timed;
replay keeps the command sequence and offsets fixed. Raw ash resources are
private, allocated on the same Vulkan device, with explicit pass dependencies
and a fence before reuse. No direct raw access to wgpu-owned resources.

## Results

### Backend floor, 2026-10-13

Release/optimized, RTX 4060, NVIDIA 610.62, Vulkan, 16x16 RGBA8 target.
Each of the three invocations ran under `heavy.py`, `HEAVY_BIG=1`, with
pause polling and a 60-second hard cap. All completed successfully.

| Draws / passes | wgpu direct | wgpu reused bundles | ash Vulkan |
|---|---:|---:|---:|
| 5,000 / 1 | 0.884 us/draw | 0.386 us/draw | 0.326 us/draw |
| 900 / 1 | 0.924 us/draw | 0.423 us/draw | 0.343 us/draw |
| 900 / 900 | 7.146 us/pass | 7.151 us/pass | 1.351 us/pass |

For 5,000 draws, direct wgpu recording/finish/submit: 0.178/0.673/0.033
us per draw. Bundles: 0.001/0.356/0.029; raw Vulkan: 0.298/0.000/0.028.
Bundle construction: 0.501 us/draw, excluded from repeated replay timing.
Difference between 900-pass and one-pass frames, divided by 899 added
passes: approximately 6.23 us wgpu, 6.74 us bundles, 1.01 us raw Vulkan.
This is a same-target clear-pass floor; real ping-pong textures, barriers,
sampling and larger targets can cost more. It does not prove the game's
dependent simulation passes can be merged or replaced with compute.

Reproduce (release binary; serialize GPU access and bound each invocation):

```
native_draw_cost 20 5000 all 1
native_draw_cost 20 900 all 1
native_draw_cost 20 900 all 900
```

Implemented/compiled: state-object API and native recorder (pending GPU
verification at this checkpoint). No original-game comparison or FPS claim.
The floor uses WGSL generated in the example, no game shader data.

Light runtime `cargo test --release -p ddruntime --no-default-features
--no-fail-fast -- --test-threads=1`: **152 passed, 1 failed, 1 ignored**, 23
suite results. Existing `d3d9_device::a_gamma_ramp_is_shown_on_a_full_screen_swap_chain_only`
fails at line 135: Reset to full screen returns `0x8876086c` rather than 0.
No runtime source edits; test unchanged. The original release build was
terminated without a Rust diagnostic (`0xffffffff`); serialized retry built
successfully. All build products are under `C:/ddda-targets/L110`.

Next: native GPU equivalence and game-shaped native recorder timing after
head's combo4 game-run hold; choose backend from the measured floors.

### Native wgpu device checkpoint

Head selected wgpu first (board 10760), retaining a later Vulkan option.
`ddgpu::native` provides device-scoped typed handles for blend, rasterizer,
depth/stencil, sampler, input layout, shaders, buffers, textures and views.
Pipeline creation interns the state handle tuple plus attachment formats,
sample state and topology. Shader translation links semantics and uses L83's
IR cache. Compatible pipelines share texture layouts and interned texture
sets. Recorders only bind handles, filter redundant state, support instance
ranges, and own separate encoders/rings so recording can move to workers.
There are no D3D9 render-state numbers in the recording API. Descriptors
currently reuse wgpu's portable value enums; object ownership/recording is
behind native handles, while resource import is explicitly a wgpu bridge.

Full block updates write only changed blocks; byte-range updates coalesce
until the next draw. Submission generations preserve retained constants;
bounded-ring failure preserves the previous snapshot and can be retried
after submission. CPU delta writes still require full GPU blocks with this
base's shader ABI. Imports retain resources; replacement/release in the D3D9
backend does not retarget existing handles. Native work must be submitted
before switching producers. Arenas retain their objects until Device drop.

Verified optimized Vulkan, 2026-10-13:

- **2 native GPU tests pass** (1.03 s and 1.55 s): eight combinations of
  indexed/nonindexed, blend, scissor match the existing backend AND host
  D3D9 pixels exactly; imported texture sampling, compatible-layout sharing,
  constant generation reuse, redundant bind suppression, ring exhaustion
  and coalesced range updates also pass.
- **3 ddgpu CPU unit tests pass**. Runtime result remains 152/1/1 above;
  L118 independently reproduced the gamma failure on its pristine base
  (board 10844). No D3D9 implementation changes beyond module export.
- `native_draw_cost 20 5000 native 1`: **6.538 us/draw total**, 2.149 recording
  and 4.389 finish/submit/upload. 40 pipeline handles, 200 distinct sampled
  texture sets, both constant blocks changing every draw, minimal synthetic
  shaders and 16x16 target. 115,000 draws / 230,000 uniform writes including
  three warmups. **The <2 us target is not met.**
- Same-window existing `draw_cost_game 5000 10`: 12.43 us/draw submitted,
  8.24 deferred recording, 105.95/129.49 ms with GPU waited. This benchmark
  has larger shaders, six textures and larger targets, so these numbers do
  **not** establish a matched-workload speedup.

These later GPU runs used the head's explicit board 10835 exception to the
heavy wait: run directly beside head-o1i's CPU-only build, Vulkan, 60-second
watchdogs. Timings are CPU-contended and need a quiet repeat after compact
constants are integrated. No game run or original gameplay comparison.

### Vulkan interoperability feasibility

The compiled/readback probe `native_draw_cost 1 1 interop 1` **passes**.
It initializes wgpu-owned resources, declares COPY_DST transitions in one
wgpu encoder, records an ash texture clear and hal buffer clear in a second
raw encoder, then uses a third wgpu encoder to copy/read them back. The
pixel is exactly red and the buffer exactly zero; one ordered wgpu submit
owns synchronization. Resources remain alive through completion.

The initial single-encoder probe failed with wgpu-core's explicit panic:
mixing raw and wgpu encoding APIs is forbidden. Separate encoders are
required in pinned wgpu 29.0.4. `transition_resources` is documented for
interop; raw commands alone do not inform wgpu's initialization/lifetime
tracking. `Texture::as_hal` exposes VkImage, but Vulkan Buffer's VkBuffer
field is private in this version. Public hal encoder methods accept its
guarded buffer; ash descriptor/vertex binding directly to wgpu buffers
would need an accessor patch or separately owned/imported buffers. The
probe demonstrates transfers, not a production mixed-rendering backend.

Next: adopt the already integrated L90 compact shader ABI from fps-line,
add compact native constant layouts and repeat image/timing checks.
