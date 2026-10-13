# L116: shader and pipeline loading stalls

Owner: codex-11. Branch: `task/L116-pipeline-warmup`, base `task/fps-line`.
Scope: ddgpu pipeline/module telemetry, persisted recipes and background warmup;
ddplay startup hook, reporting script. L109 owns runtime file I/O/archive inflate.

## Measurement

Set `DDGPU_PIPELINE_LOG=1` and capture stderr. Each `DDGPU_PIPELINE` line
records kind, cache disposition, backend Present count, phase, milliseconds
and host thread name. Module/pipeline events cover draw and helper creations.
`object-miss` means a new wgpu object; wgpu does not expose the driver's
individual cache-hit status. IR events distinguish disk hits and misses;
their times include cache I/O and parsing. Translation is a separate event.

Frame zero precedes the first backend Present. Supply observed boundaries as
`DDGPU_PIPELINE_PHASES=0:boot,100:title,200:menu,300:loading,400:gameplay`
(these are syntax examples, **not measured game boundaries**). Without them,
frames after zero are `unknown`. Repeated loading/gameplay ranges are allowed.
Boundaries can also be applied after a run:

```
python scripts/pipeline_summary.py run.log --phases 0:boot,100:title,200:menu,300:loading,400:gameplay
```

The CSV gives count, total and maximum milliseconds by phase/kind/cache.
Summed call wall times are not elapsed load time or FPS. Background work can
overlap. Only one game backend is expected; frame counts are process-wide.

## Thread audit

`ddruntime::d3d9draw::set_backend` wraps the backend in `DeferredBackend`
unless its existing synchronous diagnostic option is selected. The queue's
`d3d9-backend` consumer executes draws; `WgpuBackend::derive` calls `module`
and pipeline creation there. Parsing, translation, IR I/O and module creation
are therefore already off the guest render thread. The bounded queue can
still backpressure that producer when compilation stalls the consumer.
Dropping commands or using placeholder shaders would change the image and
is not an acceptable way to hide a cold miss.

## Results

Telemetry checkpoint: **2c12eab2**, five initial ddgpu unit tests and a parser
regression passed. The head was asked for baseline logs in board messages
10815/10819 and for the paired run in 10846/10863.

Background warmup implementation: **aeb8e5df**. `ddplay` starts it after enabling the
existing driver cache. `DDGPU_PIPELINE_WARMUP=0` disables speculative
compilation while still recording recipes (the baseline/learning mode).
`DDPLAY_PIPELINE_CACHE=0` disables both existing driver persistence and warmup.

- The checksummed/versioned `.keys` manifest sits beside the driver cache.
  Its namespace includes the **BLAKE3 hash of the player's executable**,
  wgpu's adapter/driver key, enabled device features/limits, pinned wgpu
  version, and a source fingerprint of the translator/key/descriptor code.
- Recipes contain every `PipelineKey` field, both generated-source digests
  and both sampler layouts. Generated WGSL is deduplicated in `sources/`;
  the existing L83 cache supplies exact Naga IR. These are local runtime
  cache artifacts, never repository data. The manifest has a 16 MiB and
  16,384-entry resource cap; source reads have a 4 MiB cap.
- One background thread hashes the exe, reads/writes recipes and sources,
  creates shader modules and compiles prior pipelines with the same device,
  driver cache and shared descriptor/layout factories as the draw path.
  Writes use unique temporary files and atomic replacement. Manifest writes
  are coalesced to once per second and acknowledged at host-close flush.
- The draw consumer only polls completed objects, never joins/waits for
  warmup. Both generated-source digests must match the **current** translated
  modules before a warmed pipeline is adopted. Same-token shader variants
  cannot silently reuse an earlier run's input/link translation. A missing,
  corrupt, stale or unfinished recipe follows normal validated creation.
- Translation for current reflection/interface information still occurs on
  the backend worker. Module hits avoid IR I/O/device module creation there.
  Fixed helper pipelines are logged but are outside the `PipelineKey`
  recipe set. Unknown first-use pipelines still cost compilation and can
  indirectly backpressure the render thread through the existing bounded
  queue. Startup scheduling cannot guarantee all recipes finish before use.
- wgpu 29.0.4's `Device::push_error_scope` explicitly uses a thread-local
  scope stack with `std`; the warmup worker's validation scopes therefore
  do not intercept the backend worker's errors.

Validation (GPU checks paused during the head's runs, then resumed after
message 10874):

- **Ten CPU units passed**: key round trip, identity/version/corruption,
  atomic file replacement and executable/driver namespaces, source-variant
  rejection, exact phase intervals, existing IR tests, shader identity and
  constants. **Two Python parser tests pass.**
- **Three Vulkan GPU cases passed**, through `heavy.py`, `HEAVY_BIG=1`,
  four build jobs, a 300 s build cap and **60 s per case**: persisted warmup
  pixel equivalence (0.88 s), shared render passes (7.57 s), long-lived vs
  fresh backends over 120 frames (27.78 s). A subsequent warmup-only check
  after write-coalescing/feature-namespace changes also passed. The final
  source-guard/per-backend-counter warmup replay passed in **0.46 s**.
- The synthetic 12-draw case restores **12 pipeline objects and four
  shader modules**, uses all twelve warmed pipelines, creates **zero draw
  pipelines/modules on demand**, and compares both target images byte for
  byte. Per-backend creation counters avoid interference from other tests.
- Timing examples, **not game phases or a controlled driver-cold benchmark**:
  the first synthetic sample spent 89.269 ms creating twelve on-demand
  pipelines, then 3.832 ms recreating them in the background; a repeat was
  16.446 / 6.038 ms; the final guarded replay was **11.528 / 3.816 ms**.
  Driver/global cache residency and machine load affect
  these numbers. The robust result is zero compilation on the repeat draw.
- Runtime light suite: **156 tests pass across invocations, one ignored**:
  29 unit tests in the serial invocation plus all 127 integration tests in
  21 executables using `C:/ddda-targets/L111/display-L34.txt` (recorded L34
  display). Initial/recorded full-suite attempts hit the existing variable
  Present-lock timing assertion. The serial units passed, then the current
  desktop rejected the gamma test's fullscreen Reset; the recorded display
  resolves that test. One integration cargo invocation ended abruptly with
  exit 1 and no failure diagnostic during Reset; every integration binary
  subsequently exited 0 under a 120 s per-binary cap. This is **not** a
  claim of one all-green full-suite invocation. No runtime source changed.

### Existing head-run baseline evidence

The head supplied `E:/ddda-re/notes/runs/instr1.log` (combo5, before L116).
L109's cumulative counters at 175.271 s show **552 pipelines / 1,526.307 ms**,
**776 modules / 968.322 ms**, including **241.424 ms translation**. The main
105.119-108.126 s burst contains 441 pipelines / 706.421 ms and 642 modules /
825.156 ms. Later, 150.255-151.256 s contains **9 pipelines / 386.270 ms**,
and 160.262-161.262 s contains **2 / 163.885 ms**. These one-second deltas
show late compilation costs but cannot identify individual pipeline misses
or UI phases. Private extraction: `C:/ddda-targets/L116/instr1-gpu-buckets.json`.
No corresponding L116-enabled run has been supplied yet.

No agent game run, shard build, Ghidra mutation, game-data commit or push.
Private test logs: `C:/ddda-targets/L116/` and adjacent `L116-*.log`.

### Head's paired game measurement (outstanding)

1. Integrate the completed L116 changes and build on the head's integration
   line. Use a private `DDPLAY_PIPELINE_CACHE` directory; keep identical
   settings/save/route between runs.
2. Run with `DDGPU_PIPELINE_LOG=1 DDGPU_PIPELINE_WARMUP=0` through boot,
   title/menu, load and gameplay. This records the complete recipe set.
   Close normally so `save_pipeline_caches` acknowledges recipe writes.
3. Repeat once with warmup still off for a **driver-cache-warm baseline**,
   then with `DDGPU_PIPELINE_WARMUP=1` using that same cache. This separates
   precreation from L71's existing driver-cache benefit. Keep logs separate.
4. Supply observed Present boundaries for each run to the summarizer;
   frame thresholds are route/run-specific. Compare on-demand pipeline and
   module totals by phase, background work separately, warm-hit counts,
   actual elapsed load time, and images. Do not sum overlapping CPU calls
   into elapsed loading time.

Per-phase baseline/after table remains **unmeasured** until those logs arrive.
One next action: the head's paired game run; no gameplay FPS/loading-time
gain is claimed yet.
