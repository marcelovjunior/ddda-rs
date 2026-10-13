# L138: production Vulkan pass encoder

Assigned by head to codex-9, 2026-10-13. Worktree `E:/ddda-rs-wt/L138`,
branch `task/L138-vk-encoder`, base `335318fc`. L137 source `02cf25e9`
is cherry-picked as `d0a9a0c4` for its generic ordered encoding pool.

## Scope

Opt-in `DDGPU_VK=1`: encode ordinary prepared ddgpu draws directly through
wgpu-hal's Vulkan (ash) implementation, bypassing wgpu-core's deferred draw
encoding. Keep the original shader translation, rendering state, uniform
ring, resource uploads, queue submission, and wgpu fallback. No game runs,
shard builds, game files, research dumps, or Ghidra modifications.

The head's game A/B should keep the current default
`DDDRAW_NATIVE=drawcall` and toggle `DDGPU_VK`. The separate
`native::Device`/NativeJob recorder remains L141's scope and still uses wgpu.
L143 integration must pass `&key.key` at the Vulkan pipeline registration
site after its `Hashed<PipelineKey>` wrapper is merged.

## Implementation

- `src/vk/objects.rs` owns Vulkan shader modules, pipelines, pipeline layouts,
  descriptor sets, samplers and image views. Pipeline keys are the existing
  `PipelineKey`; Naga receives the identical translated WGSL and hal applies
  the same Vulkan SPIR-V options. Blend/depth/raster/vertex descriptors use
  the existing wgpu types rather than a second handwritten enum conversion.
- The uniform descriptor set references the existing 48 MiB wgpu ring.
  Both stages use the same dynamic offsets and block sizes as the oracle.
  Texture descriptors refer to the actual sampled views, including feedback
  snapshots, sRGB views, cube/volume dimensions, and sampler parameters.
- The producer records owned `Pass`/`Draw` objects only when enabled.
  Clear/copy/query helpers keep using wgpu. L137's pool accepts ordered
  `Chunk`s and returns prepared command buffers plus lifetime keepalives.
- A run of supported passes becomes one raw command buffer, preceded by a
  wgpu `transition_resources` command buffer. HAL barriers order internal
  write/read and write/write dependencies. Final barriers restore each
  texture's initial declared layout so wgpu's tracker remains accurate.
  Whole-texture transitions conservatively reject simultaneous sampled and
  attachment use even when subresources differ. Query passes and multisample
  passes retain the wgpu path. Pipeline/group failures are whole-pass fallback.
- First use of a resource view falls back to wgpu for its initialization
  handling. Initialization becomes globally visible only after ordered queue
  submission; independently encoded later chunks cannot initialize earlier
  work retroactively. Within one ordered chunk it is safe to reuse a preceding
  fallback's initialization.
- A queue completion callback retains all raw objects and their wgpu resource
  owners. Resource release drops cache references, while outstanding batches
  retain their own references. Transient feedback descriptor/view metadata is
  removed after recording. No raw queue submission or raw resource destruction
  occurs while the GPU can still reference it.
- `ddgpu-vk` reports cumulative raw passes/draws, fallback passes,
  initialization passes and encode errors every two seconds at Present.
  `ddgpu::vk::stats()` exposes the same counters for acceptance tests.

## Validation procedure

`CARGO_INCREMENTAL=0`, `CARGO_TARGET_DIR=C:/ddda-targets/L138`,
`CARGO_BUILD_JOBS=4`. GPU processes run only when `pause.py status` says
`not paused`, one test case per process, killed at 60 seconds.

`tests/vk_encoder.rs` uses synthetic DrawCalls, with byte-exact wgpu/raw
readbacks and assertions that raw encoding actually ran. Cases cover plain
draws, u16/u32 indices and strips, dynamic scissor, MRT, depth rejection/write,
blend factors and operations, sampled render targets, 900 alternating tiny
passes, resource release/recreation, and interleaved query/copy fallback with
one/four encoder workers. The ignored `production_cost` benchmark includes
DrawCall preparation, ring upload, deferred encoding and queue submission;
GPU readback drains between samples outside the timer. It is not game FPS
and is not the prefilled raw API floor measured in L133.

## Results

Implemented and tested, awaiting the head's original-game A/B. No game was
run, no lifted shards built, and no install/private research data committed.
L137 is a dependency already on the integration line; cherry-pick the L138
commit only, not the local duplicate `d0a9a0c4`.

Final optimized ddgpu build (`profile.dev.package.ddgpu.opt-level=3`):
**20 CPU tests, 12 focused byte-exact GPU cases, and 2 existing MSAA/depth
fallback cases passed**. Each final GPU process finished within its 60 s
limit. The 900-pass dependent float32 ping-pong fixture exercised **2,698 raw
passes / 2 initialization fallbacks** across three frames; both targets
matched wgpu exactly. Query/copy fallback also matched with one/four workers.

Earlier broader debug regressions passed 21 cases (15 draw, 5 target, L137
parallel oracle). Five other processes reached the hard timeout: cache's
long-lived backend, draw's random-state and render-state sweeps, and target
format/depth-precision sweeps. No failed assertion was observed in those
partial runs; the whole ddgpu suite is **not** claimed green.

Runtime light suite: **199 passed / 2 failed / 4 ignored** initially. The
parallel-present timing test passed its isolated rerun (200 distinct tests
verified overall). The inherited fullscreen gamma Reset test still fails
with HRESULT 2289436780. No runtime code changed.

### Production benchmark

Three complete runs, each reporting the median of nine measured frames
after three warmup frames. Values below are the median of those three run
medians, in microseconds per draw/pass. These include frontend preparation,
uniform upload, encoding and queue submission; they are not raw API costs.
The shared machine produced wide timing ranges.

| Synthetic workload | wgpu | DDGPU_VK=1 | Interpretation |
| --- | ---: | ---: | --- |
| 5,000 draws, one pass/pipeline | 3.004 | 2.803 | Mixed individual runs; no clear draw-heavy gain |
| 5,000 draws, 40 pipelines, 200 sampled sets | 4.191 | 4.236 | No clear draw-heavy gain |
| 900 draws, 900 passes | 21.536 | 6.722 | 3.20x lower total CPU cost per pass |

Individual tiny-pass run medians: wgpu **21.710 / 16.808 / 21.536**;
Vulkan **6.592 / 6.722 / 6.977** us/pass. A separate profiling run measured
900-pass finisher CPU **12.101 -> 2.615 ms**, with total **19.837 -> 6.483
us/pass**. Profile counters also include the between-sample readback helper;
readback itself is excluded from the benchmark timer. No game FPS claim.

SHA-256 of byte-exact readbacks (same for both paths):

| Fixture | SHA-256 |
| --- | --- |
| Plain / u16/u32 indexed / u16 strip | `97bf0791bc375c4006e31fb0d9ce5b28c56598435f0d2ec36e4dc5deff84931e` |
| u32 strip/scissor | `196684ff4cf19d03766baeeb03cc98dfc68c7f43c032bec0b8021fbb4a09851d` |
| MRT/depth | `61322cdc82c18e06be814c5dd36e61cbdaded5bf8291e314c77260ec38282a5a` |
| Alpha blend | `230bba4cd1f9aa979eeb94e992546e6105fd547ee7ff9b009e06c77836fdd17f` |
| Constant/separate alpha | `cc63d861f29a93da668957aaac7200c1bbc4b9347cec10080375038781e89a20` |
| Reverse subtract | `0495baf2ce20206e77c4634bf2792ee56c1202f3e36a1ef656efb7971a1980c0` |
| Dependent float32 sampled targets (512 bytes) | `8c0b60b4289cc3eb5ed326d8697b80d331da37bab674b57dda1923fe4bd980cb` |
| Benchmark one pipeline | `120322b15e69f1243bae28f95644b7276d368c2bbddec86e72914546e8cc6160` |
| Benchmark 40 pipelines / 200 sets | `1e3a9ab2a631563f429d70cc6e061b3a6a20f329890c534125e09a2d4d07d1d3` |
| Benchmark 900 passes | `1ac0a064b588aeaedd03c1e34efb91b237938b411c5f501310adb6f556312045` |

Local logs: `C:/ddda-targets/L138-final-*.log`, `L138-msaa-*.log`,
`L138-shaped-bench-{1,2,3}.log`, `L138-bench-profile.log`,
`L138-runtime.log`, `L138-runtime-rerun.log`. Reproduce the benchmark with
`cargo test -p ddgpu --test vk_encoder --config
"profile.dev.package.ddgpu.opt-level=3" -- --ignored --exact production_cost
--nocapture --test-threads=1` under the environment and pause/timeout rules
above. Run focused cases individually with `--exact <case>`.

Next action: head integrates with L143's `&key.key` adjustment, then runs
paired game image/FPS captures with default `DDDRAW_NATIVE=drawcall` and
`DDGPU_VK=0/1`, checking raw/fallback/error counters and finisher timings.
