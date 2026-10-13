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
