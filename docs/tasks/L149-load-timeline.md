# L149 loading timeline

Owner: codex-6. Branch: `task/L149-load-timeline`, from `task/fps-line`.
Head assignment: loading timeline, existing-run attribution, then an exact,
opt-in attack on the largest measured cost. No skipped content or changed
input/timing; only head runs the game. L145 merged as `300edf2d`, preserving
both sides' milestone entries and retaining L140 once.

## Implementation and evidence

`DDRUNTIME_LOAD_LOG=1` now writes one `event=interval` summary at each
observed transition. It keeps the Windows process-creation origin already
used by the logger. Each summary has `from`, `to`, process time, interval
wall nanoseconds and independently sampled counter deltas. The ordinary
periodic cumulative records remain available.

- Lifecycle: main entry, GPU/runtime initialization, guest entry/exit,
  first Present, movie reader start/end. Movie reader lifetime is not an
  exact visible-video or input-acceptance interval.
- `aTitle` slot 7, **0x005317B0**, reads its callback at **+0xF0**. The
  wrapper reads it once at dispatch entry and logs changes. Labels from
  `GAMEPLAY_PATH.md`: **0x00531800** press-start state, **0x005325E0** or
  **0x00531A20** menu flow, **0x00532110** save-load request flow,
  **0x005330E0** game-main handover. Other callback addresses remain raw
  `title_state` IDs. These are code states, not confirmed visible screens.
- `sArea::vf10`, **0x00DB4C10**, takes the requested DTI at `[esp+4]` and
  stores it at `this+0x3828`. The wrapper logs the request, never labels it
  as a completed load. The DTI registry identifies the owning class.
- `aStage` slot 7, **0x00504330**, switches on the low byte of **+4**;
  the low two bytes identify mode/substate, while **+6** is a countdown
  in 0x0050ADC0. Only mode/substate changes trigger summaries; the raw
  word is retained in details. Stage number is **+0x724**. `first_stage_present` is the first Present after
  this update begins, including its initialization states.
- Ghidra **0x0050ADC0**, case **0xE**, completes its load sequence by
  storing **+4 = 2**; **0x00504330** dispatches that mode to
  **0x0050B870**. `first_gameplay_present` means the first Present after
  observing mode 2. It explicitly says `player_control=unverified`: events,
  fades and input blockers can remain. Present markers are producer
  boundaries, not GPU/display completion fences.

All addresses are DDDA.exe 2017-12-13 Steam. Ghidra was read-only. Wrappers
call the previously selected override/body exactly once; no behavior was
translated or replaced by a state-machine guess. No post-call object read
is made because a callback could destroy its own object. Transition
formatting parks guest x87 and preserves the exact incoming MXCSR.

### Counter definitions

- `cpu_{main,render,jobs,loader_io,host_main,backend,other}_ns`: Windows
  `GetThreadTimes` kernel+user CPU. Guest workers register by the creation
  entries already evidenced in `RUNTIME.md`: DAC090 render, DBDEF0/DBDF10
  jobs, DBA9A0/DB9F40 resource I/O. Other entries stay `other`; guest TIDs
  are not guessed from one run. Real query handles retain exited threads'
  final totals. Toolhelp discovers host threads at transitions; descriptions
  beginning `d3d9-backend`/`wgpu-` identify backend workers. Very short host
  threads between snapshots can be missed; guest workers register at entry.
  CPU is coarse OS accounting, not an exclusive function profiler.
- `reads/bytes/read_ns` retains all file-read service accounting;
  `archive_io_reads/bytes/ns` restricts it to opened read-only `.arc`
  handles, independently of read-ahead. Service time includes guest copy
  and locking, not just kernel disk latency. `archive_reads/archive_ns`
  remains the wrapped decompression-stream boundary, a different metric.
- Inclusive zlib calls/ns and fast/Adler/window ns; archive-nested inflate,
  geometry, Vorbis and open times remain separately visible. Inner helper
  time is already inside outer zlib time. These numbers must not be added.
- Shader modules, translation and pipeline counts/ns are fetched directly
  from ddgpu's atomic totals at the transition through a registered function
  pointer, not a delayed 1 s copy. Six timing-hook lines add the previously
  missing native `Device` module/pipeline creations to those global totals.
  Raw Vulkan Context shader/pipeline creations also contribute, with
  cache hits excluded (owner delegation: board #12490). With DDGPU_VK,
  totals include both real wgpu fallback and raw-object creations.
- Texture image-upload count, bytes and CPU copy/enqueue time, excluding
  buffer uploads. This does not measure GPU transfer completion or worker
  texture conversion. All normal runtime image uploads use `upload_reads`.
- `emu_engine_ns`: completed Unicorn execution slices, including its hooks,
  excluding adapter setup and native callbacks between slices. Availability
  is explicit when the emu feature is absent.
- Service elapsed time is sampled once per 256 dispatches **per thread**,
  with sample count and raw sample ns. It includes waits and can alias a
  repeated call pattern; it is not extrapolated or subtracted from CPU.
  Direct lifted calls cannot be exhaustively timed cheaply, so
  `lifted_ns=unavailable` is explicit.

Counters accrue on completion. An operation spanning a transition belongs
to the later interval; concurrently sampled atomics are not a global freeze.
Logging adds work, especially at many state transitions. Use matched
logging-on runs for attribution, then logging-off runs for speed acceptance.
The logger is process-wide, matching the existing telemetry.

## Existing logs: estimates, not semantic UI measurements

`scripts/load_timeline.py` keeps its existing Markdown, legacy-clock and
head `--mark` interfaces. New `--at 0,9.257559,...` emits JSON counter
deltas with each cumulative sample's age and the source log SHA-256.
Missing baselines stay unavailable. Helpers are separate nested counters,
not phases. Twelve parser tests cover old interfaces and interval behavior.

Primary evidence: `E:/ddda-re/notes/runs/run_c23def.log`, SHA-256
`c0522749c0d3870a10f3058e7c1a126c0919021d0eede7174159bdf5adcfb64d`.

| Observed interval | Wall seconds | Measured costs / qualification |
|---|---:|---|
| Process to first Present | 1.826 | GPU init 0.633; runtime init 0.080; explicit geometry 0.075. The last periodic sample precedes initialization: zero counters there do **not** prove zero work. |
| First Present to movie reader start | 7.431 | Through the latest sample, cumulative zlib 1.009 s, reads 0.074 s, pipelines 0.028 s. Counter deltas also include unsampled work before first Present. |
| Movie reader lifetime | 6.714 | Zlib nearly unchanged; Vorbis 1.890 s, reads 0.042 s. Preserve original timing and first accepted Enter behavior. |
| Movie end to 32.181 s probe | 16.209 | Menu/storage/UI flow **not disambiguated** in old log; zlib 0.337 s, Vorbis 5.658 s, reads 0.007 s, pipelines 0.116 s. No claim that background Vorbis is the critical path. |
| 32.181 to 45.474 s probes | 13.292 | Resource-loading estimate: zlib **6.660 s**, fast **4.133**, Adler **0.560**, window **0.390**; 291.37 MB reads **0.634 s**; pipelines **0.388**, modules **0.188**. Save-load start itself was not marked. |
| 45.474 to 47.135 s probes | 1.661 | First gameplay-scale draw interval: pipelines **1.207 s**, modules **0.558 s**, no further read/zlib completions in the sampled totals. Parallel costs can exceed wall time. |

The last two draw probes are 351 and 3,454 draws/frame. This is only a
workload hint, not a frame-exact loading-end or player-control marker.
Old counters were sampled about once a second, independently of the probes.
By 47.135 s, totals are zlib **8.007 s** (fast **4.928**, Adler **0.722**,
window **0.467**), reads **0.757 s**, pipelines **1.763 s**, modules
**0.822 s**, and background Vorbis **7.752 s**. Do not add nested costs or
interpret a cross-thread inclusive sum as wall savings.

Ranked actionable evidence for this default run:

1. **Decompression**, especially fast decoder CF2CB0 in the resource-load
   interval. The prior exact L145 candidate is the selected intervention.
2. **Pipeline/module creation at first heavy rendering**, after reads and
   inflate stop. Keep cache directory/temperature identical in comparison;
   timeline CPU classes will show whether compilation overlaps other work.
3. **File-read service cost**, well below decompression here. Existing
   opt-in read-ahead is not supported as the first intervention by this log.
4. **Unattributed UI/waits and background audio**: large elapsed intervals
   cannot be assigned to an optimization without state markers/thread CPU.

Cross-checks (different run options, **not** controlled speed comparisons):
`run_c23sim.log` first Present **1.664 s**, movie **9.149–15.865 s**,
gameplay-scale probes **48.699–50.366 s**; zlib **9.776 s**, reads
**0.540 s**, summed pipelines **47.956 s** by the latter probe.
`run_c23cen.log` first Present **2.435 s**, movie **9.002–15.726 s**,
gameplay-scale probes **48.547–50.208 s**; zlib **8.153 s**, reads
**0.606 s**, pipelines **7.056 s**. Allocation census overhead makes that
run unsuitable for a clean loading A/B.

Their SHA-256 values respectively:
`1fdc5263a15018272510f20ea0150149d427e1179e5029bbbf48c83030f9a6ae`,
`54296d9016bf3298d919f698dc742ec4b63e9529a11380528920c63c81b1a5f6`.

## Exact change selected and requested head run

L145 `0ebeced0` is merged here. `DDRUNTIME_ZLIB_FAST=1` selects its exact
CF2CB0 page-access/ordered forward-copy specialization. It changes no
content, stream ordering, state-machine behavior, waits, movies or input.
Prior gates: **217 streams / 12,893 original-instruction returns** for
fast-only and combined candidates; **632 leaf cases x3** configurations.
Real ARC whole/chunked microbench **1.59–1.69x**, not whole-loading speed.
The independent outer port remains off: its large-buffer measurements were
mixed. Full conditions and hashes are in `L145-inflate.md`.

Head should use its next integration exe containing this branch, same save,
resolution, pipeline-cache directory/temperature, and original autoplay
route (Enter at first accepted intro input). Two matched runs:

```
DDRUNTIME_LOAD_LOG=1
DDRUNTIME_ZLIB_FAST=0                 # repeat with 1; only changed option
DDRUNTIME_ZLIB_PORT=0
DDRUNTIME_ARCHIVE_INFLATE=1
DDRUNTIME_ZLIB_ADLER=1
DDRUNTIME_CRT_SCRATCH=1
DDRUNTIME_FILE_READ_AHEAD=0
DDGPU_NATIVE_PIPELINE_CACHE=1
DDDRAW_NATIVE=drawcall
DDGPU_VK=0
```

Keep allocation stack census disabled. Record the visible press-any-button,
main menu, Load Game acceptance and first controllable frame, to correlate
the new code-state boundaries. Compare boot-to-menu, save request to
first-gameplay Present, per-class CPU, inclusive fast/outer time and pipeline
creation. Follow with logging-off timing acceptance. No agent game run. The new `run_c24def.log` has only legacy autoplay-clock
probes, with no process-clock load-log records; it cannot replace c23 for
this interval attribution.

## Results

Implemented: timeline/state wrappers, CPU/counter interval accounting,
native module/pipeline timing hooks, historical parser extension and ranked
analysis; exact top-item candidate adopted from L145.

Final logging-on light suite: **212 passed / one inherited fullscreen-gamma
failure / 5 ignored**, including **65 unit passes**. It exercises wrappers,
interval arithmetic, Windows thread CPU snapshots, MXCSR preservation,
archive services, dispatch and the inherited L145 gates. Twelve parser tests
pass, retaining all nine earlier tests. An early wrapper-fixture failure
was fixed by supplying the observed title/stage fields and asserting they
stay unchanged. No production checks were weakened.

Light-suite log `C:/ddda-targets/L149-final-light.log`, SHA-256
`a8ae1ce119efe607aea45d34398a7b366e76038093f4456af27e9008a85e6d94`.
The initial head pause interrupted a pre-final run; it is not counted as a
completed full suite.

Frontend type-check **passed** in 68 s using the real ddplay main source
and a private `C:/ddda-targets/L149/frontend-check/Cargo.toml` with
`ddruntime.default-features=false`. This checks ddgpu/native/raw Vulkan and
the frontend snapshot hookup without building any shards or running an exe.
Log SHA-256: `387a2e6fae06877ac8421f2e4237629966071f187930b68574b0c8073221e5ce`.
`cargo check -p ddruntime --no-default-features --features emu` also **passed**
(42.49 s), covering the actual engine-slice timing hook. Log SHA-256:
`226f1d6a032599df7845e0106280e0434dbf8009b278f1e26ac172e2dcd477e6`.
All cargo commands used `CARGO_TARGET_DIR=C:/ddda-targets/L149`,
`CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`. GPU rendering was not exercised;
only counters were added to those creation paths. Game-feature integration
and head visual/timing comparison remain the integration lane's checks.
No game files, dumps, shard builds, game/GPU runs or pushes by this agent.

Not compared against the original game: new state-marker visibility/input
correlation, logging overhead and end-to-end loading gain. Those require
the requested head runs. Next action: integrate and run the matched FAST A/B.
