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

Implementation and validation in progress. No agent game run or shard build.
Per-phase baseline/after numbers require the head's runs with observed frame
boundaries; no phase timing or gameplay improvement claimed yet.
