# Milestones

### L175 semantic material and package leaves (2026-10-13)

- Opt-in EA23F0/FED190/E0F3C0/01008200 native semantic ports: **2,226
  original returns** pass ABI replay; EA23F0 reaches **712/712 decoded
  instructions**, both growth sites, NaNs and unaligned/aliased inputs.
- Seven isolated tests pass, including heap-corruption rejection, opt2 lifted bodies and serial/atomic
  Pages. Runtime light **255 pass / zero failures / seven ignored**.
- Median paired synthetic gains EA23F0 **1.512x**, E0F3C0 **2.734x**,
  01008200 **1.141x**; some individual samples regress, FED190 mixed
  across runs. mtcore/ddports **26 pass / zero failures / nine ignored**.
  No game/image/FPS claim. Details: `docs/tasks/L175-semantic-draw-c.md`.
- Next: head accumulated train with `DDRUNTIME_SEMANTIC=1`, then real
  job/image acceptance. Virtual/growth callees remain recorded mock seams
  in isolated tests; full gameplay comparison remains open.

### L162c job distribution and host scheduling (2026-10-13)

- c31 h0 150-frame census: main 65,186 callbacks, six workers
  88,096 / 78,545 / 64,249 / 55,879 / 52,116 / 42,930. Main final-callback
  solo tails add 2.661 ms/frame before its joins. Worst main/worker tails
  start within 92..135 us but run 14.65..18.51 ms; late queue ordering is
  not established as their cause. Shared atomic FIFO already distributes work.
- Independent priority/ideal-processor opt-ins forward original Windows
  requests previously ignored. Foreground requests CPU hints only; background
  requests priority -1. No invented boost, affinity pin or callback reordering.
- 19 parser and four host scheduling tests pass, including suspended guest
  startup in both modes. Light 254 reported passes / zero failures / seven
  ignored before final lifecycle refinement; final focused four pass.
  No game/FPS gain measured. Details: `docs/tasks/L162-job-wake.md`.
- Blocker: no post-fix p8 trace yet; old ctor-only run cannot prove eighth-lane
  work. Next: head capture p8 per-thread callbacks, then independent scheduling
  A/B with participant/hash/sync settings fixed and profile-off acceptance.
### L168 guest-memory locality opt-in (2026-10-13)

- `DDNATIVE_BACKING=regions`: 2 MiB-aligned guest-region host reservations,
  lazy 4 KiB commits, unchanged permissions/atomics/retirement. Large-page
  attempt fails error 1314 on this token; `large` falls back, no policy change.
- Base 41 pass; fallback five pass; runtime light 250 reported pass, zero
  failures/seven ignored. Original hash leaf replay four tests pass.
- Synthetic sequential read/read-write median paired gains 1.461x/1.385x;
  scattered/leaf gains unproven. No hardware miss-rate or gameplay claim.
- Head-owned bounded WPR profile validates. Next: matched PMC and profiler-off
  gameplay A/B after playtest; actual snapshot/counters pending. Details:
  `docs/tasks/L168-tlb.md`.
### L167 critical callbacks and exact shadow state save (2026-10-13)

- Ranked 20,700 validated joins: sky-stable shadow EE2C90 leads at
  3.905/3.651 ms solo overlap per frame; per-join IDs/durations retained.
- Opt-in JOB_SHADOW_SAVE=1 ports whole FED480 state save with ordered
  checked page accesses. 432 original returns, 68,200 paired fault/access
  checks and serial/atomic production-memory replays pass. All 56 paired
  helper samples improve, median 1.47–1.87x; no callback/FPS gain claimed.
- Four isolated and four ranking tests pass; mtcore/ddports 20 pass,
  nine ignored; light runtime 251 pass, zero fail, seven ignored.
  Combined native gate and head image/per-job A/B pending.
- Next: head train with this independent switch, then matched shadow-save
  0/1 capture. Full evidence: `docs/tasks/L167-critical-callbacks.md`.

### L158 combo31 critical paths and next lookup leaves (2026-10-13)

- Late hash=0/1 captures: 150 frames / 10,350 joins each; main joins
  **14.819→13.526 ms/frame**, solo callback tails **7.086→6.887**.
  uSkyStableShadow leads 123/150 and 121/150 frames. Its join reduction
  is **7.25%**, inside head's noise threshold; NPC counts change 8→7.
  No clean causal port/frame improvement established.
- Exact opt-in `JOB_LOOKUP` leaves 01008200/01008260: **1,380 original
  vectors**, **11,520 paired fault/access cases**, serial/atomic replay.
  Four isolated tests pass; synthetic **98/98 pairs improve**, medians
  **1.43–2.68x**. Crate gate **19 pass / nine ignored**, Python **nine pass**.
  Runtime light: **242 reported passes / zero failures / seven ignored**.
- Verified trace compression recovers **3.12 GB**. Evidence and hashes:
  `docs/tasks/L158-critical-jobs.md`. Combined native gate and live lookup
  benefit remain pending; next: head integration gate and controlled A/B.

### L159 Vulkan default image gate (2026-10-13)

- Same-run selected raw pass oracle copies attachment load state, replays
  wgpu on shadows, and compares color/depth/stencil image values in queue
  order. Separate query/resource fallback counters; initialization retained.
- 30 CPU / 19 focused GPU tests pass; dedicated four-pass oracle matches
  all 14 aspects, zero errors/skips/pending. Light suite 221 reported passes,
  inherited gamma and timing failures / six ignored; timing retry passes.
  No game acceptance; oracle commit keeps opt-in. Protocol and limitations:
  `docs/tasks/L159-vk-default.md`.
- Next: head `DDGPU_VK=1 DDGPU_VK_SHADOW=997` gameplay image gate, then
  separately adopt the staged default-on commit only after acceptance.
### L163 boot attribution and exact stream skip (2026-10-13)

- Boot intervals mix original logo holds, authored GUI sequences, input
  residence and work. Autoplay cadence ~1.66 s; title entry/exit nominally
  5.3 s. Storage's 120-frame hold has original input escape; no timer edits.
- Combo30 loading: decoders 16.969 CPU s / 14 s; 207 non-wait samples.
  CF02E0 76, CFEEC0 59 (28.5%), DB9F40 service/lock loop 52. New opt-in
  ARCHIVE_SKIP batches buffered skip, preserving original refill and ABI.
- CPU timeline replaces repeated Toolhelp with one first-Present discovery
  plus cached GetThreadTimes; process residual covers unregistered CPU.
  Boot-detail observations opt-in. 192 original-instruction + 192 scalar
  skip cases pass; 16 parser tests pass; final library results in brief.
- Not yet compared in game; inherited gamma Reset failure remains. Next:
  head skip0/1 load/image run plus boot detail; retain FAST2/TABLE opt-ins.
  Evidence and limitations: docs/LOADING.md, docs/tasks/L163-loader-cpu.md.

### L156 exact inflate and Huffman builder (2026-10-13)

- Opt-in FAST=2 adds bounded table/output spans, packed input pairs and
  aligned wide matches; TABLE=1 ports exact cf30c0 table construction.
  Defaults unchanged. Speculative input staging/literal batching regressed
  and were removed; modern-library throughput parity is not established.
- Original-instruction gate: 217 streams / 12,893 returns; 632 leaf cases
  x5; 81 table cases. Light 224 reported pass / inherited gamma failure /
  6 ignored. All final focused and full-stream rechecks passed.
- Combined ARC microbench gains are 4.1-13.6% and shared-host dependent; no
  game loading gain claimed. New zlib_table_ns isolates table work in the
  next real stage capture. Details: docs/tasks/L156-inflate-fast.md.
- Next: head FAST=1/TABLE=0 vs FAST=2/TABLE=1 with PORT=0, then repeat
  logging-off with image/control acceptance; retain separate table-only A/B.
### L158 critical scheduler jobs (2026-10-13)

- Exact E0E5E0 hash candidate: **528 original returns** match port and
  isolated lift; **672 paired fault/access cases** pass. All **105 paired
  CPU samples improve**, median **1.63–2.01x**. Opt-in JOB_HASH=1;
  combined native gate and head per-job A/B remain pending.
- Late c26s1 main-only callback leaders: DB7620 **10.679 ms/Present**,
  4B9FF0 **5.179**, 1090F70 **3.933**. These dispatch virtual methods;
  old worker coverage/tail duplication prevents per-join critical ranking.
- Opt-in private-file trace correlates frame/batch, callback completion,
  residual join, and worker signal/wait boundaries. Complete-batch parser
  separates last job from longest job and rejects loss/truncation.
- Four focused runtime and four parser tests pass. Light **226 pass /
  inherited gamma failure / six ignored**. No game or speedup claim.
- Next: head corrected census/trace capture, then exact hot-inner A/B.
  Source map, hashes and limits: `docs/tasks/L158-critical-jobs.md`.
### L162 original job participation and wake diagnostics (2026-10-13)

- Opt-in `DDRUNTIME_JOB_PARTICIPANTS=1..8` calls original `00DBD620`
  after construction; original CPU/eight clamp and frame activation remain.
  No cap patch. Intended head A/B: seven vs eight participants, main included.
- Wake analyzer reports per-batch jobs/participants, callback gaps,
  final-job solo tails and join overlaps; **17 parser tests pass**.
  Signaled lanes are distinct from active participation (one-job bypass).
- Full light gate **234 reported passes / two inherited failures / seven
  ignored**; Present timing passes its isolated retry, gamma remains.
  First-frame follow-up: **242 reported passes / inherited Present timing
  failure / seven ignored**, three focused ABI/configuration tests pass.
- c31 h0/h1: 150 frames each, 62,100 paired wakes each, no selected-batch
  rejects. Signal-to-wait p50/p95 **11.5/52.2 -> 10.6/25.0 us**;
  final-callback solo overlap **7.086 -> 6.887 ms/frame** dominates wakes.
  40 of 69 parallel batches/frame have >=8 jobs (h0).
- Ctor-only p8 configured eight but seventh worker remains idle in profile.
  First-frame original-setter reapplication candidate logs actual activation;
  head acceptance remains pending. No speculative sync primitive change.
- Next: instrumented p8 activation check and matched cache/futex A/B;
  exact measurements, hashes and limitations: `docs/tasks/L162-job-wake.md`.

### L160 job worker sizing and wake analysis (2026-10-13)

- Original constructor caps participation at min(CPUs,8), minus one for
  counts >=4: 8/12/16 CPUs all select **main + six active workers**.
  Seven foreground workers are allocated; a separate background pool has
  up to eight. More reported CPUs cannot enable the proposed larger pool.
- Producers mix per-participant and per-object work with dynamic claiming;
  the scheduler does not split a long callback. Runtime already reports
  host logical CPU count in parallel mode; ddplay requests a 1 ms timer.
- Offline L158 signal/wait/first-job pairing: **seven tests pass**. Actual
  wake latency remains pending head capture; aggregate wait durations are
  not wake delays. No runtime behavior change or game/shard run.
- Next: head L158 trace capture, cache/futex 0/0 versus 1/1, with exact
  binary/env hashes. Details: `docs/tasks/L160-job-threads.md`.
### L159 shadow failure diagnostics (2026-10-13)

- Head combo30: 1,652 shadow aspect checks / zero mismatches, then backend
  disconnect around t=91 s. `not Vulkan` can mean invalid encoder or HAL-open
  failure; Windows records NVIDIA event 153 during the run window. Exact
  trigger remains unresolved; this is not default-flip acceptance.
- Opt-in `DDGPU_DIAGNOSTICS=1` prints device-loss and fatal validation
  errors despite the runtime's silent panic hook; finisher preserves its
  panic cause. **Four CPU + two bounded GPU checks pass**; validation stays
  fatal. Next: head repeats shadow997 with diagnostics and event correlation.
  Details: `docs/tasks/L164-native-backend.md` (L159 follow-up).

### L164 native backend constant processing (2026-10-13)

- c29nat1 backend: 18.5 CPU seconds/30 seconds, 386 busy samples;
  native preparation 86 inclusive samples, constant uploads 61, synchronous
  scatter finish 27. Late counters: 52.23 draws/job, 318.54 upload bytes/draw,
  one submit/job, no new pipelines/texture sets or resource resets.
- Candidate defers scatter finish to the ordered encoder pool and skips
  coverage bitmap scans for complete constant banks, retaining range errors.
  Upload gates, source-slot tickets, draw order and submit count are unchanged.
- GPU **43 + eight ordering checks pass**, same 96-job image hash. CPU
  **30 pass / inherited strip-test failure / one ignored**; baseline reproduces
  that failure. Runtime light **233 pass / inherited gamma failure / seven ignored**.
- Paired raw-Vulkan median: 64-draw jobs **2.312 -> 2.216 us/draw**;
  5,000-draw jobs **1.804 -> 1.832** (regression). Same bytes/submits/hashes;
  no gameplay gain claimed. Next: head native VK=1 A/B and zero-mismatch
  compare. Details and limitations: `docs/tasks/L164-native-backend.md`.
### L161 generator flag fixed point (2026-10-13)

- Audited ten hot main/job lifted routines against raw stacks and opt1
  assembly. Register caching showed mixed timings and was rejected.
  Independent generator switch `LIFT_FLAG_FIXED_POINT=1` removes dead
  carry chains; default off. Regeneration removes 3,238 net flag helpers,
  preserves 44 step bodies and all per-function memory-helper counts.
- Full opt0 native/vector/inventory gate: 107 pass, 0 failures, 9 ignored;
  178,130 original cases / 59,734 functions, five unchanged inventory gaps.
  Generation 927.60 s; opt0 prebuild 370 s. Isolated opt0/opt1 replay passes
  31 cases; 43 focused generator/fixture tests pass. Full opt1 build stopped
  per head's shared-train rule; completion remains with that train.
- Long hash-loop medians improve 1.032–1.824x opt0 / 1.150–1.879x opt1;
  other fixture timings mixed. Not additive to L158 JOB_HASH=1. No game
  FPS claim. Runtime light: 232 pass, two timing failures, seven ignored;
  both failures pass isolated retries. Details: `docs/tasks/L161-codegen.md`.
- Next: head combined generation/gate with the switch enabled, then
  matched image/FPS acceptance.

### L153 game-thread synchronization (2026-10-13)

- Head combo26 late-game census: main plus six joined workers, about 69
  joins/frame at 0x00DBE66D, **18.644 / 17.813 ms/frame** inclusive with
  cache/futex 0/0 versus 1/1. Long-join tails dominate; main Sleep is absent
  in this interval. Render request waiting overlaps main work.
- Opt-in event cache and address waits; shared-table microbench 5.64–7.34x,
  event ping-pong mixed. No game/FPS gain claimed. Default single-object
  polls avoid vectors; abandoned mutex and invalid multiwait-count handling
  corrected. TLS/general waits remain service-lock serialized.
- Final light: 220 pass / inherited gamma and Present timing failures /
  6 ignored; focused all-opt-ins sync gate 24 pass, including Present timing.
  Four parser tests pass. Details: `docs/tasks/L153-sync.md`.
- Next: head profile-on same-route census, then profile-off matched
  cache/futex A/B and image acceptance before changing defaults.
### L155 combo25 save-load ranking (2026-10-13)

- Head FAST=0/1: stage-200 request to first stage update **8.176676 ->
  7.429188 s** (-9.14%), same **1,739 reads / 207.6 MB / 8,544 inflates**.
  Inclusive zlib **6.097 -> 5.101 s**; read services **0.279 -> 0.297 s**;
  zero pipeline creations in that interval. Keep exact FAST=1 selected,
  PORT=0; no new speculative archive scheduling or default change.
- Menu-exit callback to first gameplay Present **15.504909 -> 14.583692 s**
  (-0.921217 s), not confirmed input/display readiness. Neither run enters
  old `00532110`; raw title chain verifies the PC bypass. L151 preparation
  remains a boot candidate, not the dominant warm-cache save-load cost.
- **15 parser tests pass**, both 39-interval logs ranked with hashes and
  nested/parallel costs kept distinct. Light suite **212 reported passes /
  inherited gamma failure / five ignored**; required vectors **632 fast
  cases x3 + 12 outer cases pass**. Next: head repeat/logging-off FAST
  acceptance and resource-worker profiling. `docs/tasks/L155-saveload.md`.

### L155 save-load source map (2026-10-13)

- One resource FIFO worker already overlaps reads with up to three entry
  decode workers. Shared archive workspace and ordered finalizers block a
  simple parallel-archive change (`00DB9C30`/`00DB9C80`/`00DB9F40`).
- Added base/append shader gates and stage list/barrier order. The historical
  `00532110` marker is not proven PC Continue: PC storage ignores its kind 9,
  and `00531E60` has direct GameMain paths. Raw callback IDs and head input
  observations must establish the actual load interval.
- Static evidence only; no measured speedup. Next: completed combo25 FAST
  timeline ranking and exact intervention. `docs/tasks/L155-saveload.md`.

### L155 ddgpu example build repair (2026-10-13)

- `module_cost` now declares the existing pipeline logger required by its
  example-local shader cache module. `cargo build -p ddgpu --examples`
  **passes**, 3m14s in a fresh L155 target, four jobs, no incremental build.
- No GPU/game execution or renderer behavior change. Next: save-load source
  map and head's combo25 interval ranking; L151 preparation merged on L155.

### L151 parallel pipeline preparation (2026-10-13)

- Opt-in 2-4 creator threads precompile L116 exact recipes with L140 driver
  cache; shared module cells avoid duplicate background compilation. Shader
  creation events prioritize recorded variants and drop on a full draw FIFO.
- Draws never wait for creators; source checks and normal miss creation stay.
  Cold unknown variants still need first-use state. Native/raw-Vulkan objects
  are outside this recipe path. Default creator count remains one.
- 7 warmup CPU + 22 command/capture + 6 bounded GPU checks pass; identical
  two-target image hashes for N=1/2/4, including in-flight fallback. Runtime
  211 reported passes / 2 failures / 5 ignored; timing retry passes, inherited
  gamma failure remains. No game loading gain established.
- Next: head supplies c25 FAST timeline, seeds candidate recipe namespace,
  then matched N=1/2 loading/image A/B. `docs/tasks/L151-load-parallel.md`.
- Frozen production `006dcd96`: **104 passed / zero failed / nine ignored**
  in the expanded required-vector play gate. Store128 `7eddfd1d` and L130
  `a2d0407e` / `0ce6de79` are GREEN. Inventory retains five existing gaps.
- Combined L143/L141 renderer: **38 distinct GPU tests pass / three ignored**;
  all 150 state and 200 target cases run. A 60-second cache-test timeout
  passes its isolated retry in 53.12 s. CPU **37 pass**; runtime light
  **202 pass / inherited gamma failure / five ignored**. L143 `e04b76ce`
  and L141 `784f5a9c` / `83f9ad22` are GREEN for these tested contracts.
- All 33 shards rebuilt, artifact wall span **225.51 s**, rlibs **434.81 MiB**.
  No game/FPS/image acceptance claim. Details: `docs/tasks/L146-gate-c24.md`.
- Separate L142 flat exactness **106 pass / zero failures / nine ignored**;
  runtime flat-memory **five pass**. Head rejected its gameplay performance
  (board #12797); opt-in generated output stays private.
- Final typed x87/live CMP/TEST predicate codegen: base **39 pass**, Python
  **33 pass**, including 13,824 state/access comparisons plus 64 fault
  positions. Default native gates at all-shard opt0 **and** opt1 each pass
  **107 / zero failures / nine ignored**. Default gen **730 s**; 62,511 x87
  sites retargeted. Shard wall spans opt0 **613.43 s** (includes pauses),
  opt1 **558.42 s**; libraries **432.52 / 356.06 MiB** respectively.
- Synthetic x87 opt0 improves all 198 paired samples; opt1 is mixed,
  including regressions. Live predicate measurements improve at both
  levels. No gameplay FPS/image improvement claimed from these tests.
- Next: head integrates the separate helper/codegen and default-generated
  commits together and profiles gameplay, especially guest thread 0x1038.
### L152 native-job raw Vulkan bridge (2026-10-13)

- `DDGPU_VK=1` routes accepted native jobs through raw pass/draw caches;
  device-owned pipelines survive resource resets. Public native API stays
  the wgpu oracle; fallback, comparison and submission ordering remain.
- Dirty constant scatter and rendering share one ordered queue batch/job;
  identical resident snapshots skip the dirty-register scan.
- **27 CPU + 53 GPU cases pass**, zero GPU failures/timeouts; 96-job replay
  is byte-exact. Runtime light **212 pass / inherited gamma failure / 5 ignored**.
- Minimal-shader native wgpu/raw medians: changing 5,000-draw jobs
  **1.249/1.197 us per draw**; 64-draw jobs **2.495/2.505**; other fixtures
  regress. Split/combined submission timings are also mixed despite one
  queue batch/job. Remains opt-in, no native-game FPS claim.
- Next: head native VK=0/1 image/FPS/profile and VK=1 compare (zero mismatches).
  Evidence and remaining timing isolation: `docs/tasks/L152-native-vk.md`.
  Evidence and submission timings: `docs/tasks/L152-native-vk.md`.
  corrected. Part 2 caches runtime/thread state in one const TLS record;
  TLS Get/Set and semaphore release/single waits bypass the service lock.
  Host Win32 and concurrency regressions cover errors, reuse and abandonment.
- Final paired CPU microbench: TLS 1/4 threads **1.28x/18.87x**, semaphore
  release/poll **1.69x**; local bookkeeping and DeviceLock medians regress ~6%.
  No measured gain is established for the render worker's own TLS bookkeeping.
- Part 2 light: **229 pass / inherited fullscreen-gamma failure / 7 ignored**;
  Present timing passes; all-opt-ins library **84 pass**. Nine parser tests
  pass. Corrected census adds the
  missing worker callbacks, suppresses tail redispatch and emits sparse rows.
  Detailed measurements/limits: `docs/tasks/L153-sync.md`.
- Next: head adopts corrected census alongside L158's correlated batch trace,
  identifies long-join worker jobs, and performs profile-off matched image/FPS
  acceptance. Event cache/futex remain opt-in; no part 2 game gain claimed.

### L139 combo21 combined native gate (2026-10-13)
### L149 loading timeline (2026-10-13)

- Opt-in transition summaries tie title callback/stage state IDs to per-class
  Windows thread CPU and archive/zlib/GPU/upload deltas. Code-state and
  producer-Present boundaries remain distinct from visible/input readiness.
- c23 resource-load estimate, 32.181-45.474 s: inclusive zlib **6.660 s**
  (fast decoder **4.133 s**), file-read services **0.634 s**. Next 1.661 s
  interval: pipelines **1.207 s**, modules **0.558 s**, overlapping costs.
- Exact top-item candidate adopted from L145, still opt-in FAST=1; no
  measured whole-loading improvement yet. Parser keeps legacy/manual marks.
- Logging-on light suite **212 pass / inherited gamma failure / 5 ignored**;
  **12 parser tests pass**; no-shard frontend and emu-feature checks pass.
  Definitions/limits: `docs/tasks/L149-load-timeline.md`. Next: head matched
  FAST=0/1 with PORT=0, then visual/input correlation of the state markers.

### L145 outer inflate candidate (2026-10-13)

- Whole original-layout `cf02e0` translation, opt-in `DDRUNTIME_ZLIB_PORT=1`:
  **217 streams / 12,893 original-instruction returns match** registers,
  defined flags, stack, state, window and output. Samples every one of the
  install's 144 ARC classes; synthetic save, raw/gzip and random splits.
- Runtime light: **205 pass / inherited fullscreen gamma failure / 5 ignored**.
  Large-buffer microbenchmarks are mixed (including regressions); a tiny-buffer
  repetitive stream improves 1.63x. No whole-loading gain established.
- Head c23: 8.0195 s outer time is inclusive; fast decoder 4.9350 s,
  Adler 0.7229 s, window 0.4669 s. Outer plus untracked helpers: ~1.8947 s.
  Allocation census overflowed 99.58% of bytes; captured startup ranks cannot
  identify gameplay allocation leaders. Details: `docs/tasks/L145-inflate.md`.
- Head-requested fast decoder follow-up: `DDRUNTIME_ZLIB_FAST=1` page access
  and ordered forward copies pass **632 leaf cases x3 configurations** plus
  **217 streams / 12,893 returns**, with and without the new outer port.
  Real ARC whole/chunked microbench **1.59–1.69x** over current decoder;
  repetitive synthetic **3.11–4.47x**. Fast light **207 pass / gamma failure /
  5 ignored**, plus final three access checks and parallel-policy replay.
- Next: head's matched **FAST=0/1, PORT=0** loading A/B; both remain opt-in.

### L140 loading costs (2026-10-13)

- FPS line `335318fc` plus x87 discard `f4bfb866` and L130 FEDB80/FEDC00/
  FED200 candidates: combined pin `981fb77a` is **GREEN**, **100 tests
  passed / zero failures / seven benchmark ignores**. All 33 generated
  crates rebuilt; expanded required-vector supplements and inventory pass.
- Per-candidate correctness GREEN: `f4bfb866`, `382e5f35`, `4d53d724`,
  `546a052d`. Existing five inventory gaps remain. No agent game/GPU run.
- Head's c21 profile: main word loads/stores account for 23.9%/8.3% of
  180 busy samples; similar shares on job threads. Retained store128 helper
  compiles once in base, preserving each lane's permission/access checks.
- Follow-up gate **102 pass / zero failures / seven benchmark ignores**;
  base debug **36 pass**; runtime light **198 pass / inherited gamma
  failure / four ignored**. Ordered-access oracle: 2,304 cases plus fault
  ordering. All 33 generated crates rebuilt; no L142 flat-view code included.
- Synthetic aligned reference-loop/helper thread cycles: caller opt0
  **153.486 -> 61.833**, opt1 **29.657 -> 15.197**. All 33 opt0 paired
  samples improve across three access cases; opt1 has two mixed cases.
  No gameplay speedup claim; other multiword experiments were dropped.
- Next: head's image/FPS/loading comparison and separately pinned L142
  integration. `docs/tasks/L139-gate-c21.md`.
### L147 CRT profile attribution checkpoint (2026-10-13)
### L147 CRT profile attribution and upload copies (2026-10-13)

- `_NLG_Return2` is a one-byte CRT return followed by private memcpy code.
  DbgHelp's export-only Size is synthetic and cannot establish a function
  boundary. Matching public PDB, unwind ranges and export thunks now bound
  names; remaining gaps are explicit unresolved RVAs.
- 17 Python tests pass; benign-process capture/replay reattributes the same
  78/84 samples from `_NLG_Return2` to `memcpy_repmovs_amd`. No game run or
  FPS claim. Raw module/PC evidence is retained for offline resolution.
- Older c17 raw stacks identify eight backend copy samples in redundant
  image packing/buffer clones; both now removed. 22 CPU and 12 focused GPU
  tests pass. Tight 64 KiB image preparation: 2.718 -> 0.006 us, seven ->
  zero allocations. Queue staging remains; no end-to-end/FPS claim.
- All 17 c17 NLG samples on guest 0x1038 call X87::memory (L146 owner
  informed); L144 already removed snapshot zeroing/DrawCall copies.
  Runtime light: 202 pass, five ignored, inherited gamma failure after
  isolated timing retry. Next: head captures current combo24 with corrected
  profiler; text-only c21/c23 cannot recover PCs. `docs/tasks/L147-nlg.md`.
### L148 Vulkan frontend checkpoint (2026-10-13)

- L143 merged with L138 preserved. Opt-in raw draws retain cached binding
  records; one-stream recording and per-pass resource ownership avoid repeated
  allocations/clones. Constants use cached coherent mapped Vulkan memory,
  with a GPU completion fence at wrap and staged fallback on other devices.
- Final candidate plain + 144,000 changing-constant draws / two ring wraps
  match wgpu byte-exactly. Earlier 13 GPU cases pass; final full sweep and
  repeated timings await head combo24 pause. 24 CPU tests pass; runtime
  light 205 pass / inherited gamma failure / 5 ignored. No game/FPS claim.
- Next: bounded final GPU/timing gate after pause; details and provisional
  timings in `docs/tasks/L148-vk-frontend.md`.

### L134 x87 helper follow-up, next batch (2026-10-13)

- Discarding x87 callers omit unused PUSHFQ/POP capture; public flag-returning
  calls, opcode guards, memory order and PREEMPT polling remain intact.
  **34 base tests pass in debug/play**, including 312 full-state/access
  cases. Runtime light: **190 pass / inherited gamma failure / 3 ignored**.
- Caller-opt1/base-opt3 FLD1/pop median **5.407 -> 2.350 ns/pair**, all three
  alternating pairs improve. Other pairs are load-sensitive; no FPS claim.
  Profile evidence, exact benchmark and limits: `docs/tasks/L134-combined-gate.md`.
- Not included in either green gate below. Next: combine this helper with
  the next integration batch for required native replay, then head gameplay.

### L134 second combined gate: L132b and L130 integration (2026-10-13)

- Exact production pin `7aa2d159`, checked on L134 `10a484e9`: expanded
  required-vector play gate **94 pass / 0 fail / 4 benchmarks ignored**.
  All 33 shards rebuilt; **178,130 lifted cases** pass, as do every readable
  port suite and inventory declaration checks. Five existing gaps remain.
- L132b packed permission/backing entries (9 -> 8 MiB page table) pass
  31 base tests and the full lifted replay. L130's eight new ports pass
  the required core oracles, including six duration decoder implementations.
  L136 tracks missing runtime activation for entries without generated
  bodies; this gate does not prove that activation. No FPS claim here.
- The fixed shared-sink lock passes both 400-frame mtdraw comparisons.
  Details and per-suite counts: `docs/tasks/L134-combined-gate.md`.
- Next: measured x87 helper follow-up, then the next batched native gate
  and head game comparison; x87 candidate is not part of this green pin.

### L134 combined native gate, first integration (2026-10-13)

- FPS line `941f5d2c` plus final L132 `ff98640d`: expanded play-profile
  required-vector gate **88 passed / 0 failed / 1 benchmark ignored**;
  **178,130 lifted cases** and inventory checks pass. Original apply and
  state oracles include 1,568 ABI cases, 811 decodes and 3,584 state cases.
- Found a process-wide sink race in parallel mtdraw tests; shared test lock
  fixes it without changing the executor or weakening comparisons. Adopted
  owner's `4bcef3fa`. Standard gate now includes base, draw, registry and
  state suites. Existing five inventory verification gaps remain.
- L131 c17 profile identifies remaining memory cost and x87 helper cost;
  a private x87 flag-capture experiment is promising, not integrated here.
  No game/image/FPS claim. Details: `docs/tasks/L134-combined-gate.md`.
- Next: head's second combined pin `7aa2d159` (L132b plus newer ports and
  worker fixes), then the next helper batch.
### L140 loading costs (2026-10-13)

- Exact opt-in Adler helper: 45 original-instruction cases pass, including
  eight installed ARC payloads; 128 KiB microbenchmark 495 -> 44 us/call.
  16 whole-stream A/B cases match across 607 returns (state/output/registers).
  Outer zlib state machine remains original; caller/helper census added.
- Opt-in CRT scratch removes repeated host allocation for bounded copies
  and all memset sizes. Identity, overlap and allocation-count checks pass.
  Opt-in allocator stack census supports offline top-ten attribution.
- Existing Vulkan persistence/warmup audited; cache follows runtime sandbox,
  native-device cache hookup opt-in. Two cold/warm GPU image tests pass.
- Final runtime light: 203 pass, one inherited gamma failure, four ignored.
- Next: head's separate allocation census and unskipped loading A/B; whole
  loading gains and top-ten game sites remain unmeasured. Details and final
  suite status: `docs/tasks/L140-loading.md`.
### L138 production Vulkan pass encoder (2026-10-13)

- Opt-in `DDGPU_VK=1` owns HAL Vulkan pipelines/descriptors and batches raw
  passes through L137's ordered finisher. Wgpu retains queue submission,
  resource initialization and lifetime; tracked preambles plus internal
  barriers preserve layouts. Unsupported/query/MSAA passes use wgpu.
- **20 CPU / 12 focused byte-exact GPU / 2 MSAA fallback tests pass**.
  Dependent 900-pass float32 ping-pong: **2,698 raw / 2 initialization
  fallbacks**, byte-exact. Earlier broad sweeps: 21 pass, five hard timeouts.
- Three production benchmark repeats: median **21.536 -> 6.722 us/pass**
  for 900 tiny passes (3.20x); 5,000-draw workloads show no clear overall
  gain, including 40 pipelines / 200 sampled sets. Synthetic, not game FPS.
- Runtime light: initial 199 pass / 2 fail / 4 ignored; timing failure passes
  alone, inherited fullscreen-gamma failure remains. No runtime edits.
- Blocker: original-game image/FPS acceptance requires head's run. Next:
  integrate with L143's wrapped pipeline key and A/B `DDGPU_VK=0/1` on
  default `DDDRAW_NATIVE=drawcall`. Hashes/details: `docs/tasks/L138-vk-encoder.md`.

### L109d memory ownership diagnostics (2026-10-13)

- Head's c18 run reaches 17.05 GiB private memory with only ~0.64 GiB live
  Rust allocations. Unicorn 2.0.1 commits 1 GiB per cached engine and its
  strong hook ownership prevents teardown, reproduced CPU-only.
- New opt-in `DDPLAY_ALLOC_LOG=1` measures live Rust requested bytes and
  4 KiB aligned allocations through the unchanged System allocator.
  Standalone off/on tests cover allocation, zeroing, realloc and real global
  allocator startup. No game run by agent.
- Pin Unicorn 2.1.1, x86 only, with a 32 MiB translation cache: two-engine
  commitment 2,153,340,928 -> 10,113,024 bytes; after drop 561,152 bytes.
  Six fallback state/call tests pass before/after; light suite 194 pass,
  one inherited fullscreen-gamma failure, four ignored. No game A/B yet.
- Follow-up removes 24 heap buffers per emulator state roundtrip; all 16
  wide-register reads match the allocating API. Six fallback tests and 54
  unit tests pass. Paired debug short calls 2.641 -> 1.679 s; synthetic hot
  loop 81 -> 161 ms remains a reported regression, not a game FPS result.
- Next: head integrates and repeats memory/image/save-load timing run;
  profile remaining load time afterward. c18's 32..45 s contains 7.147 s
  inclusive zlib time vs 0.478 s reads; opt-in entry-buffer counters now
  distinguish small reads around the fast-decoder guard.
  `docs/tasks/L109d-memory.md`.
### L133 native rejection fallback and Vulkan interop (2026-10-13)

- Combo17 native log: **63,496 rejected jobs / 3,488,189 lost draws**;
  producer enqueue counters were mistaken for worker acceptance. Compare
  mode ran **zero** comparisons (capability flag bypass, sent to claude-2).
- `native_or_draw` retains original DrawCalls on the same worker FIFO;
  atomic rejection replays them once without a producer wait. **12 CPU
  transport tests pass**; integrated by claude-2. Combo19 has **42,880 matching
  attachment/aspect comparisons / zero mismatches** for accepted jobs.
- `ab4318c9`: bounded triangle strips exclude restart by the D3D9 index-range
  contract; covers **99.954%** of cap1 strips. **Seven worker GPU / 16 CPU
  tests pass**; runtime light **187 pass / one inherited gamma failure /
  three ignored**. Query, channel-fill and depth-bias cases keep FIFO fallback.
- Raw Vulkan drawing with wgpu-owned targets/VB/IB, tracked barriers and
  wgpu queue submission: **eight image cases pass**. Preliminary median
  **0.354 us/draw** vs ash **0.326** / wgpu **1.129**, prefilled uniforms;
  shared-load timing limits and migration risks in the L133 brief.
- Next: head's fresh Compare/Native run with the strip fix to measure wider
  uptake and FPS. No valid combo17 native image/FPS acceptance claim.

### L129 combined integer/float flag fusion gate (2026-10-13)

- Merged FPS-line `05b35e35` with both lifter emitters and regenerated
  **59,734 functions / 33 crates**, preserving 44 scoped step functions.
- **178,130 original lifted cases pass; zero failing lifted functions**.
  Full required-vector native gate: **49 tests pass**, readable ports and
  inventory checks pass; **26 focused Python tests** also pass.
- Corrected one pre-L62 x87 test to use documented `park()` host boundaries;
  host assertions retained, guest continuity assertions added. No runtime
  change or weakened fixture checks. Existing 5 coverage gaps remain.
- Clean shard compilation: **264.95 s wall span**, **434.82 MiB rlibs**;
  generated source **+0.2194%**. No comparative compile-speed/FPS claim.
- Next: head's same-scene loading/image/FPS measurement. Details and
  timing/profile limits: `docs/tasks/L129-dead-flags.md`.

### L119 native sink: drawcall is the default (2026-10-13)

- The executor's scene-pass draw records are decoded straight from the
  nDraw objects (mtstate on cache misses, identity and per-frame binding
  caches, 0x00DAA8B0's history-dependent sampler section made on the
  context and device) into the D3D9 path's DrawCall: the applies, the
  D3D9 services and the snapshot are skipped for them
  (crates/ddruntime/src/ndraw_native.rs).
- **Compared with the original path in game**: combo19, `DDDRAW_NATIVE=verify`,
  ~420,000 draws per 10 s compared field by field with the DrawCalls the
  D3D9 path made, all equal (run_c19ver.log). Tests: the sink's DrawCall
  equals the runtime D3D9 path's (tests/ndraw_native.rs, real D3D9
  objects), the sampler section equals mtstate::shader on 3,000 random
  cases, decode unit tests.
- So `DDDRAW_NATIVE` unset now means `drawcall`; `0`/`off` is the escape
  hatch (everything through the D3D9 path); `verify`, `compare`, `census`
  and `1`/`native` (native device jobs, L126) as before.
- combo15 (`drawcall`): 14.2 fps avg, equal to the D3D9 path; the render
  thread is no longer the bound (the main thread's game code and sMain's
  job join 0x00DBE560 are). Details: docs/tasks/L119-native-executor.md.
- Next: native jobs (`1`) as the scene path once compare is clean.

### L126 native job transport checkpoint (2026-10-13)

- Owned NativeJob/Pass/Draw transport with effective D3D9 values, identity
  keys, resource IDs and register ranges; same DeferredBackend FIFO.
  One-time capability check; supported jobs enqueue without a round trip.
  Unexpected worker rejection is logged loudly (head-approved contract).
- **8 transport tests pass**, including upload/native/query ordering and
  unsupported-job ownership. Worker and game comparison pending.
- Worker now executes through native::Device with immutable state/shader /
  texture-group identity caches on the same device/queue. **4 Vulkan GPU tests
  pass** (16 backend/host-driver image cases plus FIFO/resource/rejection tests),
  **14 ddgpu CPU tests pass**. Per-job compact recorder; persistent resident /
  bundle reuse and game/FPS comparison remain separate work.
- L127 shadow hooks compiled; next: end-to-end shadow gate and head N2 run.
  **14 ddgpu CPU tests pass**. Persistent resident recorder and finisher
  handoff now compile; focused GPU gate pending shared HEAVY_BIG availability.
- L127 five shadow integration tests pass; runtime light **182 pass / one
  inherited gamma-reset failure / three ignored**. Next: final resident /
  finisher GPU gate, then head N2 image/FPS run.
  **14 ddgpu CPU tests pass**. Final resident-recorder/finisher gate:
  **16 Vulkan tests pass** (six API, five worker, five shadow), including
  zero-reupload for identical jobs and async shadow submission ordering.
- Persistent source `e3599f42` + ordering fix `542111eb`; bundles opt-in.
  Runtime light **182 pass / one inherited gamma-reset failure / three
  ignored**. Next: head N2 image/FPS run.
  Details: `docs/tasks/L126-native-worker.md`.

### L123 native resident constants and bundles (2026-10-13)

- Stable constant snapshots upload changed 16-byte registers in one packed
  transfer + ordered compute scatter. Exact repeated draw sequences reuse
  render bundles; dynamic state splits sequences. Persistent texture/sampler
  groups retain identity keys. Existing direct API remains available.
- Matched optimized 5,000 draws / 40 pipelines / 200 sampled sets, synthetic
  8,200 VS + 7,300 PS register writes/frame: full **1.644**, resident **1.264**,
  resident + bundles **0.843 us/draw**, including upload/finish/submit.
  Three alternating repeats; bundle range 0.822–0.868. Uploads **15.67 MB ->
  340 KB/frame**. Minimal shaders; no gameplay/FPS claim.
- **6 GPU tests / 11 CPU tests pass**; runtime light **171 pass / 1 inherited
  fullscreen gamma-reset failure / 3 ignored**. Core `e062d80a`.
- Next: head-assigned L126 owned NativeJob FIFO + WgpuBackend worker adapter
  for L119 N2. Exact conditions and limitations: `docs/tasks/L123-native-device-2.md`.

### L122 gameplay pass map (2026-10-13)

- Frame7539: **951 target runs / 5,295 draws**, including **864 SoftBody
  simulation runs**; full target sequence, eight copies and sampled-resource
  dependencies in `docs/FRAME_PASSES.md`.
- **402 shader identities (225 VS / 177 PS)** in 50 sets; 334 byte-exact
  root-package matches. Shader reflection identifies sky-scattering half-float
  targets, material mip chains, God Rays, DOF, color-correction LUT and GUI.
  These correct the earlier size-only tone-map/bloom/luminance guesses.
- Draw offsets/counts, shader END boundaries, hash uniqueness and the
  108,502-command slice digest checked. Static issuers cited with DTI/Ghidra
  evidence; dynamic object-to-command joins remain unresolved. No runtime
  changes, game/GPU/shard runs, image-equivalence or FPS claim.
- Next: head's same-run capture with producer tags; existing L111 stats alone
  lack caller/class identities. L113 retains SoftBody replacement ownership.
### L109c boot attribution and inflate default (2026-10-13)

- Head combo6: boot archive inflate **4.85 -> 1.39 s**, first Present still
  **5.84–5.86 s**. Cold run has zero shader modules/pipelines through 5.02 s.
- **632 exact inflate cases** pass again; default-on and explicit-off
  registration tested. Read-ahead remains default-off. Per head's updated
  brief, no fast-boot option: original timing/input paths remain unchanged.
- Boot profile implicates scene/geometry setup; a nested Vorbis symbol is
  inconsistent with original callers. Exact opt-in FE9580/12EB690 counters
  added; wrapper-effects regression and eight parser tests pass.
- Float compare/LAHF/TEST/Jcc fusion covers **1,537 sites / 701 functions**,
  nine in FE9580. Three tests pass, including **401,408 native oracle
  cases** preserving AH, flags and MXCSR. Whole-function original replay
  also passes before/after fusion (three fixtures, **6,155 call boundaries**).
  Whole geometry port now passes **12 cases / 18,499 boundaries**, including
  NaNs/subnormals, with original SSE executed on hardware for exact MXCSR.
  Finite-position indexing removes redundant scans; opt-in via
  `DDRUNTIME_GEOMETRY_PORT=1`, off pending head A/B.
  Next: head integration regeneration/gate and geometry A/B; UI timings
  need head marks. No measured geometry or first-Present speedup yet.

### L109 loading baseline, cache experiment and original waits (2026-10-13)

- Head instr1: at 16.032 s, archive inflate **4.854 s**, file reads
  **0.056 s**, pipelines **0.007 s**; first Present **6.002 s**.
  Final archive inflate **5.482 s**. UI phase marks remain unobserved.
- Bounded install-archive read-ahead implemented and tested, **off by
  default**: warm sequential 128 KiB **7.601 -> 12.903 us** (regression),
  repeated 128 KiB **7.559 -> 4.352 us**; host I/O microbench only.
- Light suite **168 pass / 1 existing gamma failure / 2 ignored**;
  final cache/APC/overlay/budget checks pass after buffer reuse.
- Original logo/disclaimer timers unchanged: **2 s / 4 s** nominal
  holds; alternative regional lists total **2 / 6 / 10 s**, plus GUI
  transitions. Source addresses and reachability limits in L109 brief.
- Next: head's matched run with inflate port `9093ace7`, observed UI
  timestamps and all-zlib telemetry; keep cache disabled for this A/B.

### L109 readable archive inflate fast loop (2026-10-13)

- Whole `0x00cf2cb0` fast decoder port; original outer inflate/streaming
  state machines retain headers, checksums, buffering and callbacks.
- **559 original Unicorn cases pass**: all scratch/stack/heap bytes,
  GPR/XMM registers and arithmetic flags. No shard or game build/run.
- Isolated opt1 generated / opt3 readable leaf: **18.209 -> 2.260 us**
  for 349 output bytes, **16.274 -> 3.239 us** for 512 bytes; three
  alternating 1,000-call rounds. No whole-loading speedup claim.
- Next: head's load-log/same-save comparison, baseline `45b89e6f`,
  readable leaf selectable with `DDRUNTIME_ARCHIVE_INFLATE=0/1`.
### L110 native backend floor (2026-10-13)

- Optimized 5,000-draw/40-pipeline/200-group synthetic Vulkan floor,
  including finish + submit: **wgpu 0.884, reused bundles 0.386, ash 0.326
  us/draw**. Uniform data prefilled; no texture sampling/gameplay claim.
- 900 one-draw clear passes: **7.146 / 7.151 / 1.351 us/pass**, respectively.
  Bundles reduce repeated draw processing but do not remove pass overhead.
- Native API: **4 GPU tests pass**, including full/compact exact D3D9/host
  comparisons and texture/constants/ring checks; **5 CPU tests pass**.
- Matched 5,000-draw native workload: full **3.839**, compact **1.692**,
  compact range updates **1.433 us/draw**, including upload/finish/submit
  (three alternating repeats). Minimal shaders; no gameplay/FPS claim.
- wgpu-owned texture/buffer -> raw Vulkan/hal -> wgpu readback passes with
  separate ordered encoders; mixing raw and normal encoding in one fails.
- Current-base runtime light suite **167 pass / 1 existing gamma-reset
  failure / 2 ignored**. Next: L119 integration and one captured-pass oracle.
  Details and limitations: `docs/tasks/L110-native-device.md`.

### L111 nDraw executor observer (2026-10-13)

- Opt-in read-only executor wrapper plus Present-selected frame counters;
  off registers no new execution/Present hooks. Nested command decoder,
  state/shader/sampler/buffer identities and ordered surface target runs
  sourced to DAB480/DA6DA0 and apply helpers. Counts are raw records.
- Seven synthetic regressions pass; enabled light unit suite 35 pass /
  1 existing ignored. Full suite 158 pass / 2 existing timing failures /
  1 ignored; both failures pass isolated reruns (160 verified across runs).
  Fullscreen gamma fails with switch off too on current desktop; recorded
  L34 display resolves it. No tests/checks weakened.
- cap1 last20 analytical extraction preserves 108,502 command payloads
  and 50,507 referenced blobs (284 MB raw vs 19.79 GB whole-capture blobs).
  Unchanged frame_stats completed: 107,544 draws / 19,704 passes.
- Mean 5,377.2 draws / 985.2 passes per frame, including 898.2 single-draw
  tiny float32 target passes. Constants change 248,722 bytes/frame;
  fixed-state lists differ for 4.898% of predecessor comparisons.
- Next: head-run DDRUNTIME_NDRAW_STATS=1, EVERY=1 for executor/capture
  correlation; no game, image-equivalence or FPS claim from this batch.
  Details: docs/tasks/L111-executor-stats.md.
### L109 loading instrumentation and read scratch (2026-10-13)

- Opt-in process-relative init/movie/file/archive/GPU/frame telemetry;
  original archive read/inflate bodies retained. Timeline parser keeps
  unobserved title/menu/loading phases explicitly unmeasured.
- Synthetic optimized warm 128 KiB ReadFile: **11.569 -> 9.643 us/call**,
  **1 -> 0 allocations** (three alternating pairs, five samples each).
  Small reads have no reliable timing win; no game loading speedup claim.
- Light runtime serial suite: **153 passed, 1 existing gamma failure,
  2 ignored**. Parser **8 pass**, ddports **1 pass**. Details:
  `docs/tasks/L109-loading.md`.
- Next: head-run unskipped boot/menu/save baseline using
  `DDRUNTIME_LOAD_LOG=1`, then same-save before/after phase comparison.
### L118 dense D3D9 state and snapshot access (2026-10-13)

- Dense sampler/stage banks and render-state indices preserve presence, unusual
  IDs and immutable draw payloads. Scoped device borrows avoid repeated lock
  bookkeeping; dirty snapshots avoid temporary Vec growth.
- Three paired debug dirty-sampler draw medians **49.71 -> 29.97 us**, allocations
  **8 -> 6** including COM test buffers. Clean draw timings overlap; no FPS claim.
- Runtime light suite **158 passed / one fullscreen-gamma Reset failure /
  one ignored**; new map-equivalence, snapshot-retention and lock-unwind checks
  pass. No game/shard/GPU runs. Details: `docs/tasks/L118-d3d9-render-thread.md`.
- Next: head combo4 gameplay/image comparison and renewed render-thread profile.
### L117 nDraw state semantics (2026-10-13)

- New standalone mtstate decoder: four descriptor constructors, eight
  applies and InputLayout/declaration synthesis compared with original
  Unicorn vectors. **3,584 cases / 17,928 D3D9 calls** and checked memory
  match; four contract tests and strict crate clippy pass.
- Raster depth-bias multiplier is `0x33800001`; sampler max-mip conversion
  truncates. This is a call-level semantic decoder, not an ABI override.
- NDRAW section9 maps descriptors to D3D9/wgpu and lists native gaps.
  Runtime light serial:155 pass,1 inherited fullscreen gamma Reset failure,
  1 ignored (initial parallel lock-timing failure passes serially).
- Next: L112/L110 integration and head-run frame comparison. No GPU/game
  run or image/FPS claim. Details: `docs/tasks/L117-state-semantics.md`.

### L90 shared draw payloads (2026-10-13)

- Shader tokens/declarations are shared immutable allocations; constants
  detach on changed setters and remain immutable for queued/recorded draws.
  Backend caches use retained shader identities, avoiding per-draw bytecode
  hashing while still sharing identical independently created programs.
- On L92/L93 base: runtime light suite **150 passed**; frame serialization
  **5 passed**, identity regression **1 passed**. Runtime allocations
  **15 -> 10/draw**; three paired debug samples give median **49.35 ->
  45.59 us/draw**, with overlapping ranges under shared-machine load.
- Completed GPU driver comparisons passed; long-lived cache, depth precision
  and target formats hit the 60-second limit and remain unverified.
  Debug GPU benchmark: **442.91 -> 256.04 us/draw**, same 11,000 draws / 23
  passes. No gameplay/FPS claim. Next: head's Arc-only integration run,
  then separate compact-constant/snapshot batches.

### L82 input polling convoy (2026-10-13)

- Head's measured GetAsyncKeyState hotspot (1,284,189 calls / 14,719 ms wait
  over 120 s) now polls atomic accepted-key state without the guest lock.
  New host events alone use the locked pump to preserve hook suppression;
  GetKeyState retains per-thread message-removal/toggle semantics.
- Five input-query bindings are LockFree. Busy-worker 32-poll scenario:
  **32 -> 0 acquisitions/blocks**, **4.461 ms -> 15.003 us/call**.
  **141 light tests pass**, including existing hook/queue behavior, concurrent
  edge consumption and a 20,000-step comparison against the old key model.
- Next: head's gameplay/FPS comparison; no agent game or shard build/run.

### L82 render service lock tails (2026-10-13)

- DrawPrimitive, DrawIndexedPrimitive and Clear retain locked snapshots and
  backend command order, but no longer reacquire solely to return. UP stays
  locked for post-draw state changes. Per-thread/frame contention telemetry
  identifies waiting services and holders; parallel preemption avoids a
  serial-only hook and suspended-thread scans.
- Synthetic 32-call contention: 64 to 32 acquisitions, 32 to zero blocks,
  4.479 ms to 71.496 us/call. **138 light runtime tests pass**; actual COM
  regression checks acquisition counts and recorded commands. No game run.
- Next: head's L82 redirect targets GetAsyncKeyState's measured lock convoy
  with atomic input snapshots; head owns gameplay/FPS comparison.
### L81 audio source scratch (2026-10-12)

- Cached voice decoder format and reused source byte/float/ADPCM buffers;
  bit-exact synthetic stream/mixed-output comparison passes. Runtime: 134
  tests initially pass (two unrelated lock-timing checks fail on loaded
  reruns); ddaudio: five unit and all nine host XAudio2 oracles pass.
- Optimized 480-frame feed medians improve 1.05-1.21x across three repeats;
  removes two PCM / four ADPCM temporary allocations after warmup.
- Profile correction: mixer runs on guest `0x103c`, not render `0x1034`;
  the generic byte-vector symbol does not establish an audio caller.
- No game/FPS comparison. Next: head caller-resolved render profile after
  integration. Details: `docs/tasks/L81-audio-render-thread.md`.
### L83 production shader IR cache (2026-10-13)

- Exact Naga IR cache connected to draw-module creation; unchanged translation
  and device validation. Cache misses/corruption regenerate safely.
- Cache tests 2, reflection 29,198 shaders, WGSL variants 60,524 and runtime
  light serial tests 133 pass. All 29,198 IR roundtrips are exact.
- Preliminary 1,000-shader Vulkan sample: 4.31 -> 1.05 ms including translation
  and cache I/O. Filesystem residency matters; whole-corpus paired timing pending.
- Initial GPU pass: 29 executed cases pass before yielding to L90's gate.
- Next: remaining targets, warm GPU repeat and full paired timing after GPU
  release. Details: `docs/tasks/L83-naga-direct.md`.

### L66 hot/cold build (2026-10-12)

- Optional function-level hot packages implemented; 20 focused Python tests,
  one real hot/cold Rust graph test and the native play-profile gate pass.
  Required lift corpus: 178,130 cases / 59,734 functions. Input audit clean.
- Fixed shard: 254.89 s opt1 to 21.49 s opt0/eight-thread front end.
  Full play + emu: 636.08 s (first full L66 build, some dependencies cached), versus head's approximate 25 min.
  Provisional profile list has 83 functions; 230 hot replay cases match.
- No compile improvement from predicate/accessor experiments; not retained.
  Head owns base helpers. Full sampled list and runtime baseline are pending.
- Scope narrowed to layout/gate: emission, base structure and toolchain
  work handed to L73/L74/L75. Extra baseline/replay remains queued behind
  another lane's GPU test; results will post to head/L75.
- Next: complete hot sample and head-run clean FPS comparison. See
  `docs/tasks/L66-build-speed.md` for conditions and limits.

### L28c remaining recorder review (2026-10-12)

- **+0 functions / +0 cases**; audit preserves all **59,685 / 177,649**.
- **56a434b8**, **78 tests pass**: finite integer proofs can cross opaque
  vector shuffles and x87 exchanges. Two recipes now reach explicit
  floating-comparison blockers; no numeric branch facts are guessed.
- RESIDUALS documents **nine recorder set-asides** and moves **565308 /
  56532A** to the lifter owner for credit through recorded parent 5652E0.
  Main **17add4c7** merged; no generation or Rust build.
- **Next action:** recorder entries from the title/gameplay emulator log;
  revisit set-asides when new evidence addresses their stated blockers.

### L28c shared continuation inputs (2026-10-12, pending replay)

- **+7 functions / +21 cases**, all 3/3; private **59,685 / 177,649**.
  Audit preserves every **59,678 / 177,628** baseline record/case.
- Train 15 merged; owner boundary rule integrated as **2d1a4f80**.
  **205 light tests**, then **76 focused tests** for checked-caller bounds.
- D29AF0 caller replays pass, but ordinary and proof-enabled target trials
  still reach the limit. RESIDUALS: **49 rows, 23 pending, 11 unfinished
  recorder rows**. No generation or Rust build; head replay required.
- **Next action:** remaining finite-bound inputs, boot requests first;
  coordinate the two inherited-frame entries with the lifter owner.

### L28c frame and scalar input batches (2026-10-12, pending replay)

- **+16 functions / +48 cases**, all 3/3; private **59,678 / 177,628**.
  Every audit preserves all **59,662 / 177,580** baseline records/cases.
- Latest main **1ed8c4aa** merged; **202 light tests pass**. All recorders
  exited. RESIDUALS: **49 rows, 16 pending**, 34 recorder / 15 lifter or gap.
- **Next action:** prepare shared CRT tails with the owner-confirmed L50
  rule; remaining bound/caller-state trials still hit the instruction limit.


### L28c bounded callee cleanup batch (2026-10-12, pending replay)

- **+9/+27**, all targeted dispatchers 3/3; this round **+16/+48**.
  Private **59,662 / 177,580**, all baseline **59,646 / 177,532** unchanged.
- Source **9a345bd2**, **186 light tests pass**, clean combined audit.
  RESIDUALS has **65 rows, 16 pending replay**; main train 13 merged.
- **Next action:** remaining fixed-frame input hints and shared CRT tails;
  18 dispatch, seven bound, six individual recorder blockers remain.


### L28c aligned-frame dispatch batch (2026-10-12, pending replay)

- **+7 functions / +21 cases**, all 3/3; private **59,653 / 177,553**.
  Six use matching aligned-frame saves, one the existing wrapper strategy.
- **182 tests pass**; all **59,646 / 177,532** baseline records/cases unchanged.
  Main train 13 merged; RESIDUALS has **65 rows, seven awaiting replay**.
- **Next action:** checked callee-body cleanup inputs for nine dispatch rows.


### L28c post-train-13 census (2026-10-12)

- Main da9e8e4b merged; **59,375 / 59,440 verified**, **65 residuals**
  (47 recorder, 18 lifter/gap). All 44 prior pending rows now credited.
- Aligned-frame mock-input strategy **7443b1f7**, **182 light tests pass**.
  Six recording trials queued; private data has no new additions yet.
- **Next action:** finish the serial frame trials and audit their results.


### L28c smaller input classes (2026-10-12, pending replay)

- **+6/+18**, all three return-address and three memory-extent rows;
  cumulative **+43 functions / +129 cases**, private **59,646 / 177,532**.
- **180 tests pass**; audit preserves all **59,603 / 177,403** prior records
  and cases. RESIDUALS has **44 pending replay**; Rust verification pending.
- **Next action:** valid caller/object state for 35 dispatch rows; boot
  boundary fixture requests first. Seven instruction-bound rows remain.


### L28c sentinel and finite-proof batch (2026-10-12, pending replay)

- **+6/+18**, all 3/3: three longer finite proofs and three generated
  sentinel-array inputs. Cumulative **+37/+111**; private **59,640 / 177,514**.
- **180 light tests pass**; baseline **59,603 / 177,403** unchanged, audit
  clean. Main merged; RESIDUALS marks **38 awaiting replay**.
- **Next action:** finish seven smaller input-class trials; valid caller
  state remains the main blocker for 34 dispatch rows. Boot stops first.


### L28c instruction-bound class batch (2026-10-12, pending replay)

- **+11/+33**, all 3/3, from finite generated-state traces. Cumulative
  **+31/+93**, private **59,634 / 177,496**; prior records unchanged.
- **177 light tests pass**. RESIDUALS marks 32 rows awaiting replay and
  distinguishes sentinel-input loops from remaining proof/dispatch blockers.
- **Next action:** retry three measured wall-time limits without changing
  the instruction ceiling; supply bounded sentinel records for three loops.

### L28c dispatch/return class batch (2026-10-12, pending replay)

- **+20 functions / +60 cases**, all 3/3, with the general wrapper-call
  strategy. Private **59,623 / 177,463**; all **59,603 / 177,403** prior
  records/cases unchanged (`class-dispatch-audit.json`).
- Main train 12 merged; **175 light tests pass**. RESIDUALS now contains
  **109 inventory residuals**, including **21 awaiting private-case replay**.
- **Next action:** finish the 27 instruction-bound trials, then checked
  caller-state inputs for remaining dispatch/return failures. Boot stops first.

### L28c residual-class proofs (2026-10-12)

- SETcc condition bytes and width-correct NEG extend finite-trace proofs;
  **166 light tests pass**, including synthetic original execution.
- Largest class dispatch/return trials are queued first; instruction-bound
  rows follow. Accepted data remains **59,603 / 177,403**.
- **Next action:** finish serial class trials, audit additions, update the
  residual map against the completed train-12 inventory.

### Post-train-11 residual map (2026-10-12)

- [RESIDUALS.md](RESIDUALS.md) lists all **137 / 59,440** non-verified
  inventory functions after train 11 (**59,303 verified**), one row each:
  class, blocker, owner. **104 recorder; 33 lifter/gap/oracle**.
- New serializer fixture remains pending replay: **+1/+3**, 163 Python
  tests, unchanged prior-data audit. Both remaining manager chains record
  0/3; 0x00493AA0 stays aside. Main a6fb8349 merged.
- **Next action:** head replay of 0x00C9E0F0; then use the census to assign
  the next blocker, with boot stops first.

### L28c exact-register serializer (2026-10-12, pending replay)

- **+1/+3:** 0x00C9E0F0 records 3/3 with explicit exact-register proof
  inputs. **163 tests pass**; private **59,603 / 177,403**.
- Audit: all prior **59,602 functions / 177,400 cases unchanged**.
- **Next action:** finish two checked manager chains, then refresh the
  residual census against the completed train-11 inventory.

### L28c string/constructed fixtures (2026-10-12, pending replay)

- **+3/+9:** two string helpers and a checked constructed self-copy, 3/3
  each. Cumulative **+10/+30**, private **59,602 / 177,400**.
- Audit: all **59,592 baseline functions / 177,370 cases unchanged**;
  no stale mocks or unexpected edits. **161 tests pass**; main merged.
- Five recipes remain, including 0x00493AA0 set aside. **Next action:**
  new boot stops first, otherwise checked state for 0x004A0C00's branch.

### L28c division/tail fixtures (2026-10-12, pending replay)

- **+2/+6:** 0x0046A2D0 and 0x00DBA780 record 3/3; **161 tests pass**.
- Cumulative **+7/+21**, private **59,599 / 177,391**; baseline records
  unchanged. Main merged. **Next action:** constructor and string recipes.

### L28c byte/vector proof facts (2026-10-12)

- Exact narrow-register lanes and bitwise-zero vector stores retain only
  proved bits. **161 tests pass**; accepted data remains **59,597 / 177,385**.
- **Next action:** finish queued trials, then checked 0x00815C60 constructor
  input and the two string-copy recipes.

### L28c checked division (2026-10-12)

- Division proof checks zero/overflow and preserves undefined-flag limits.
  **159 tests pass**; two original-game trials await the boot heavy slot.
- **Next action:** finish sibling/destructor recording, then string inputs.

### L28c mocked tail proof (2026-10-12)

- Direct mock tails retain return-address and exact runtime mock checks.
  **158 tests pass**; original destructor recording is pending.
- **Next action:** signed division proof, then sibling/destructor trials.

### L28c multiply/rotate fixtures (2026-10-12, pending replay)

- **+2/+6:** 0x00494D80 and 0x0046A150 record 3/3. **157 tests pass.**
- Cumulative **+5/+15**, native-L28c **59,597 / 177,385**; all baseline
  records unchanged. **Next action:** 0x0046A2D0 proof and destructor tail.

### L28c exact-input fixtures (2026-10-12, pending replay)

- **+3/+9**: 0x004A06B0, 0x00495150, 0x0050A110 record 3/3 each.
  **155 tests pass**; all 59,592 baseline records / 177,370 cases unchanged.
- Native-L28c **59,595 / 177,379**. Main train 10 merged; Rust replay is
  head's next step. **Next action:** remaining IMUL/ROL proof gaps.

### L28c standalone input proofs (2026-10-12)

- Exact-input proof now also supports standalone recorder distributions.
  **155 tests pass**; original-game trials are pending the heavy slot.
- Fourteen residuals are queued for serial trials; 0x00493AA0 stays aside.
  **Next action:** finish the waiting boot-route trial, then that list.

### L28c checked stack arguments (2026-10-12)

- Setup outputs can bind method stack arguments while preserving the return
  address and subsequent setup distributions. **154 tests pass.**
- 0x004A06B0 and 0x00DBA780 producer recipes are prepared, not recorded.
  **Next action:** run serial trials when head's heavy slot becomes free.

### L28c integer proof operations (2026-10-12)

- CDQ, double shifts and overflow-aware signed branches have regression
  coverage. **153 tests pass; private records unchanged.**
- 0x00494D80 retry queued behind head's train. **Next action:** finish the
  retry, then checked stack-argument bindings for 0x004A06B0.

### L28c exact input-page proofs (2026-10-12)

- Opt-in checked-state proof uses the recorder's exact initial page bytes
  with existing alias guards. **151 tests pass; 59,592 / 177,370 unchanged.**
- 0x00494D80 reaches 69,197 proof instructions; CDQ is unsupported.
  **Next action:** general integer proof semantics. 0x00493AA0 set aside
  with both setup-order mismatches documented, per head.

### L28c earlier producer bindings (2026-10-12)

- Method inputs can select any checked setup phase without rebinding later
  producers. **148 light tests pass**, audit unchanged beyond boot16 +1/+3.
- Both output/manager orderings for 0x00493AA0 change accepted observations
  and reject. **Next action:** resolve those shared input dependencies.

### L28c bounded producer replay (2026-10-12)

- Guarded constructors re-prove their budgets on accumulated setup state.
  **147 light tests pass**; no further additions or changed records.
- Destination producer 0x004915A0 returns, but changed observations reject
  the sequence. **Next action:** establish independent producer ordering.

### L28c boot stop 16 (2026-10-12, pending replay)

- **0x00DC8270 records 3/3** on checked 0x00DD5ED0 constructor state.
  Proof: **49,933 instructions**, limit 69,933. **145 light tests pass.**
- Private native-L28c **59,592 / 177,370** (+1/+3); all prior records
  unchanged. **Next action:** head replay, then remaining input recipes.

### L28c explicit setup bindings (2026-10-12)

- Checked setup outputs can supply explicit method-register proof facts.
  **145 light tests pass; accepted records unchanged (59,591 / 177,367).**
- Three-producer 0x00493AA0 remains 0/3; priority moves to boot stop 16,
  0x00DC8270. **Next action:** checked sound-engine constructor state.

### L28c affine proof checkpoint (2026-10-12)

- Symbolic pointer cancellation advances the manager-backed 0x00493AA0
  proof without treating random register values as constants. **143 light
  tests pass**; no new fixture or accepted-data change.
- **Next action:** destination constructor 0x004915A0 and an explicit
  checked output-register binding; the next proof stop tests that pointer.

### L28c checked producer sequences (2026-10-12)

- Setup producers can run sequentially only when each replay matches its
  accepted case; prior state and overlapping writes retain execution order.
  **140 light tests pass**, accepted data unchanged at **59,591 / 177,367**.
- Save/player manager setup passes three seeds; 0x00493AA0 remains 0/3.
  Its proof now reaches pointer cancellation at 0x00493C24.
- **Next action:** prove that affine cancellation without making unknown
  input registers or their flags concrete; boot stops retain priority.

### L28c fixed-frame checkpoint (2026-10-12, pending replay)

- **0x0081C330 records 3/3** after separating its fixed local frame from
  outgoing mock arguments. **137 light tests pass**; all 24 wrapper cases
  re-record identically.
- Private native-L28c: **59,591 / 177,367**, +10/+30 this session. Audit
  proves all 59,581 baseline records and 177,337 cases unchanged.
- Fifteen recorder residuals remain unfinished, alongside two other-lane
  entries and nine unconfirmed boundaries. The 0x00493AA0 proof needs both
  save/player manager state; either producer alone still rejects.
  **Next action:** checked composition of those manager setup states.

### L28c wrapper fixtures (2026-10-12, pending replay)

- Frameless virtual-tail wrappers use an opt-in mock argument strategy;
  **7 functions / 21 cases** record normally, including the string helpers.
- Private native-L28c: **59,590 / 177,364**, cumulative +9/+27; baseline
  records unchanged. **135 light tests pass**; full replay belongs to head.
- **Next action:** the separate 0x0081C330 stack-frame rejection, then
  remaining finite-work recipes. Details in L28 Results.

### L28c boot stop 15 bulk fill (2026-10-12, pending replay)

- **0x00DC7D00 records 3/3.** Constant-size modeled memset effects can stay
  lazy until accessed, retaining exact final byte ranges for existing replay.
  Ordinary page and bulk-byte guards remain; no Rust format change.
- Private native-L28c: **59,583 functions / 177,343 cases**, +2/+6 including
  the constructed array fixture. All baseline records/cases remain unchanged.
- **Next action:** head replay/boot preview, then wrapper/manager fixtures.

### L28c constructed array fixture (2026-10-12, pending replay)

- Original constructor 0x00490E80 supplies the four arrays for 0x00491340;
  the latter records **3/3** after checked setup replay. Guarded proofs can
  use setup-written bytes and trace stack-allocation helpers' moved returns.
- Private native-L28c: **59,581 -> 59,582 / 177,337 -> 177,340 cases**.
  Audit proves every old record/case unchanged. Manager trials retain their
  budget/cleanup blockers; no full Rust build or replay here.
- **Next action:** frameless virtual-wrapper mock input strategy, then the
  remaining manager and record recipes; details in L28 Results.

### L28c boot stop 14 fixture (2026-10-12, pending replay)

- **0x00DD5ED0 records 3/3**. The opt-in store-fact rule preserves its own
  vtable and member writes across non-overlapping closed calls; the repeated
  virtual call resolves to original 0x00DC67E0. Its proven bound is 83,137
  instructions. Initial object contents remain unknown, and alias guards stay on.
- Private native-L28c: **59,580 -> 59,581 functions / 177,334 -> 177,337
  cases**. The prior movie fixture and all other accepted data are audited
  unchanged. **119 light tests pass**; full replay is left to the head.
- **Next action:** preview boot past this constructor, then continue the
  remaining constructor/manager input recipes.

### L28c original setup-state fixture (2026-10-12, pending replay)

- Original reset 0x00F7B170 supplies the stopped movie state required by
  0x00F7DE40; the latter now records 3/3 normal-return cases. The general
  constructor/setup strategy checks the producer against its accepted case
  before transferring memory. It never assigns game-specific field values.
- Private native-L28c grows **59,579 -> 59,580 functions / 177,331 ->
  177,334 cases**. The audit proves all old records/cases unchanged.
  Six new synthetic tests cover the transfer and refusal paths; full Rust
  replay belongs to the integration train. Results describe all 37 queued
  entries' input requirements and distinguish state from cleanup/budget faults.
- **Next action:** boot stop 14, 0x00DD5ED0: preserve an alias-checked vtable
  fact across closed calls to resolve its repeated indirect call exactly.

### L28c inventory fixture batch (2026-10-12, pending replay)

- Nineteen more inventory functions record three normal-return cases each.
  Private native-L28c grows
  **59,560 -> 59,579 records / 177,274 -> 177,331 cases**; the per-address
  audit proves every old record and case unchanged.
- **22549c3b** adds opt-in, alias-checked caller-argument facts to the finite
  bound proof. **0x0075BAD0 records 3/3 and is adopted**; the other eighteen
  use existing zero-call rules (including five newly prepared entries).
  **105 light tests pass**; no full Rust replay. The owner's cleanup fix
  is integrated as **13446abb**.
- Results classify all 211 non-verified inventory entries, reserving the
  44 gaps and boot stop 12 for claude-6. Seven direct-route residuals remain
  (six ours plus his boot stop), and all 30 one-hop residuals remain after
  five-policy retries. **Next action:** head snapshot/adoption and train
  replay; then diagnose the remaining input/state requirements. Data is open.

### L28c register-object fixture (2026-10-12, pending replay)

- **0x00478600 records 3/3** after separating entry register objects from
  zero-return mocks. Its object is passed in ESI; the general input policy
  reuses the existing pointer palette and keeps all bound/alias/call guards.
- Private native-L28c: **59,559 → 59,560 records / 177,271 → 177,274
  cases**. All old records are unchanged. The strengthened SIMD-store
  guard also re-records all **33 old guarded cases identically**.
  **102 light tests pass**; the cleanup expectation is resolved.
- Retried the direct-route and remaining one-hop queue; eight direct and
  30 one-hop entries remain. Main is merged at **d386e757**.
  **Next action:** head replay/preview;
  DB8B90/4080A0 and the three L46 x87 helpers stay with claude-6.

### L28c guarded-contract follow-up (2026-10-12, original runs queued)

- The head's cleanup-test correction is integrated as **5016233c**;
  its module now passes 13/13. Checked indirect mock contracts and exact
  same-base store/read facts establish candidate bounds for 0x00478600
  and 0x00DD5ED0, pending original recordings.
- A negative test caught misleading Capstone SIMD-store metadata. The
  guard now explicitly handles memory destinations; a recheck of the
  previous 33 guarded cases is queued without replacing accepted records.
- **Next action:** obtain the heavy slot, record, compare and append/audit.
  Data remains **59,559 / 177,271**, open; L46 helpers remain untouched.

### L28c constructor/title fixtures (2026-10-12, pending replay)

- A guarded finite-trace strategy records eleven more constructor/title
  functions, three cases each. It traces closed callees and enforces
  unknown-write non-aliasing assumptions during original execution.
  Ordinary instruction limits and unknown-control refusals remain.
- Private native-L28c: **59,548 → 59,559 records / 177,238 → 177,271
  cases**; all old records/cases unchanged. **79 light tests pass**.
  One older cleanup-test expectation awaits its file owner's correction;
  the valid split cleanup and actual imbalance have a passing regression.
- Four requested constructors retain evidenced blockers (two indirect
  calls, one unknown condition, one unresolved terminal-path cleanup).
  **Next action:** head replay/boot preview and claude-6's cleanup evidence;
  details and exact residual addresses are in L28 Results. Data stays open.

### L28c archive boot stop recorded (2026-10-12, pending replay)

- `0x00419100` now records **3/3** cases. A conservative image-only
  abstract trace proves its **78,453-instruction** return path; the opt-in
  `image-bounded-v1` policy uses that proof plus the existing 20,000
  allowance. Unknown/input-dependent control retains the ordinary guard.
- Every original case registers all **13,061** entries from ten image
  tables correctly. Private native-L28c: **59,547 → 59,548 records /
  177,235 → 177,238 cases**. Hash audit: every old record/case unchanged.
  **49 light tests pass**; no full Rust replay or shared-data writes.
- **Next action:** boot preview, then the remaining constructor route;
  merge the lifter owner's integration branch before budget-area edits.

### L28c title-route batch audited (2026-10-12, pending integration replay)

- Final pointer-route trials add `0x00860A60`, `0x00874F90`, and
  `0x00B54BE0`, three fixtures each, using the same delayed-callee cleanup
  rule. Its no-stack-arguments regression passes: **43 light tests** total.
- This batch adds **13 functions / 39 cases** overall: native-L28c
  **59,534 → 59,547 records / 177,196 → 177,235 cases**. Cumulative hash
  audit proves all old records and case multisets unchanged. All four
  requested boot-route functions are recorded and reported for preview.
- **Next action:** integration replay/boot preview; coordinate remaining
  constructor budget proofs with the lifter owner. Trials covered all 32
  eligible one-hop and 29 two-hop candidates; 77 tested title/pointer
  residuals retain rejection evidence. No guard relaxed or quarantine
  overridden. The data set remains open per head's latest instruction.

### L28c next boot route recorded (2026-10-12, pending integration replay)

- `0x00DA9EC0` and `0x01104470` now have three original-code fixtures
  each. The indirect-call cleanup scan now crosses neutral register setup
  before the later argument push; branch and ESP barriers remain. The
  constructor's image-backed inputs avoid its byte-divisor zero at
  `0x01104491`. **42 light tests pass** including the new barrier test.
- Private native-L28c: **59,542 → 59,544 records / 177,220 → 177,226
  cases**; all old records/case multisets unchanged by hash audit. All
  four requested boot-route functions now have fixtures. The set remains
  open for preview, with full replay deferred to the integration train.
- **Next action:** resume boot and continue the remaining no-fixture queue.

### L28c render-call fixtures (2026-10-12, pending integration replay)

- Fixed mock cleanup when a later direct callee consumes arguments pushed
  before an indirect call. At `0x01128F55`, the saved diagnostic string
  belongs to `0x00DA89B0` (`ret 8`), not the COM call. The general rule has
  two synthetic regression tests; all **41** relevant light tests pass.
- Eight render functions now record, including boot-route `0x01129030`
  and `0x01129460`: **59,534 → 59,542 records / 177,196 → 177,220 cases**.
  Hash audit proves every old record/case multiset unchanged. Private
  native-L28c remains open for head's requested boot preview and additions.
- **Next action:** trace remaining boot stop `0x00DA9EC0`, then
  `0x01104470`; constructor loop budgets remain unresolved. No Rust replay
  here; the head verifies the integration train.

### L28c boot-blocker fixtures (2026-10-12, pending integration replay)

- `0x00DAFED0` now records **8/8** bounded record-range inputs, including
  ascending, descending, equal and mixed finite keys. Original recorded
  writes pass bounds, permutation and partition checks; no execution
  budget or recorder guard was relaxed. The reusable layout strategy
  seeds related first/last/output arguments before execution.
- Refreshed `0x00F6A380`, the sole imported caller still mocking closed
  `__alloca_probe` (`0x012F2280`). Private native-L28c:
  **59,533 → 59,534 entries**, **177,188 → 177,196 cases**; missing
  candidate fixtures **229 → 228**. Separate hash audits prove the refresh
  changed only that caller and the partition addition changed no old
  records. **39 light tests pass**; Rust replay is the head's train.
- **Next action:** integrate this frozen pair and resume the real boot;
  use its next executed missing function to prioritize the remaining
  title-path queue. Evidence and merge notes are in
  [L28 Results](tasks/L28-crt-platform-class.md).

### L45 function tails, gate and boot triage (2026-10-12, pending train 4)

- **Evidence:** table jumps proven on every path (both memcpy variants
  16/16, tails past Ghidra's end lifted), frame tails (`__VEC_memcpy`)
  and EH4 scope code (all 39 L27b scope targets) decoded into their
  functions. `gen` no longer declares functions with verification gaps:
  44 unproven tables remain, against 59,173 of 59,500 expected verified
  on the train-2 base. native-L45: 33 stale records refreshed in place,
  everything else hash-identical. native-L45b (train 3): 12 boot
  functions recorded (0x00DFECF0, `___crtExitProcess`, `_purecall`, …),
  audit ok. The discovery boundary rule drops 58 non-code admissions.
  L27b is merged on `task/L27b-integrate`.
- **Checked:** 134 Python tests plus `test_steps.py`. **Not checked:**
  `gen` and the Rust replay (train), `tests/options.rs`.
- **Blockers:** `__VEC_memcpy` has no vector (no input reaches its size
  range); x87 interior helpers 0x012F1C9D and 0x01304753; mock-return-1
  loops 0x00410000 and 0x00E0D750.
- **One next action:** train 4 merges native-L45 (train-2 base) and
  native-L45b (train-3 base) per address, regenerates and replays; see
  [L45 Results](tasks/L45-function-tails.md#results).

### L28b recorder batch (2026-10-12, pending integration replay)

- The opt-in `zero-call-v1` input strategy supplies a call from an aligned
  caller stack. Original wrappers 0x012F4E90/0x012F4EA6 each pass 3/3
  trials; `__alloca_probe` 0x012F2280 now promotes and all 23 existing
  mocking callers re-record. No recorder rejection check changed.
- Private native-L17: 59,090 → **59,115 selected entries**, 175,899 →
  **175,969 cases**, 317 → **292** missing fixtures. Seven additions are
  in TITLE_PATH.md, including 0x004709D0 (title-data loading). Hash audit:
  59,066 old entries/case multisets unchanged; only the promoted helper
  and its 23 callers refreshed, with no removals or stale mocks.
- **Checked:** 33 light Python tests and original-wrapper trials. **Not
  yet checked:** Rust replay (head's integration train). The image-table
  loop at 0x00419100 and stack-slot loops at 0x004786B5/0x00475540 remain
  recorder-budget blockers; see [L28 Results](tasks/L28-crt-platform-class.md).
- **One next action:** integrate and replay the boundary-changing pair;
  refresh any other lane's cases that still mock 0x012F2280.


L27b continuation batch (2026-10-12): production instruction emission and
Registry/Session routing now deliver transactional AV/#DE and transfer across
service callbacks. Light checks pass: 94 runtime + 7 base + 52 lifter tests;
12 emitted execution fixtures include original `0x01303CB9`/`0x013070E5`
after RtlUnwind. Windows oracle passes. Blocker to claiming all real __except
paths: 39 of 58 census filter/body addresses are absent from the selected
instruction keys. Next action: train 3 regeneration/replay and lifter-lane
scope coverage; see [L27b Results](tasks/L27b-seh-faults.md#results).

L27 runtime SEH batch (2026-10-11): read-only census identifies 42 EH4
registrations / 48 scope records, seven C++ FuncInfo thunks and five throw
sites. Per-thread TEBs, software exception search/nesting and target-frame
unwind are implemented; Windows x86 oracle confirms layout, handler order,
nested flags, modified divide context and unwind behavior. All 70 runtime
tests pass (14 SEH regressions, including the original `_except_handler4`;
three existing install-dependent tests ignored). Blocker: generated instruction EIP and
nonlocal continuation are absent, so hardware-fault delivery and original
__except transfers remain incomplete. Next action: route the executor
contract in [L27 Results](tasks/L27-seh-dispatch.md#results) to the lifter lane.

**Current focus (2026-10-09): the opening slice.** M5 is now a vertical
slice from the new game to the Encampment, played in co-op; see
[OPENING_SLICE.md](OPENING_SLICE.md). Other systems advance only where the
slice needs them.

Each milestone is a route the players can actually do, with a gate checked
against the original game. Co-op comes early (M3) on purpose: it validates
the architecture before many systems are built on it.

| # | Route | Gate (checked against the original) |
| --- | --- | --- |
| **M0** | Foundation: tooling, exe identity, DTI registry in Ghidra, `.arc` reader, data inventory | Every `.arc` opens; DTI names applied (**passed** 2026-10-08) |
| **M1** | Look: Cassardis and its surroundings render like the original (terrain layers, lighting, water, fog, sky, weather, time of day) | Screenshot comparison at recorded camera poses, morning/noon/night |
| **M2** | Walk: the Arisen moves, animates, collides, climbs and spends stamina like the original | Recorded paths, speeds, jump arcs and stamina curves match within tolerance |
| **M3** | Together: LAN host and client, two Arisen in Cassardis, shared main pawn | Both see each other move smoothly; client input applied by the host |
| **M4** | Fight: melee and magic, one enemy family (goblins), status effects, the main pawn fighting and talking | Damage, stagger, AI choices and pawn chatter match a recorded fight |
| **M5** | The opening: character creation, the prologue, the Cassardis dragon attack, the first quests | The route plays through; quest and flag state matches the original's save after each step |
| **M6** | Gran Soren and the core loop: shops, forging, inns and storage, the Rift and hiring pawns, notice-board quests, NPC schedules, day/night, fast travel | Shop stock and prices, Rift costs, schedules and quest flags match the original |
| **M7** | Gransys: every region, enemy, vocation, skill and augment; streaming and loading screens | Every area loads; spawn tables and stats match the data and recordings |
| **M8** | The campaign: every main and side quest, both endings, the post-game Everfall | Full playthrough; save state comparable with the original at each chapter |
| **M9** | Bitterblack Isle (Dark Arisen) | Full Bitterblack run |
| **M10** | Release quality: settings, saves, performance, FPS unlock, co-op polish | Stable sessions; no regressions on the acceptance routes |

Research runs ahead of the active milestone. The systems are broken down in
[ROADMAP.md](ROADMAP.md), work items in [tasks/](tasks/README.md), and the
multi-agent process in [BOARD.md](BOARD.md). The original game is observed
through the in-process oracle ([tasks/oracle.md](tasks/oracle.md)) and the
reference recordings in `E:\ddda-re` (private).

### Research documents

| Topic | Doc | Status |
|---|---|---|
| Formats | FORMATS.md, TEXT.md, PARAMS.md, ENEMIES.md, SURVEY.md | merged |
| World | WORLD.md | merged |
| Movement, animation | MOVEMENT.md, ANIMATION.md | merged; ledge and blend in progress |
| Shading | SHADING.md | in progress (claude-3) |
| Pawns and the Rift | PAWNS.md | in progress (claude-5) |
| Shops and economy | SHOPS.md | in progress (claude-6) |
| Events, cutscenes, quests | EVENTS.md, QUESTS.md | in progress (claude-7) |
| AI | AI.md | in progress (codex-5) |
| Weather, NPCs, GUI, map, sound, effects | WEATHER/NPCS/GUI/MAP/SOUND/EFFECTS.md | assigned to the Codex lane |
| Live oracle | ORACLE.md | in progress (claude-4) |

## Status

**Done: M0** (2026-10-08)

Done:
- JDK 21 (Temurin 21.0.12) installed; Ghidra 12.1.4 runs headless.
- Analysis copy of DDDA.exe unpacked with Steamless 3.1.0.5 (SteamStub 3.1).
- Exe imported into `E:\ddda-re\ghidra\DDDA.gpr`; auto-analysis done
  (11.5 min, 33,870 functions). Backup in `E:\ddda-re\ghidra-backups`.
- GhidraMCP 6.0.0 built against 12.1.4, running on 127.0.0.1:8089,
  registered with Claude Code as `ghidra-ddda` (user scope).
- Repo created with a whitelist `.gitignore`, AGENTS.md, method and co-op
  design docs.
- Format survey (`E:\ddda-re\notes\formats-survey.md`): 8,536 `.arc` v7
  archives, 358k zlib entries, 144 resource types; MIT specs available
  (Albam, Riftstone).
- DTI class registry harvested: 4,405 classes, 4,343 vtables, 25,791
  virtual functions named ([DTI.md](DTI.md)). Reproducible from
  `research/dti` (identical label set verified).
- DTI labels applied to the Ghidra project. Vtable runs trimmed at the
  first non-code word (143 vtables); the 57 functions an earlier run had
  created at non-code addresses were removed. Final state 2026-10-08
  20:10: 30,540 functions and 13,153 data labels named, 0 failures.
- `mtarc` crate (ARC v7 reader) and `ddtool` CLI. **Gate passed
  2026-10-08:** `ddtool arc-scan <nativePC> --inflate` opens all 8,536
  archives, inflates all 358,431 entries to their recorded sizes, 144
  types all named from the DTI registry, 0 failures, 33 s.

**Active: M1** (Look: one area with a free camera)

Done (format readers; checked by re-running the scans 2026-10-08 20:05):
- `mttex`: all 73,706 `rTexture` v0x99 parse. Format byte to D3DFORMAT
  is the exe's own table at `0x01440698` (byte-for-byte; one copying
  slip at index 4 fixed).
- `mtmod`, `mtmrl`, `mtshader`: all 28,094 models (70.2M vertices, 71.3M
  triangles) and 28,152 material lists parse. All 2,077,947 material
  commands resolve through `BbsShaderPackage.mfx`, which also gives the 24
  vertex layouts. Format notes are in [FORMATS.md](FORMATS.md).
- World placement rule from the live game ([WORLD.md](WORLD.md)).

Open data anomalies (unexplained, not yet ruled harmless):
- 123,205 of 70.2M normals off unit length by more than 0.05.
- 25,811 skinned vertices reference a joint at or above the bone count.

Viewer (`ddvfs`, `ddview`), first renders 2026-10-08 20:30:
- Fixed before it would build or show anything: a compile error, the cell
  model path (folder comes from A, not B), corrupted build artifacts
  from the drive failure (`cargo clean`), and screenshots taken before
  Bevy's async pipeline compilation finished (`--shot-delay`, default
  10 s).
- 5x5 cells around `st100_58m50n` load and join seamlessly (25 cells,
  231 meshes, 56,527 triangles, 28 textures, 0 problems): the placement
  rule holds. Triangles are CCW-front against the stored normals (12,000
  vs 2 in four cells), so Bevy's default culling is right.
- The ground-level shot from the live player pose shows plausible terrain.
  **Not yet compared with the original game**: no reference screenshot.

Reference comparison (2026-10-08, private `E:\ddda-re\shots\refs`):
- Screenshot listener records the live camera pose (uCameraGame +0x30..+0x6c)
  at each F12. Six usable references from two areas; poses checked by a
  ground fit (player feet on the origin cell's terrain within 1 cm for
  ref001, 002, 004, 005). ref006 unverified, ref007 uncertain.
- **No mirroring.** Terrain, coast and sea sides match in every reference.
- Static scenery (`_s00` layouts via `mtlot`) lands where the original has
  it: a sword prop (ref001) and a large tree (ref004) match exactly.
- Water (sea plane at y 30000 plus river/lake/wave models, stage frame) and
  a ring of distant `fmfore` cell models are drawn. Water material is a
  placeholder.

M1 gate comparison (2026-10-09, branch task/m1-gate; side-by-sides in
the private `E:\ddda-re\shots`, rendered with `ddview --ref`):
- Since the list above: terrain layers, the original's lighting (SH,
  sun with shadow map, linear fog), the stage filter chain (vignette and
  tone curve), the Rayleigh/Mie sky dome, water materials, the castle and
  the other `uFmSetBase` landmarks, the far cell terrain, the stage
  low models (`st100_all00` and the area backdrops: the horizon mountains)
  and the primitives' pass and LOD rules are drawn ([SHADING.md](SHADING.md)).
- Without `--light` the lighting, fog and sky come from the game data for
  a clock and weather (`--clock`, `--weather`; default 12:00, fine).
  The sun light colour reproduces the four live day reads within 1 %.
- ref001 (castle view, clock unknown, at 12:00 fine): mean colour of the
  frame (8-bit sRGB) ref (97,103,103) / ours (100,107,107); distant
  mountain (106,142,186) / (104,140,185); mid ground (110,127,139) /
  (110,129,142). The haze, the horizon mountains and the cypresses match.
- ref002 (coast cliff, 12:00 fine): frame (98,106,108) / (113,120,121);
  the party, the HUD and the event chest are not drawn.
- ref004 / ref005 (coast with a tree, 12:00 fine): frame (80,92,89) /
  (113,122,111) and (105,116,119) / (119,129,130). The original's clock
  is unknown; its tree shadow shows a lower sun than 12:00.
- refs-lit 12:21 and 17:45 (live lighting, cloudy weather found from the
  fog): ground (30,39,40) / (31,40,41) and (26,34,34) / (25,34,35); a
  second ground box matches to the level.
- Night and water (2026-10-09, branch task/m1-night-water): the sea is
  unlit forward with the environment cubes (traced shaders), the cloud
  domes of the weather turn the horizon blue under the fog, the sky spot
  light lights the ground at night, and the stage DOF is drawn. At 12:00
  fine: sea ref / ours ref001 (63,78,96) / (48,62,77), ref004 (56,68,78) /
  (44,55,62) (was (74,97,116)); horizon ref004 (142,166,189) /
  (134,153,169) (was (184,184,184)), ref002 (138,160,183) / (129,146,163);
  refs-lit 23:04 ground (5,9,11) / (5,13,14) (was (1,4,5)); the day
  captures are unchanged (within 1-2 levels).
- Gaps: stars and the moon; night point lights (lamps, lantern) and the
  moon light; the water's normal maps and the foam effect; characters and
  event objects. Captures with the clock and weather recorded (sea and
  sky in view, a lamp-lit night) would let refs 001-005 be compared at
  their real time.

**Started: M2** (Walk), alongside the M1 shading gaps, at the maintainer's
request (2026-10-08). See [ANIMATION.md](ANIMATION.md).

Done:
- `mtlmt`: every motion list decodes (4,313 files, 92.2 M keys) with value
  checks (key spans, unit quaternions).
- Skinning fixed and explained: skinned vertices use a per-model skin space
  D (`inverse_bind x rest world`, identical for every bone of 400 models),
  not the bounding box; `IASkinBridge2wt` joint bytes.
- `ddview --character`: the male body plays motions. Slot 0 of
  `m0000_co` is a standing idle and slot 3 a walk cycle; correct by eye,
  **not yet compared with the original game**.

- Motion ids from the exe: `cPlActWait` / `cPlActWalk` / `cPlActRun`
  request motions 0 / 3 / 4 (`bank << 8 | slot`, bank 0 = `m0000_co`),
  confirming the slots identified by eye; walk starts on frame 0 or 18.
- Root motion (joint 255) drives movement: walk 198.5 cm / 38 frames,
  run 423.0 cm / 22 frames (1.57 and 5.77 m/s at the assumed 30 fps).
- `ddsim` (headless, N players): input -> action (wait/walk/run) ->
  root-motion movement -> ground height. `ddworld::Terrain` gives ground
  height from the cell terrain; it matches the four verified references'
  recorded feet within 1.1 cm.
- `ddview --play` / `--demo`: two local players (P1 WASD + LShift, P2
  arrows + RShift), follow camera on P1. Seen working with scripted input.

Not yet: wall/object collision (SBC), turning rate, walk/run thresholds,
blending between motions (the original blends; we switch), the other
motion banks, a dressed Arisen.

- `mtsbc` reads every collision mesh (7,362 files, 3.28 M triangles, all
  normals consistent). `--play` now stands on the cells' collision meshes
  (within 1 cm of the references) and is blocked by faces higher than a
  step (STEP_UP 60 cm, placeholder).

- Movement compared with a recording of the original (read-only memory,
  2026-10-08): walk, run and sprint move by a velocity from
  `m_pl000_00` (`rAdjustParam`), not root motion. Run velocity matches the
  recorded 10.165 cm/frame exactly; ground speed is 3-10% under the
  recorded path speed (open). Jump apex 92.6 cm vs 93.1 recorded. The
  simulation steps at 120 Hz. Town and field walk/run variants. Sprint-start
  ramp. Second jump plays the dagger roll. See MOVEMENT.md.
- Walls: players collide with steep triangles of the stage collision and
  slide along them. Only the `h` meshes are movement collision (`e` is
  `cSplitSbc::mpClsnEffMrgRsrc`, effects). Big trees block; bushes and
  shrubs have no stage collision, as in the data.

Blockers: none. Open: turn rate, run/sprint stop motions, ledge detection
from `cPlCliffHangBegin`, motion blending.

Next action: maintainer playtest of `--play` (speeds, walls, ledges); then
ledge detection from the exe.

## Batch 2026-10-09 (overnight, multi-agent)

The head coordinated about 20 agents through the board (`docs/BOARD.md`).

**Merged into main** (each checked by `scripts/merge_task.py`: no
conflict markers, tests pass):

| Area | Briefs |
|---|---|
| Readers | text (`mtgmd`), player params (`ddparam`), enemy data (`mtprp`, `ddenemy`), AI state machines (`mtfsm`), weather (`mtweather`), map and travel (`ddmap`), GUI (`mtgui`), NPC links (`ddnpc`), sound (`mtsound`), effects (`mteffect`) |
| Research | pawns and the Rift (`ddsave`, `ddpawn`, PAWNS.md), stamina rules, asset survey (SURVEY.md) |
| Core | ledges (ddsim, `Ground::ray`/`sphere`), motion blending (`mtlmt::blend`) |
| Tools | the in-process oracle (`research/ddda-oracle`, ORACLE.md), MtDTI reflected fields (`research/dti/dti_props.py`) |

**Compared with the original:**
- The clock: 1/30-s units, scaled by a per-frame `mDeltaTime` from the QPC
  timer (RECORDINGS.md; the formula was traced by codex-11).
- Sprint stamina: -1.5 per frame. Recovery: +1.625 per frame, after a
  0.59-0.61 s hold.
- Keyboard turning is instant.
- Blend lengths and countdown, through an oracle trace (ANIMATION.md).
- The offline Rift pool matches the level table, and hire costs are
  checked against a live read (PAWNS.md).

**In progress:**
- Claude lane: terrain shading (claude-3), shops (claude-6), events and
  quests (claude-7), combat (claude-8), vocations (claude-9), pawn builder
  (claude-5).
- Codex lane: rift-ui, save-pawns, enemy resource selection, weather
  clock, travel arrival.
- Live: the ledge jump/fall rule (claude-4).

**Blockers:** none. The mage save limits live checks of vocation-specific
moves such as the dagger double jump.

**Next action:** finish the M1 shading gate and the M2 gate. That means
recorded paths, speeds, jump arcs and stamina through the oracle against
`ddsim`, as an automated acceptance test.

## Batch 2026-10-09 (morning)

**M1 (look):** blended terrain layers, lighting from the game data by
time and weather, the sky's scatter shaders, the stage's tone curve and
vignette, water materials and reflections, the backdrop models, the far
ring, foliage billboards, depth of field.
- ref001 is within 3-4% of mean colour.
- The day captures are within 1-3 levels.
- Open: stars, moon, lamp lights (they need a Cassardis night capture,
  waiting on the maintainer), the Bluemoon Tower detail.

See SHADING.md and the M1 section above.

**M2 (walk):** acceptance replay (`ddaccept`) at 40 of 44 rows, up from 16.
- Done: velocity from data, the motion speed rate, stamina, the sprint
  stop, falls, landing holds, load and body classes, and the
  `cScrAdjust` capsule sweep and slide.
- Open: the drop path after frame 195. It needs a live trace, and the
  game is paused on the maintainer's fight.

**M3 (together):** `ddhost` (FRAME.md, ADR 0002), with economy, Rift and
progression wired in, and two-player save/load with stable hashes. `ddnet`
LAN co-op passes loopback tests under 10% loss and 40 ± 20 ms (prediction
error ≤ 5.7 cm). `ddview --host/--join` is in progress (claude-10).

**M4 (fight):** `ddcombat` covers hit volumes on joints, the exact shape
tests, hit history, the damage formulas and region pools; the goblin's
first swing hits exactly once, on frame 45. `ddai` runs the goblin's FSM.
The think tables are in progress (claude-9). Status effects: build-up
only.

**M5/M6 groundwork:**
- `ddstory` runs the opening (q0000 → q0002) in the traced frame order.
- The event player plays st200 ev10 with its cameras, subtitles and NPC
  outfits.
- Characters are assembled from the save: one skeleton, part masks,
  morphs checked bit for bit against the exe, weapons.
- `ddeconomy`, `ddrift` and `ddprogress` runtimes exist.

**Next actions:**
- the M3 GUI join test;
- the M4 think tables;
- play from the save in `--play`, once claude-10 releases `play.rs`;
- the live trace for the drop path.

**M3 two-window check (head, 2026-10-09 14:16):** on one PC, a host and a
guest started from the same save:

```
ddview --host 47812 --save <save> --goblin --demo
ddview --join 127.0.0.1:47812 --save <save>
```

- The guest joined within 5 s, at the host's spot (pre-join).
- Both logs report identical positions for seats 0, 1 and 4, all three
  dressed, and the host's goblin replicated to the guest (HP 0/1000,
  dead).
- The host's screenshot shows the guest's dressed Arisen beside its own
  party. The guest's shows the host's Arisen and pawn.
- Not checked: two physical machines, or a guest with a different save.

**M5 opening slice (codex-18, merged 0c291a6, 2026-10-09):** the
Cassardis opening plays through in LAN co-op with no teleports:
- st200 ev10 → control → Quina conversation;
- a collision-only, 197-waypoint walk into the village → ev15 → q0002
  boot;
- the end state matches the original's: SceNo 1400, GSF 372 and 381 set,
  q0001 bits `[18000000, 0, 3]`.

The guest shows the faces and subtitles, and the host's and guest's
cameras restore the gameplay projection after each event. Test:
`ddview` 15 release tests plus the Cassardis and story-loopback
regressions.

Open:
- the prologue fast-forward is a stub;
- guest dialogue and menus;
- the event freeze for NPC group 57 (Quina moves during shared events);
- the elder's ear and brow culling (cutscene-visuals);
- an ev15 frame-300 view of roof and sky.

**Batch 2026-10-09 evening (head + Codex lane), all on main:**
- **Movement feel (head):**
  - turning recorded with the oracle (adjust 3);
  - drawing between fixed frames (`play::Between`);
  - air attacks (rows 0x302);
  - the hard landing with fall damage.
- **Field streaming and the stage handoff (head):**
  - `ddworld::stream`: the grid comes from the exe, the 5/5/3 windows from
    0x00507440, the predicate from 0x005075d0;
  - `Streamer::enter`: door scenarios and JumpArea, with test
    `the_cassardis_gate_arrives_on_field_ground`.
- **Movies (head):** `mtmovie` decodes the WMVs with Media Foundation;
  `ddview --movie st100ev00` plays them.
- **Character edit, first version (head):** `ddview --charaedit`; the 41
  fields rebuild the body through `edit_look`; Enter fills `EditResult`.
- **Audio (codex-1):** event streams, conditional voices, motion sounds by
  surface, random selection.
- **Opening slice:**
  - prologue setup and encounter facts (codex-27);
  - q0002 records, live facts and the opening's stage handoff (codex-26);
  - dragon AI, resources, body and hover (codex-25);
  - chimera catalogue and multi-part damage (codex-28).
- **Pawn-actor brief:** saved pawn records into active fighters (codex-1).
- **Incident:** E: dropped off at 15:11 (a loose SATA cable). The repo was
  verified with `git fsck`. Bundles are now kept in `C:\ddda-backup`.

Blockers:
- the chimera fight (prologue);
- the dragon fight;
- the Rook actor (pawn-actor);
- the NPC applied controllers for q0002 (codex-26);
- the opening hookup of the movie and the editor.

Next action: q0003 trace (road to the Encampment), head.

**Batch 2026-10-09 night (head, from the maintainer's playtests):**
- **The game clock in play** advances at the original's rate (0x0043f2c0)
  and relights the stage and sky each game minute, spread over frames.
- **Performance:**
  - the clock's relighting was the 400-650 ms stutter;
  - the sun shadow now follows the stage's `uSkyStableShadow`: one map over
    60 m instead of four cascades over 1 km;
  - whole-stage parts respect their shadow-cast flag;
  - the distant ring is 6 cells.
  - Cassardis walk 46 → 12 ms; field 18 → 11 ms. `--perf` logs frame
    times and the main systems.
- **Cassardis rendering:**
  - one LOD per model, measured to the model's bounding-sphere centre
    (0x00ea5320, 0x00ea3d00; `mLODType` automatic from 0x00c25234), which
    fixed the z-fighting;
  - the opening's camera gets collision.
- **Camera collision as the original** (0x007de570): a ray, then a 1.3 x
  near-plane sphere.
- **Tools:** `--probe` (F8 crosshair, F9 logs the surfaces under it,
  drawn or hidden, with LOD masks).

Briefs queued for the Codex lane: camera-collision follow-ups,
character-shadows, skill-vfx.

**Batch 2026-10-09 late (head + Codex lane, from the maintainer's playtests):**
- **Floor shadows:** the original's two `uAmbientShadow` per player body
  (0x00B565A0), drawn straight down from joints 0x10/0x14 (ankles), 50 cm
  wide, 1-100 cm deep with both decays (vf11 0x00E7B6F0, 0x00E7AB30);
  blend state still a viewer choice (shadows.rs).
- **Camera:** sweep from the target outward to the nearest obstruction
  (0x010FDEA0, 0.1 cm response, attribute 0x10 ignored), Cassardis
  collision keeps its flags, the near-camera actor fade (0x0088BDC0
  thresholds), actor-offset shift traced (codex-30).
- **"Missing ground" in Cassardis is the original's own hole**, hidden by
  an auto-closing door (maintainer, in the original). The LOD rule was
  confirmed live on the field (codex-29: camera eye to the model's world
  sphere centre, rModel mid/low, ViewRange NORMAL = 1). New brief: doors.
- **Arisen look:** the omitted cBodyEdit controller (37 joints, sex tables,
  slender/muscle/bust) and the hand-joint scale rule (codex-1).
- **Opening route:** dragon pass C, chimera ThinkInitialization, NPC
  placement/intel lifetime/name families/Guard contracts (codex-1/28/31);
  the route harness reaches the real gate and the field (codex-29), next
  blocker the field detection-range rule 0x00443A60 (codex-1).
- **Perf:** `--perf` render diagnostics; room median 6.9 ms; field spikes
  up to 55 ms under investigation with the route harness.
- **Tools:** the probe logs collision distance per hit.

Next: doors (codex-32), field detection rule (codex-1), NPC adapter in the
opening (codex-31), route perf (codex-29).

**Batch 2026-10-10 (head + Codex on Sol 6.1 with step tasks):**
- **Process:** the Codex lane now runs on step tasks (`docs/tasks/steps/`,
  S01-S28): one moderate problem each, with known facts and a done check;
  head reviews, merges and writes the next ones from the Results.
- **Classes:** the Strider (dagger list 0x014F86B8, AirS1/AirS2/AirL, rows
  0x38-0x3A, cost 0x1BA, check 10 height 0x75; kit lvstrider + dagger
  0x195); the weapon table's choice applies the vocation's kit
  (`Host::apply_job_kit`) and starting weapons on the body. The bow is
  traced (secondary list 0x014F8198, command 7 / bit 0x8, `cPlActWpnBow`
  vtable 0x015E2F08) and queued (S25-S27).
- **Combat in the opening:** the opening host has a fight; field cells'
  gated enemy groups spawn into it; party HUD; the small-enemy gauge
  (last damaged goblin, 450 frames, 0x0067C110); hired pawns (Rook) get a
  body, a fight seat and their record's look.
- **Cutscene faces:** skinned parts were culled by rest-pose bounds (S10,
  S11: ear, hair, brows); the face's special constraints and their joint
  order (S12, 0x00812330, 0x01054540..0x01054690).
- **Opening flow:** the movie at q0000's awaited PlayEvent with the traced
  skip input (S08, S16) and its subtitles (S19); field NPC admission
  including the global (0,0) layouts (S13); the second Arisen dressed (S20).
- **Cyclops (on hold with the Encampment):** action enter, placement HP/legs,
  think driver, navigation services (S05, S06, S14, S17).
- **Blocker found:** after ev15 the story waits for NPC 1's Guard TalkEnd
  (S23, in progress: the authentic bb3290 fallback).
- **Plan:** the prologue is for later; the dragon attack is required (S24);
  the Encampment waits for the maintainer's playtest.
- **Disk:** E: fell to 27 GB; the 43 GB stale debug target was removed.

**Batch 2026-10-10, afternoon (head + Codex children 54/58/60/61/62):**
- **Route:** NPC 1's Guard TalkEnd -> GSF381 -> q0002 boots (S23); field
  goblins bind to event1's EnemyCheck and killing group 23 sets q2:20
  through the authored path; Reynard keeps his body after the admission
  LOT closes (closed spawn gate != removal, 0x0075F860 vs 0x0075FAA0,
  S32) and plays his authored motions; his thanks, reward 1058 and q2:88
  run from n304QuestCtrl (S39); the demo walks into event1's Area
  (RescueApproach) and on toward q0002_event2 (RoadWalk).
- **Enemies:** goblin RunTrace/RunEnd/RunStumble (far goblins used to
  stand still); the vf33 stat defaults (HP x2 -> 2000, attack x1.3) and
  the placements' cSetInfoPawn rates (rescue goblins attack x0.2);
  experience 100 per goblin (or mExperienceOW) through the party award,
  level-ups reaching the fighter (S37 core). Reynard as a combat body
  with source sensor selection and NoDamage/Invincible gates (S34, in
  progress).
- **Weapons and items:** the Strider's bow: mode and phases (S25), arrows
  that hit from the real bow joint (S26), in the viewer with the TPS
  camera and layered upper-body motion (S27); the new-game bag, rewards
  and quick use (S28).
- **Dragon:** host actor with its entrance (S29); landing/PressAttack and
  breath in progress (S30); LAN guests see every enemy kind, protocol 7
  (S33).
- **Viewer:** failed runs exit non-zero; `--no-vsync` (implied by perf
  runs). Field perf, uncapped: median ~22 ms, our systems ~3 ms; the rest
  is render-side (S41).
- **Next steps queued:** S35 vocation GUI, S36 pawn commands, S38 item
  menu, S40 event cameras (camera 12 = cCamExParamAnim, 3 s, blocks the
  player), S41 frame rate (180 fps target), S42 graphics settings, S43
  level-up notice.

**Batch 2026-10-10, evening (head + Codex 61..78):**
- **Rescue playable as authored:** enemy perception (S49: sensor queries,
  alert states, hate decay); Reynard's HP from his placement (x2.5 = 2500,
  0x00BBC4E0); the native gauge veto of party hits on gauged NPCs (S54,
  0x00BB7680, region-independent; the HP1/escape civilian rule is Japan-only).
  From the RescueApproach checkpoint: q2:20, only rescue goblins hurt him.
- **Tutorial UI from the install:** vocation menu (S35, uGUIJobChange),
  level-up notice (S43), item menu over host bags (S38), graphics settings
  (S42: own INI, read-only import, live VSync/DoF/shadow/sampler/FOV),
  goblin appearance and equipment by placement (S47: early goblins bare).
- **State and co-op:** host-owned bags saved with checkpoints (S51); running
  quest scripts saved/restored (S48, native STOR + optional STCX); profile
  parking on leave/rejoin (S55); LAN quest cameras, NPC combat state and
  stage (S44, protocol 9); pawn orders Go/Come/Help (S36, protocol 8).
- **Tooling:** route checkpoints and resume (S46, `DDVIEW_OPENING_FROM`),
  `DDVIEW_TIME_SCALE` for unattended runs, GPU check policy (one batched
  run, bounded, from checkpoints), board status fix.
- **Performance (S41):** named Bevy traces; compact per-submesh skin palettes
  by default: skin upload 17.8 -> 2.05 MB/frame (-88%), field median
  14.2 -> ~11 ms (GPU clocks covary; no isolated fps claim).
- **Whole route, one run (2026-10-10):** the unattended x4 route plays from the
  intro room through Quina, the gate, the field, the rescue (q2:20, Reynard
  1594/2500 HP, Arisen level 2), Reynard's talks and the road to q0002_event2:
  q2:10 and q0003 boot, exit 0 in 2 min 24 s wall. It needed one fix: the
  push-out moved the Arisen without the ground sweep (0x0076D6B0 runs before
  the scenery sweep), so a goblin push uphill buried the body 17 cm under
  the slope and froze RoadWalk (`Sim::push_player`, cbc5b85). Route-v2
  checkpoints regenerated from this run, Done included.
- **Merged on the way:** S30 static coordinate basis and primary parent
  binding, S53 death helper with atomic `Host::load` prevalidation, S41
  finite shadow caster extent (opt-in; no measured gain).
- **Pawn chatter (S52, protocol 10):** the installed situation/message
  tables, native recognition clocks from the seated cameras' views (union
  over local seats, no proximity fallback), reliable per-profile
  PawnMessage, and the opening's presenter hooks (de93e84).
- **Character creation (S56):** the installed editor flow: sex, name with
  the exe's charset/blacklist rules, 16 installed presets, confirmation;
  per-profile EDIT in the save; names on the S35/S43 GUIs. Two-profile
  capture checked.
- **Death and retry (S53, protocol 11):** grounded death phases, game over
  from an authoritative DeathReport, atomic retry (candidate host, then an
  infallible swap; live roster and connections kept), the six native vital
  fields saved per profile (VITL), epoch-tagged reliable messages.
- **Pawn voice and names (S52):** voices read from `rom/sound/pwn.pck` with
  the exe's own CRC32 path lookup; story pawn names localized like the
  original (English "Rook"); checked in an actual capture.
- **Beach scene groundwork (S31):** native Demo reactions, NPC capsules from
  the installed adjust table, damage edges, and the native editor exit to
  stage 200/start 3 with the prologue-end flag that gates it.
- **Dragon fire (S30):** coordinate basis/parent binding, generator inputs
  and initialization, each step checked against the original's instructions.
- **New steps:** S57 base vocation starting skills (codex-81), S58 equipment
  menu, S59 navigation (codex-82; the beach movement needs it), S60 the
  tutorial's numbers against the running original.
- **Vertical slice wrap-up (2026-10-10 evening):** final unattended route on
  main (after S53/S52/S31/S30/S59 merges) plays intro room -> Quina -> gate
  -> field -> rescue (q2:20) -> Reynard -> road -> q2:10 + q0003 boot, exit
  0, 2 min 30 s wall at x4. Route-v2 checkpoints regenerated from it.
  Run: `DDVIEW_DEMO_FIGHT=1 DDVIEW_TIME_SCALE=4 ddview <nativePC>
  --perf-route --unattended`; resume any step with `DDVIEW_OPENING_FROM=
  E:/ddda-re/notes/checkpoints/route-v2/<Step>.ddhs DDVIEW_OPENING_STEP=<Step>`.
  After the wrap-up, head ran the paused gates itself: S30 (198 pass) and
  S59 (6 pass) merged; main re-checked (204 pass, viewer builds). Still on
  branches: S31 beach scene (`step/s31-dragon-route`, exit helper; GotoEx
  needs the S59 query wired to bodies), S57 starting skills
  (`step/s57-base-skills`; blocked by the arrow supplement-row load error,
  see its brief; Shield wire tag and protocol 12 added there).
  Not started: S58 equipment menu, S60 oracle numbers, S41 frame rate.
- **Open:** dragon fire emission/damage (S30), beach scene wiring (S31,
  codex-80), navigation (S59), starting skills (S57), frame rate toward
  180 fps (S41: ~10.6 ms field frame vs 5.5 ms), Ghidra MCP down since the
  low-memory stop.

## Batch 2026-10-11 (L01: native lifter switch tables)

- **Implemented:** strict absolute dword switch tables, including byte-index
  remapping; image-derived targets become block leaders and runtime jumps
  read through `Mem`. Unknown targets panic. Scan: 29,165 -> 30,111 liftable
  functions; indirect rejections 2,107 -> 776. See
  [L01 results](tasks/L01-lifter-switch-tables.md#results).
- **Tested / compared with the original:** 12 synthetic tests pass; required
  Rust vector replay passes 4,551 cases over 1,525 functions, including 931
  new switch functions and all 48 promoted callers. Table reads occur in
  274 functions (37 byte-remapped), across 300 distinct entries. This is
  sampled coverage, with external callees mocked where documented.
- **Verification tooling:** opt-in zero boundary inputs improve switch-path
  coverage; closed sparse runs now reject execution of non-image bytes,
  after one invalid older cached fixture was reproduced. Private L01 data
  only; generated Rust/test files are not part of the source handoff.
- **Blockers / limits:** 15 newly liftable functions have no usable vector;
  remaining indirect forms stay rejected. External-call vector caches need
  invalidation when the callable set changes.
- **One next action:** build structured original-exe fixtures for the 15
  unverified functions listed in `E:/ddda-re/notes/native-L01/unverified-new.json`.

## Batch 2026-10-11 (L03: native lifter codegen size)

- **Implemented:** in-place Cpu state, exact binary-ALU helpers, conservative
  straight-line bodies, and limited debug info for ddnative tests. Cached
  instruction semantics and the 25,609-function selection remain unchanged.
  Generated Rust falls 109.94 -> 79.55 MB; clean lifted-test build falls
  1,040.73 -> 275.91 seconds and touched-module rebuild 113.96 -> 63.24 seconds
  with four Cargo jobs. See [L03 results](tasks/L03-lifter-codegen-size.md#results).
- **Tested:** 17 Python regressions and both Rust library tests pass.
  A 9,476-case subset passes with private call-boundary-corrected fixtures.
  Full replay of the unchanged prepared 76,533 cases fails the same 23
  functions before and after codegen changes; no newly failing functions.
  Generated code, tests and private data are excluded from source commits.
- **Blockers / limits:** the required all-green full replay is not achieved.
  A private recorder diagnostic clears 19 existing failures (indirect calls
  to closed targets and recursive calls were not mocked as the lifter
  expects). Four still fail with either emitter and fresh recordings:
  `0x00C1D790`, `0x00D4E940`, `0x00D9EF20`, `0x00FB9A10`. This is sampled
  vector comparison, with external callees mocked, not gameplay comparison.
- **One next action:** the head should resolve the recorder call contract
  and the four remaining baseline mismatches, then rerun the full replay.

## Batch 2026-10-11 (L05: native lifter x87 arithmetic)

- **Implemented:** hardware-backed 80-bit x87 registers, control/status,
  arithmetic, conversions, comparisons and SAHF/LAHF, preserving host state.
  Scan: 30,527 -> 31,977 liftable functions; rejections 7,201 -> 5,751.
  See [L05 results](tasks/L05-lifter-x87-arithmetic.md#results).
- **Tested / compared with the original:** 21 Python regressions, seven
  Rust unit tests and the full 84,312-case replay over 28,244 functions pass.
  All 26,834 prepared functions remain; 1,346 newly liftable functions and
  64 dependencies are added. One final full build takes 9m19s, replay 29.83s,
  with two Cargo jobs and no concurrent vector run. This is sampled
  instruction comparison; external callees remain mocked where documented.
- **Evidence / recorder:** DDDA creates its D3D device with FPU_PRESERVE;
  incoming live-thread control words still need capture. Unicorn's actual
  reset differs from the assumed fixture default, so sparse fixtures now
  explicitly set CW 0x037F and empty tags. Affected vectors were re-recorded;
  stack faults are rejected and new cases compare full 80-bit st0 and CW.
- **Blockers / limits:** 104 new candidates lack usable fixtures; generic
  external mocks do not consume x87 arguments. Unmasked exceptions and
  environment instructions remain unsupported. Private L05 data and
  generated files are excluded from the source commit.
- **One next action:** create structured original-exe fixtures for the 104
  candidates in `E:/ddda-re/notes/native-L05/unverified-L05.json`.

## Batch 2026-10-11 (L06: native lifter crate split)

- **Implemented:** 16 fixed address shards depend on a small machine-state
  base crate. Cross-shard direct calls use an immutable resolver, separate
  from external-call mocks; the ddnative facade preserves existing APIs.
  Regeneration writes only changed files, and the inventory finds citations
  in shard sources. See [L06 results](tasks/L06-lifter-crate-split.md#results).
- **Tested / compared with the original:** the unchanged 28,244-function,
  84,312-case batch and seven Rust units pass after clean builds at two and
  four jobs. No functions or cases were dropped or re-recorded. Synthetic
  regressions cover cross-crate nesting, initialization, mock boundaries,
  missing callees, stable regeneration and inventory declarations.
- **Measured:** maximum observed rustc working set falls from L05's
  17.82 GiB to 1.59 GiB. Clean compilation takes 3m34s at two jobs or
  2m02s at four, versus the recorded monolithic 9m19s. Two jobs remain the
  gate default; builds and vector recording must not overlap.
- **Blockers / limits:** no split blocker. This preserves sampled fixture
  coverage, not gameplay verification; release builds and larger future
  batches were not benchmarked. Source-only crate skeletons require
  regeneration before building after merge.
- **One next action:** the head should regenerate the shards on main and
  run the full native gate with its complete private fixture collection.

## Batch 2026-10-11 (L08: native lifter integer/SSE coverage)

- **Implemented:** carry/borrow, multiply/divide widths, rotates and double
  shifts, bit operations, conditional moves, exchanges, sign extension,
  direction-aware MOVS/STOS and SSE/SSE2 arithmetic, conversions and lane
  operations. Defined flags are exact; observed undefined results reject.
  Same scan candidates: 33,669 -> 36,793 liftable, 4,059 -> 935 rejected.
  See [L08 results](tasks/L08-lifter-integer-sse.md#results).
- **Tested / compared with the original:** 31,580 functions / 94,088 cases
  pass private full replay, retaining all 28,665 prepared functions and
  adding 2,915. Also pass: 32 Python regressions (79 synthetic instruction
  programs), seven new hardware-comparison tests and seven existing Rust
  units. Compilation takes 3m25s and replay 44.21s at four jobs, without
  incremental compilation or concurrent recording; minimum free memory
  during the final build/replay is 17.97 GiB. These are sampled fixtures,
  with external-call mocks, not gameplay coverage.
- **Evidence / limits:** replay caught a Capstone COMISD/UCOMISD operand
  width error, now fixed and regression-tested. Of 3,124 raw additions,
  206 lack usable fixtures, two need unavailable x87 callee contracts and
  `0x00F276C0` is quarantined for a separately reproduced large-argument
  FSIN difference between Unicorn and CPU. Five new roots stay external
  as callees to preserve prepared callers' usable boundaries. Vector
  worker growth persists: recording was reduced from six to three workers
  proactively; free memory never crossed the user's 6 GiB threshold.
- **One next action:** the head should review/adopt private `native-L08`
  patterns/vectors and regenerate on main; the 209 remaining additions
  are listed in private `unverified-L08.json`. Source commits exclude
  generated shards and all private data; shared native data was untouched.

## Batch 2026-10-11 (R01: Phase 1 runtime design)

- **Designed, not implemented:** [RUNTIME.md](RUNTIME.md) proposes guest
  memory, ABI dispatch, threading/services, graphics/audio boundaries,
  existing-crate reuse and staged boot/title/load/co-op gates. No runtime
  code or new gameplay behavior was added.
- **Measured:** WinMain `0x004079E0` has 276 known direct-reachable entries,
  89 lifted; adding the main ctor and inspected vtable roots gives 1,591
  known, 991 lifted. The complete 43-entry non-platform-labelled direct
  gap list is in the plan. Static graphs do not measure dynamic frequency.
- **New blocker:** CRT initialization at `0x012EDABC..0x012EDAC9` invokes
  5,656 non-null targets, but 5,651 are absent from inventory. Examples
  include game DTI registration at `0x0136DDF0`; treating every high-address
  function as replaceable CRT would skip required game initialization.
- **Checked:** installed/analysis section bytes and 273 normal imports
  match; read-only measurement/probe scripts and report/document consistency
  checks pass. XAudio2 2.7 creation and thread/APC boundaries are addressed.
  No runtime build or original-game boot was performed. Ghidra was
  unavailable; executable evidence came from read-only disassembly.
- **One next action:** head reviews the design, then commissions the
  initializer and dynamic-target manifest before runtime implementation.

### L10: supplementary native entry points (2026-10-11)

- **Implemented:** deterministic installed-PE initializer/RTTI/direct-call
  discovery, private supplementary TSV and SQL evidence table, configurable
  inventory database, and incremental lifting that preserves existing
  fixtures. MT vtables require registration, getDTI and an object-vtable
  store; arbitrary executable-pointer runs are excluded. Ghidra stayed down.
- **Measured:** 6,094 missing entries: 5,656 C/C++ initializers and 438
  virtual targets; no additional direct-call targets even after expansion.
  6,072 lift and 6,061 pass original-instruction replay. `0x00533360`,
  `0x004FB960`, `0x004FBA60`, `0x00533840`, and initializer `0x0136DDE0`
  (the actual entry containing R01's `0x0136DDF0` site) now verify.
- **Verified:** private selection 31,580 -> 37,641 functions, 94,088 ->
  112,266 cases, with every prepared record retained. Seven new and 32
  existing Python regressions pass; the addition/dependency subset and one
  final full `cargo test -p ddnative` pass (32 Rust tests). Inventory now
  has 56,572 non-thunk entries, 37,651 verified (66.6%; 73.4% of summed
  size estimates). This is bounded per-function replay, not game boot.
- **Memory:** final build 3m34s plus 48.60s lifted replay at four jobs,
  incremental off, minimum observed free RAM 15.63 GiB. A full scan was
  proactively stopped at 11.45 GiB free; the incremental scan released
  bodies and used little memory. No free-memory reading fell below 6 GiB;
  no build/vector overlap, shared writes, gate invocation or push.
- **Open:** 22 lift rejections (21 indirect jumps, one interior branch),
  11 without fixtures, and RTTI tables lacking store corroboration remain
  outside the verified set. Twenty-two size hints and overlapping shared
  tails limit byte-percentage precision. See the L10 Results for private
  artifacts and the exact shared regeneration procedure.
- **One next action:** head reviews/adopts the supplementary inventory and
  matching verified pattern/vector pair before runtime boot-manifest work.

## Batch 2026-10-11 (L09: structured native fixtures)

- **Implemented:** versioned small-word, zero-object and mixed-object
  recorder inputs; bounded per-instruction wall-clock rejection; private
  missing-entry scan, per-seed diagnostics and resumable strategy trials.
  Existing memory, return, x87 and flag guards remain active. Each decoded
  body is released; the supplementary inventory is included.
- **Measured:** 5,244 missing functions still lift against the existing
  closed set. Random retry unlocks 1,317, zero inputs 2,435, small words 85,
  zero objects 13, mixed objects 21: **3,871 additional passing lifts**.
  The three new distributions contribute 119 beyond random/zero. The old
  selection and fixtures are preserved verbatim; no closed boundary changed.
- **Verified:** 41,512 lifted functions / 123,574 cases, up from
  37,641 / 112,266. Six additions overlap existing hand ports, giving
  **41,516 inventory-verified functions (73.4%) and 85.8% of summed code
  bytes**. All 47 Python regressions, the addition/dependency replay subset
  and the final full `cargo test -p ddnative` pass (32 Rust tests).
  `aGameMain::vf06` at `0x004FB3E0` now has sampled mock-boundary replay;
  this is not proof of a working frame or game boot.
- **Resources:** final full compilation 4m25s, lifted replay 55.64s,
  minimum observed free memory 14.94 GiB at four jobs, incremental off.
  Vector workers stayed at six, recycled every eight functions. No
  build/vector overlap, shared writes, gate invocation, Ghidra restart or
  push. Private accepted data and reports remain under `native-L09`.
- **Open:** 1,373 missing candidates still lack cases (1,118 primarily
  outside-image execution in the final strategy), 971 reject during lifting,
  and four known mismatches remain quarantined. `0x0041DD60` and initializer
  `0x0137E490` still lack fixtures. Historical no-vector cache counts are not
  an exhaustive current candidate census; see the L09 Results and reports.
- **One next action:** head reviews/adopts the verified pair, then traces
  representative remaining outside-image failures without relaxing guards.


## Batch 2026-10-11 (L11: indirect tails and recorder stack cleanup)

- **Implemented:** non-switch dword register/memory jumps in ext mode read
  their target through guest memory/registers, continue at known local
  instruction starts, and otherwise tail-call through `call_ext`. Closed
  decoding stays strict; ambiguous absolute switches remain rejected.
- **Traced and corrected:** mock argument counting included interleaved
  callee saves (`0x004065EA`, `0x0040D826`); linear stack-pop discovery read
  a neighbouring function after a tail jump (`0x00D009E6`); indirect caller
  cleanup could be applied twice (`0x0040846C`). Recorded pop values remain
  explicit mock ABI guesses; every existing validity guard remains active.
  New `trace_recorder.py` diagnoses unusable fixtures without a Rust build.
- **Measured:** missing-entry lift rejections **971 -> 595**; 376 new
  liftable bodies, 371 now verified. Recorder fixes/retries add another 898
  previously liftable functions. A controlled 60-function sample drops
  outside-image failures from 57 to 13; 53 original failures followed own
  returns and four mocked tail returns. Residual causes are not assumed.
- **Verified:** selection **41,512 -> 42,781**, cases **123,574 -> 127,303**,
  all baseline records unchanged. All 53 Python tests, the 46,098-case
  addition/dependency subset, and final full `cargo test -p ddnative` pass
  (32 Rust tests). `sBbsRpgMain::vf06` at `0x0041DD60` now has sampled
  original-instruction replay with mocked callees; no game-boot claim.
- **Inventory/resources:** 42,785 verified, **75.6%** of current 56,577
  non-thunk entries and **87.5%** of summed code-size estimates. Five added
  supplementary entries explain the denominator change since L09. Final
  compile 4m20s, replay 57.42s, total 324.06s; minimum free RAM 16.26 GiB.
  Four Cargo jobs, incremental off, six vector workers, no build/vector
  overlap or memory-threshold crossing. Shared data stays read-only.
- **Open:** 485 recorder failures (286 primarily outside-image in the last
  input pass), 595 lift rejections (333 unsupported switch remappings), and
  four unchanged quarantines. Initializer `0x0137E490` still lacks fixtures.
- **One next action:** head reviews/adopts the matching private `native-L11`
  pair and regenerates; then investigate switch remappings and residual
  mock-stack failures without relaxing guards. Source/tests/docs only;
  no runtime code, Ghidra restart, shared gate or push.

## Batch 2026-10-11 (L12: remaining lift rejections)

- **Implemented:** strict cross-register byte-remapped switch tables;
  delayed mocked x87 returns through supported x87 work/direct branches;
  INT3/UD2 terminal traps, LEAVE, PINSRW/PEXTRW and REP RET. Platform-dependent
  CPUID/RDTSC/PUSHFD reject explicitly. See L12 Results and the native pipeline.
- **Recorder evidence:** delayed local allocation at `0x008AA2D5` and
  `0x00782D15` was counted as outgoing arguments, then popped twice. A
  constrained matching-epilogue hint excludes it; this remains mock ABI
  inference. A same-seed retry adds 14 functions. No guard was relaxed.
- **Measured:** scan rejections **595 -> 86**, 509 newly liftable bodies;
  selected functions **42,781 -> 43,294**, cases **127,303 -> 128,772**.
  All baseline records remain unchanged. The 513 additions include 306
  switch bodies, 140 x87 cases, 20 trap-path functions, six singleton
  instruction cases and 41 previously liftable functions.
- **Inventory:** **43,298 verified, 76.5% of functions, 90.8% of code bytes**
  (14,081,075 summed bytes). Sixteen supplementary entries increase the
  denominator to 56,593 non-thunks. This is sampled instruction comparison
  with external mocks; no live-game or game-boot claim.
- **Validation:** 63 Python tests pass; the addition/dependency subset passes
  43,838 cases / 14,699 functions and seven Rust units. Final required-vector
  `cargo test -p ddnative` passes all 32 Rust tests, including all 128,772
  lifted cases. Compile 4m13s, replay 58.15s, total 318.06s; minimum free RAM
  14.29 GiB. Four Cargo jobs, incremental off, six vector workers, no overlap
  or memory-threshold crossing.
- **Open:** 497 liftable candidates still lack fixtures (287 primarily
  outside-image in the final strategy), 86 lift rejections and four unchanged
  quarantines. Initializer `0x0137E490` remains without a usable fixture.
- **One next action:** head reviews/adopts the matching private `native-L12`
  pair and regenerates, then traces residual recorder failures and resolves
  ambiguous indirect x87 flow without weakening guards. Shared data remains
  read-only; source/tests/docs only, no runtime code, gate, Ghidra restart or push.

## Batch 2026-10-11 (L13: exhaustive residual census)

- **Accounted:** all 832 initially non-platform/unverified entries, including
  244 thunks omitted from old scans. Four already verified hand ports were in
  the old no-fixture list; five explicit platform models had `translated`
  status from later citations. The scanner now includes unverified thunks
  and excludes verified entries. No shared inventory relabeling.
- **Closed:** all 244 thunk entries, three SFENCE bodies and 26 recorder
  failures acquire fixtures. SFENCE executes the host instruction; FLDL2E/
  FLDLN2 use saved x87 state and pass synthetic extended-value comparisons.
  Bounded switches now inform mock cleanup (`0x00E0BF48`), with consistent
  returns, overlap rejection and the existing 512-instruction limit.
- **Evidence:** a 24-function same-input trace sample improves from zero to
  two successes; other failures remain guarded. Host/Unicorn reciprocal
  probes differ on 128/128 vectors per instruction, so a CPU-specific oracle
  and replay contract remain open. FS exception-chain access needs guest
  thread/SEH state. Details and regeneration are in `docs/INVENTORY.md` and
  L13 Results; private JSON retains per-entry instruction/trial evidence.
- **Measured:** selected entries **43,294 -> 43,567**, cases **128,772 ->
  129,587**, baseline records unchanged. Conventional non-thunk verification
  **43,298 -> 43,327 (76.6%, 90.9% code bytes)**, plus 244 verified thunks.
  Denominator remains 56,593 non-thunks / 15,516,093 summed bytes.
- **Validated:** 66 Python tests; subset 43,184 cases / 14,459 functions and
  seven Rust units pass. Strict final census has **559** entries: 467 recorder
  failures, 83 lift rejections, four quarantines, five platform overlaps;
  zero unexplained/unrecorded/unadopted rows. Final required-vector
  `cargo test -p ddnative` passes 32 tests / 129,587 lifted cases. Compile
  4m23s, replay 59.86s, total 330.06s, minimum free RAM 12.73 GiB. Own heavy
  jobs serialized; external builds caused launch deferrals, with no 6 GiB
  threshold crossing. Generated files remain outside the source commit.
- **One next action:** head reviews/adopts the matching private `native-L13`
  pair and census, then chooses the reciprocal-oracle/CPU contract and traces
  remaining stack-context failures. Initializer `0x0137E490` remains without
  a usable fixture; no live-game/boot claim, shared writes, gate or push.

### L14 platform-inclusive lifting (2026-10-11)

- **Implemented:** both scan paths include historical platform classes.
  Inventory retains `platform_rule` independently of verification. Per-rule
  reports distinguish selected bodies, recorder failures, lift rejections and
  replay mismatches; the five explicit test models remain excluded.
- **Compared:** 12,602 additional entries / 37,640 cases. All DTI allocation
  wrappers, construct-if-memory wrappers and base destructor chains verify;
  deleting destructors reach 3,925/3,926. CRT `__snprintf_s` 0x012ED991 and
  `__ioinit` 0x012F7C13 gain fixtures. Historical classes total 12,638 verified
  of 12,804 entries including thunks. This is body/callee-event replay, not
  a closed CRT startup or live-game claim.
- **Measured:** selection 43,567 -> 56,169; cases 129,587 -> 167,227, all
  baseline records unchanged. Verified non-thunks 43,327 -> 55,884 (98.7%,
  95.7% of summed code bytes), plus 289 verified thunks. Denominator remains
  56,593 non-thunks / 15,516,093 bytes. Whole-inventory residual: 533 recorder
  failures, 161 lift rejections, ten quarantines and five models = 709 rows.
- **Validated:** 69 Python tests; subset 80,009 cases / 26,788 functions plus
  seven units; final required-vector `cargo test -p ddnative` passes all 32
  Rust tests and 167,227 lifted cases. Successful invocation compile 4m58s,
  replay 90.09s, total 398.08s, minimum free 9.70 GiB. External builds caused
  deferrals and two stopped L14 full-build attempts; only its own process
  trees were killed. Lowest observed free memory 8.29 GiB; never below 6 GiB.
  The completed invocation is not a clean-build timing benchmark.
- **Open:** six new x87 control/status oracle mismatches are quarantined with
  cases and diagnostics, preserving the four old quarantines. Startup
  0x012EF8A7 still lacks a fixture and depends on unsupported FS/SEH helpers.
  Destructor 0x00DB8CD0 exposes unresolved import-tail mock cleanup. Original
  bodies at the five model entries require a distinct recording/runtime
  binding contract; no expectations or status bits were normalized.
- **One next action:** head reviews/adopts the matching private native-L14
  pair and quarantine evidence, regenerates Rust and platform rule metadata,
  then rebuilds the shared inventory. Source/docs commits only; generated
  files and private data are not committed. See L14 Results for per-rule
  counts, evidence and exact regeneration steps. No shared writes or push.

### L15 runtime prerequisites — verified privately, ready for adoption

- **Implemented:** distinct original-entry recorder contracts at model
  addresses; per-Cpu FS base and Unicorn GDT/TEB setup; general mock cleanup
  fixes for startup calls and named import tails. Details and addresses are
  in `docs/tasks/L15-runtime-prereqs.md` and `docs/NATIVE_PIPELINE.md`.
- **Compared against the original:** 137 additions / 394 cases,
  including three allocator originals, memset, `__mtinit`, `__cinit`,
  twelve FS bodies and destructor 0x00DB8CD0. Baseline scan 533 candidates /
  161 rejections becomes 554 / 145 (now including memcpy's rejection).
- **Tested:** 65 native Python tests, nine inventory tests, subset replay,
  and all 32 Rust tests with required vectors pass. Full replay covers
  56,306 entries / 167,621 cases, preserving every old record/case. The
  private inventory reaches 56,021 verified non-thunks (99.0%, 96.1% of
  code bytes), plus 289 thunks. Full build/test took 415 s at four jobs,
  with 9.81 GiB minimum free RAM; no build/vector overlap in final testing.
- **Open:** memcpy 0x012EFEA0 needs a nonzero switch-index proof; SEH frame
  helpers need explicit caller-frame fixtures. In total, 417 entries still
  lack fixtures, 145 reject lifting, and ten stay quarantined. Image bytes
  were already real: startup gains come from evidenced mock ABI cleanup.
- **One next action:** head reviews/adopts the private native-L15 pair,
  regenerates/replays and rebuilds shared inventory, then sets runtime
  thread FS bases and binds the verified original entries. See L15 Results
  for exact addresses, evidence and regeneration steps. No shared writes,
  generated-code commits, native gate, runtime edits or push.


### L18: runtime movie boundary (2026-10-11)

- **Implemented:** WMVCore delay exports and guest reader/media/header/sample
  interfaces. Host Windows WMVCore decodes through a guest IStream callback
  bridge; no host filename or pointer is substituted for the guest stream.
- **Evidence:** `0x00F7B250` selects YV12 and 48 kHz PCM; `0x00F7C200`
  consumes timed samples; `0x00F7B530` splits Y,V,U planes. Complete census
  and test results: [L18 Results](tasks/L18-movies.md#results).
- **Tested:** 39 runtime tests pass; all six installed WMVs decode through
  guest-dispatched fixture streams, original delay thunks and the lifted
  format selector. Y-plane variation, timestamps, sizes, interleaved reads
  and four movies' 48 kHz / six-channel PCM checked. No live-original
  pixel/audio comparison or complete uMovie session yet.
- **Blockers:** original stream AddRef/Release/Seek at `0x00F7B650`,
  `0x00F7B660`, `0x00F7B7D0` are absent from the base resolver. Capture
  encoding (`0x00F95320`) is separate and remains explicitly unsupported.
- **One next action:** add the three stream bodies through the native
  workflow, then exercise original uMovie playback in a running session.

### L16: headless runtime input and audio services (2026-10-11)

- **Implemented:** 17 import bindings; DirectInput8A/system mouse/keyboard,
  XInput 1.3's four independent ports, USER32 key state/hooks/settings and host
  event/rumble channels. XAudio2 2.7 engine and voice ABI, null endpoint,
  descriptor/range/loop timing, EOS and flush completion, guest audio worker
  callbacks, effect parameter callbacks, and a bounded float-block output seam.
- **Evidence:** keyboard ctor `0x00DF43E0`, mouse `0x00E05120`, pad paths
  `0x00DF52B0..0x00DF9EE0`; XAudio engine `0x00DCE0E0`, AudioReverb 2.7
  CLSID `0x0142AC70` used at `0x00DCE600`, streaming-ring completion callback
  `0x00DCCD00`. The full API/COM census and SDK contracts are in
  [L16 Results](tasks/L16-input-audio-services.md#results).
- **Validation:** `cargo test -p ddruntime`: 36 passed (13 new + 23 existing).
  Private original-engine fixture also passes: lifted `0x00DCE0E0` creates the
  engine/mastering voice and reads the endpoint format through these services.
  Tests exercise import/COM stdcall
  cleanup and output layouts, device injection, worker callback identity,
  real-time PCM/ADPCM consumption, loops/flush/EOS, effect parameters and
  per-session isolation. Not compared against live game input/audio.
- **Blockers / limits:** output is deliberately silent; decoding, audible
  mixing/effects and physical host backends remain. No legacy joystick force
  feedback or general multi-thread keyboard-hook chains are advertised.
  Static caller roles do not establish original OS thread IDs. Callback
  `0x00DCCD00` is missing from the inventory/generated program (head notified);
  real game streaming requires that native entry.
- **One next action:** head integrates the services into the startup trace and
  captures the next dynamic input/audio boundary before wiring host devices
  or claiming audible parity.


## Batch 2026-10-12 (train 1: batched verification; title/gameplay/co-op maps)

- **Process change:** verification is batched. Agents run cheap checks and
  commit; the head merges about five ready branches into an integration
  branch and runs one gen + `native_gate.sh` + runtime tests
  (docs/tasks/AGENT-RULES.md, "Batched verification"). Train 1 landed on
  main 59f55a9:
  - L25: performance. GPU present 2.9-3.5 ms → 0.4-0.6 ms; per-draw
    backend cost 475 µs → 18 µs; vector replay 551 → 357 ms.
  - L29: every render-target format the renderer uses equals the driver.
  - L24: 29 readable math ports (`crates/mtmath`, `docs/MATH.md`).
  - L26: an offline Steam client (14 imports, 37 interface methods) and
    the DDDA.sav sandbox with read-only import.
  - L19: audible XAudio2 2.7 mixing, bit-exact against the real 2.7 on
    PCM, ADPCM, SRC and reverb; a cpal sink in ddplay.
  - L36: ddruntime builds without the lifted program
    (`--no-default-features`).
- **Earlier merges, 2026-10-11/12:**
  - L17 B+C: the SEH helpers and both worker loops (58,704 verified,
    99.0%, 96.4% of bytes).
  - L18: movies.
  - L20: D3D9 completeness.
  - L23: all 29,198 shaders equal the driver (28,488 bit-exact, 710 within
    332 ULP, 0 differing).
  - L30/L30b: the frame-rate audit (`docs/FRAME_RATE.md`; FRAME.md 1a: the
    clock, base 30, `MaxFPS` 30-999).
- **Maps:**
  - `docs/TITLE_PATH.md` (L35/L35b): WinMain to the title's first frame;
    3,276 of 3,594 path functions verified.
  - `docs/GAMEPLAY_PATH.md` (L38/L38b): title to gameplay, and the census
    of all 586 non-verified functions:
    - 410 are recorder failures (claude-3: mock ABI 190, NaN 81, runaway
      73, noreturn 15, other);
    - 176 are lifter rejections (claude-6).
  - `docs/COOP_ON_REBUILD.md` (L40): the single-player assumptions (one
    pointer, `sPlayerManager+0x99C`) and a seat-relative proposal.
- **Blockers:**
  - The boot stops in CRT start-up (`__setmbcp`), which is in L28 batch 1
    on train 2.
  - Then the title path's 80+ unverified functions (IME setup, singleton
    constructors, the sShader2 loader 0x00E0E470) and the IMM32 and D3DX
    services (L43, L37).
- **One next action:** run train 2 (L28 + L22 data adoption, L31, L33, L34,
  L37, the L25 prototypes, L32), then rerun the boot.

## Batch 2026-10-12 (train 2: 99.5% verified; build fixes)

- **Train 2 landed on main 1be373e7.** It contains:
  - L28 batches 1-2 (CRT start-up rows, claude-3) and L22 (lifter
    rejections: MXCSR, SSE oracle, x87 forms, budget and switch rules,
    RTTI-less vtables; claude-6). Their data was merged with
    `merge_lift_data.py` (59,502 entries / 177,102 cases). **Gate: 59,217
    of 59,500 verified (99.5%, 98.2% of code bytes).**
  - L21 (boot, TIB/TEB unification), L27 (SEH dispatch), L31 (MSAA), L32
    (network), L33 (readable mtcore), L34/L34b (options, window), L37
    (D3DX), L39 (readable frame systems), L43 (IMM32), L44 (deflake), and
    the L25 prototypes (call-target cache, inline memory fast path).
  - Suites: ddruntime (all but one), `--no-default-features`, the light
    crates and ddgpu all green; ddplay builds.
- **Build fixes** (cba09f1e):
  - The L25 fast-path accessors were `#[inline(always)]`. At opt-level 0
    that copied the page-table check into every lifted memory access: the
    shard rlibs grew 4x, to ~280 MB, and the runtime examples went past the
    2 GB image limit (LNK1248). They are plain `#[inline]` now (a shard is
    50 MB), with a rule in AGENT-RULES.
  - The shards' debug=1 overrides moved from the test profile to dev, so
    `cargo build` and `cargo test` share one build. After the gate, the
    ddruntime tests and ddplay took under a minute to build.
  - Measured on one 22 MB shard (`-Z time-passes`, one job):
    - debug info: full 84 s, line tables 76 s, none 71 s; typeck plus
      borrowck take ~37 s regardless;
    - opt-level 1: 492 s, of which crate-local ThinLTO is 339 s;
    - release at opt-level 3: 661 s; 528 s with LTO off; opt-level 2
      without LTO 549 s; opt-level 1 without LTO 408 s. Release settings
      stay as they are until a ddperf replay shows what each costs at run
      time;
    - peak RSS ~3 GB per shard compiler.
  - Third-party crates are optimised in dev/test builds (aa8e6a3e):
    d3d9wgsl's driver comparison went from 745 s to 52 s.
    heavy.py now gives a slot that runs alone with 20 GB free five cargo
    jobs, and runs every job in a Windows Job Object (orphaned rustc/cargo
    die with it; the idle watchdog uses the job's CPU accounting).
- **Found by the train:** `tests/options.rs` (L34, first run) stops at
  0x012EE130. That is a jump-table tail of the CRT memcpy variant
  0x012EDE40, past Ghidra's end of the function. The lifted body treats it
  as external, yet the function counts as verified because its vectors
  never reach it. The test is ignored with that reason; brief L45
  (claude-6) fixes the rule and the verification hole.
- **Train 3** (worktree `E:\ddda-rs-wt\train3`, assembled): L28-crt, L41
  (frame oracle), L25-results, L42 (parallel guest threads; conflicts with
  train 2 resolved by the head). Waiting for L27b (claude-6, after L45) and
  Codex's L28b recorder batches.
- **One next action:** claude-2 reruns the boot on main (`__fpmath`, the
  sine-table initializers and `__aulldvrm` are fixed by train 2's data).

## Batch 2026-10-12 (train 3: 99.6%; parallel guest threads; boot into WinMain)

- **Train 3 landed on main e3d0fdd6. Gate: 59,248 of 59,500 verified
  (99.6%, 99.0% of code bytes).** It contains:
  - L42: parallel guest threads, atomic page table, service lock;
  - L41: the whole-frame oracle;
  - L28-crt (claude-3's last code) and the L25 results;
  - Codex's L28b batch (data only; its recorder code rides train 4).
  Every suite is green: ddruntime (L42's parallel tests included), the
  light crates, ddgpu, and the ddplay build.
- **Per-address data merges.** `E:/ddda-re/tools/merge_lift_data3.py`
  merges a private set into the shared data per address, against the
  set's base (record and cases as one unit; a conflict stops the merge).
  Agents may now re-record existing functions (promotions).
- **Build:** aa8e6a3e's optimised third-party crates also caught the
  ddlift shards, because they are not workspace members. 12db9dcb pins
  them to opt-level 0. With that, train 3's cold gate build took ~8
  minutes, and the light suites 3 minutes instead of 17.
- **Boot (claude-2):** the real boot runs the whole CRT start-up
  (`__cinit`, all 5,662 initialisers) and enters WinMain. It stops at
  0x00DAFED0, an introsort partition with no fixture (Codex). claude-2's
  static look-ahead to the title is in the L21 Results:
  - 42 functions on direct-call routes;
  - 51 one hop away through vtables or pointers;
  - 45 two hops away.
  They are routed to Codex (no fixture) and claude-6 (not lifted).
- **L45 (claude-6), in progress:**
  - a general edge-bound rule for table jumps;
  - gen no longer counts a function as verified when it has an unbound
    direct target, an unproven table or a stale body (~51 functions,
    reasons in `patterns/verification_gaps.json`);
  - boot triage next.
- **One next action:** Codex's 0x00DAFED0 fixture, then claude-2 reruns
  the boot.

## Batch 2026-10-12 (train 4: honest verification; boot past sRender)

- **Train 4 landed on main 863f6817: 59,229 of 59,440 verified (99.6%).**
  It contains:
  - claude-6's L45 rules: tails past Ghidra's end, frame tails, and
    verification gaps, so 44 functions with unproven tables are no longer
    credited, and 60 non-code admissions left the denominator;
  - L27b (SEH faults inside lifted code);
  - the boot triage records;
  - claude-2's services (CreateProcessA, wsprintfA) and look-ahead tables;
  - Codex's recorder rules and batches. The boot functions 0x00DAFED0,
    0x00DA9EC0, 0x00419100 and the sBbsRpgMain-ctor route are recorded.
  `tests/options.rs` passes un-ignored.
- **Incident:** L27b emitted a per-instruction step function for every
  lifted function. The shards tripled (lift_12: 21 → 61 MB), one rustc
  reached 11 GB, and the machine ran out of RAM (Claude Code stopped the
  head's background jobs: Ghidra server, status server, memory guard).
  Fixes:
  - steps only with LIFT_INSTRUCTIONS=1 (24c4f5c4);
  - the executor runs whole-function bindings (b84b3e44, diagnosed by
    claude-2);
  - heavy.py caps each slot job at 16 GB of committed memory (a Job
    Object limit), so a runaway compiler fails instead of the machine;
  - L47 (claude-6) scopes steps to the 44 bodies that register EH4 scopes.
- **Boot (preview on Codex's data with L46):** the boot gets past:
  - sRender's set-up;
  - the archive-manager tables;
  - `_qsort` in the sShader2 constructor (L46: an undefined CF after DIV
    must stay undefined through INC).
- **One next action:** train 5 (L46 x87 state and frame-tail fix, L47
  scoped steps, Codex's next batches), then claude-2's next boot stop.

## Batch 2026-10-12 (train 5: precise SEH where needed; boot stops 10-12)

- **Train 5 landed on main 84cf1194: 59,231 of 59,440 verified (99.6%),**
  with 46 verification gaps not credited (44 unproven tables, plus two
  fragments that pop registers they never pushed). Every suite is green;
  `tests/options.rs` and the SEH tests are included.
- **L47 (claude-6):** instruction steps only for the 44 bodies that
  register EH4 scope tables, +0.12% code. A fault inside an EH4 body now
  resumes at its `__except` (a test runs the original
  `__IsNonwritableInCurrentImage`).
- **L46 (claude-6):**
  - fnsave/frstor measured on the FPU and modelled bit-exact;
  - three x87 helpers promoted closed, and 0x012F1C9D recorded;
  - jumps into inlined frame tails stay local (memcpy's `__VEC_memcpy`
    path; the train-4 overcount is gone);
  - INC/DEC keep an undefined CF undefined (boot stop 10, `_qsort`);
  - an int3 path counts as noreturn for callee pops (stop 12,
    0x00DB8B90).
- **Codex:** register-passed objects in the recorder (0x00478600).
- **One next action:** claude-2's boot run on main (past stop 12); L48
  (claude-6) re-credits the gaps (edge bounds through memory slots,
  byte-remap tables, per-slot cases for input-indexed switches).

## Batch 2026-10-12 (train 6: 99.7%)

- **Train 6 landed on main 50b358f8: 59,250 of 59,440 verified (99.7%).**
  Codex's recorder now has guarded-stack and register-object inputs (19
  functions, boot stop 13's 0x0075BAD0 among them). Every suite is green.
- Codex classified all 211 non-verified functions (L28 Results). Next is
  a constructed-object input strategy: run the verified constructor first,
  then the method. claude-6 is on L48 (re-credit the 46 gaps).
- **One next action:** claude-2's boot run past stop 13.

## Batch 2026-10-12 (trains 7-8: 59,282 verified)

- **Train 7 (2c93b524):** L48 (claude-6) re-credits 30 of the 46 gaps:
  - memory-slot bounds, byte-remap tables and register or constant
    bounds prove 14 tables;
  - 16 input-indexed switches are slot-swept: every discovered slot has a
    case, with the table's end evidenced (the `slot-sweep-v1` strategy).
  16 gaps remain.
- **Train 8 (5525e431):** Codex records boot stop 14 (0x00DD5ED0: a
  vtable the constructor writes itself, resolved statically, from
  claude-2's evidence) and 0x00F7DE40.
- **59,282 of 59,440 verified (99.7%).**
- Codex moved to a fresh session (the old one, ~7M tokens, kept tripping
  the provider's content filter on x86 trace work).
- **One next action:** claude-2's boot past stop 14. Meanwhile claude-6
  works on L49 (lifter-side residuals) and Codex on input recipes for the
  37 residuals.

## Batch 2026-10-12 (train 9: 99.8%)

- **Train 9 landed on main b88555c9: 59,292 of 59,440 verified (99.8%).**
  Codex now records a bounded memset as one lazy, exact byte range, so a
  1 MiB clear no longer hits the recorder's page limit. It also has
  argument inputs for virtual tail wrappers and keeps fixed local frames
  apart from mock arguments. Boot stop 15 (0x00DC7D00, a sound-object
  reset) is among its 10 functions.
- **One next action:** claude-2's boot past stop 15.

## Batch 2026-10-12 (train 10: boot stop 16; the boot runs on)

- **Train 10 landed on main 3f3ca505: 59,293 of 59,440 verified (99.8%).**
  Codex's recipe records the sound engine reset (boot stop 16,
  0x00DC8270, ~100K instructions; its budget comes from the loop proof).
- With it, `Session::start` no longer stopped within the runtime test:
  the boot now runs on past stop 16. `Session::start_through` ends after
  a named CRT step, and the start-up test now asserts all eight CRT steps.
- **One next action:** claude-2 runs the boot with a wall-clock limit and
  reports where it stops or what it loops on.

## Milestone 2026-10-12: the boot reaches the game's main loop

- On main 3f3ca505 (train 10), the real boot runs the CRT start-up and
  WinMain and enters the main loop (WinMain 0x004079E0 → loop 0x00DF0D20
  → `sBbsRpgMain::vf06` 0x0041DD60, the frame). It runs frame after frame,
  with no stop and no hang (claude-2, 1,500 s run, 73 host-stack samples
  via `research/runtime/stack_sampler.py`).
- The worker pools, the sound thread and the other guest threads sit in
  their own waits; no thread is blocked on a missing service.
- Most samples are in rendering. `d3d9draw::flush` re-reads dirty
  resource shadows through `GuestMemory::read_bytes`, byte by byte, in
  the debug build.
- **Next actions** (claude-2):
  - a page-wise `read_bytes`;
  - a release-build frame rate;
  - ddplay's window: what the game shows (logos, movies, title).

## Batch 2026-10-12 (trains 11-12: 59,331 verified)

- **Train 11 (a6fb8349):** Codex's division/tail and constructed-copy
  fixtures (10 functions).
- **Train 12:** claude-6's L49:
  - `caller-state-v1` records a helper from a verified caller's exact
    state at the call (the CRT x87 error handlers);
  - new table proofs: caller-checked bounds, compares on copies, je/jne
    edges, stack slots across pushes. 16 gaps down to 5.
  claude-2's runtime tooling also landed: page-wise `read_bytes`, a
  service trace (`DDRUNTIME_TRACE_SERVICES`), visible guest-thread stops,
  the host-stack sampler and capture statistics.
- **59,331 of 59,440 verified (99.8%, 99.4% of code bytes).**
- **Boot:** the main loop presents frames (~5.4/s in a debug build), but
  the screen is static and untextured. The archive loader thread stops at
  boot stop 17 (0x01146410 jumps into 0x01146420, a body Ghidra split
  off; 7 such live cases). claude-6 is fixing the rule (L50).
- **One next action:** L50's stop 17, then the loader reads all of
  `rom\bbs_rpg.arc` and the title should start drawing textured frames.

## Batch 2026-10-12 (train 13: 99.9%)

- **Train 13: 59,375 of 59,440 verified (99.9%, 99.6% of code bytes).**
  Codex's residual batches add 44 functions: tail-wrapper argument
  inputs, finite-trace budget proofs and sentinel inputs.
  `docs/RESIDUALS.md` lists every remaining function with its class,
  blocker and owner.
- **One next action:** claude-6's L50 (boot stop 17, the archive loader),
  then the title screen.

## Milestone 2026-10-12: the boot reaches the title stage (emulator fallback, L51)

- **L51 (head):** `ddruntime` feature `emu` (off by default; enabled at run
  time with `DDRUNTIME_EMU_FALLBACK=1`).
  - A call into .text with nothing bound runs the original machine code in
    Unicorn 2.0.1, on the guest's own pages mapped in place.
  - Calls to bound functions leave the emulator and run natively. Registers,
    EFLAGS, x87 (FNSAVE image), MXCSR, XMM and FS (a GDT descriptor at the
    TEB) cross both ways.
  - Instances are pooled per thread.
  - Every emulated entry is logged with its caller (`DDRUNTIME_EMU_LOG`).
  - It is diagnostic: shipped code stays lifted and verified.
- **First real run (main + L51, 15 min):** no stop. Only 5 untranslated
  entries were needed:
  - 0x01146420 (stop 17, L50);
  - 0x01325580, 0x01325830, 0x013271E0, 0x012F4E74 (indirect-call
    entries missing from the inventory).
  The service trace shows the title stage: `Title.arc`,
  `movie\title\title_A.wmv`, the title BGM (`Tittle_DDN.sngw`,
  `gamestart.sngw`), `root.arc`, the shader package and the system
  textures and fonts.
- **Release build (claude-2):** the main loop presents 60.5-60.9 frames/s,
  the game's own MaxFPS limiter. The cold release build took ~2.6 h.
- **Build:** L52 splits the lifted code into 32 shards (512 KiB address
  regions; one shard: 33 s and 1.5 GB peak instead of ~75 s and 3 GB).
  Release shards are built at opt-level 1.
- **Next actions:**
  - ddplay with the fallback, a screenshot of the title (claude-2);
  - the 5 entries translated (claude-6);
  - train 15 (L51, L52).

## Milestone 2026-10-12: the title screen

- ddplay (debug, `--features emu`; main + L21 + L51 + 24eaf3bb) reaches the
  title screen. claude-2's DPI-aware PrintWindow screenshots show:
  - the ESRB "MATURE 17+" rating screen (t=120 s);
  - the title movie with its music (the Dragon's Dogma wyrm logo, ©CAPCOM);
  - at t=900 s the title, "DRAGON'S DOGMA / DARK ARISEN", the red wyrm
    over the forest, "PRESS ANY BUTTON".
  It ran at 0.1-2 fps in the debug build with the service trace and
  capture on; the release build runs the main loop at 60 fps.
- Emulated on the way: stop 17 (L50 fixes it) and four entries that
  Ghidra never made functions for:
  - three are slots of a function-pointer table at 0x01467D38, which
    looks like libvorbis backends;
  - one is 0x012F4E74, reached from 0x013046B0.
  These go to claude-6 as L54.
- Rendering: DrawPrimitive no longer draws with the device's index buffer
  (24eaf3bb, claude-2), and indexed strips carry their index format on the
  pipeline (b392b767, head; equals the D3D9 driver).

## Batch 2026-10-12 (trains 14-18: 59,450 of 59,469 verified)

- Trains 14-18 adopted:
  - L50, functions Ghidra split in two (boot stop 17, the archive
    loader);
  - L52, 32 smaller shards (`shard = (addr >> 19) % 32`; release shards
    at opt-level 1);
  - L54, the libvorbis backend table and 0x012F4E74;
  - L57, a preemption point in `call_lifted`.
  Main 0cd11f67: 59,450 of 59,469 (99.97%).
- Runtime-only fixes merged directly, with ddruntime tests:
  - L57b: serial-mode preemption at every lifted function entry, through
    `GuestMemory::page_table`;
  - L57c: no preemption inside services (`ServiceScope`);
  - L59: ddgpu copies a texture that a draw both samples and renders
    into, as the D3D9 driver tolerates.
- The `play` cargo profile:
  - shards at opt-level 1, no LTO and no debug info, codegen-units 64;
  - one warm target, C:/ddda-targets/play;
  - runtime-only changes rebuild in 15-20 s.

## Milestone 2026-10-12: the main menu

- ddplay in the play build, parallel mode (`DDRUNTIME_PARALLEL=1`, the
  17:55 exe with L59), reaches the main menu at t of about 380 s:
  - the dragon-relief background and the ornamented banner;
  - "Main Menu" (highlighted), "Manual", "Exit Game";
  - the help box "Go to Main Menu.".
  It ran at 2-16 FPS (claude-2).
- Open: in parallel mode a worker thread sometimes stops with "RefCell
  already borrowed" while the game runs on (claude-2, L64).
- Frame rate is now the bottleneck for every test run, and the next
  focus:
  - **L61**, memcpy/memset services: the game's copies of video frames
    (about 1.4 MB per 720p frame) went through byte-wise models; they are
    now page-wise services. Serial title movie: about 1 to 4 FPS.
  - **L62**, the x87 state stays live in the host FPU. Each x87
    instruction had saved and restored the whole FPU state twice; the
    sound decoder thread spent most of its time there. The guest state now
    stays loaded between instructions and is parked at service calls.
  - **L63**, bulk copies in ReadFile, movie samples, audio mixing and
    surface copies, which had moved one byte per Mem call.
  - `scripts/sample_profile.py`, a sampling profiler for the running
    game: per thread, its leaf functions and the nearest lifted game
    function.

## Milestone 2026-10-12 (~19:50): into the game world

- `play-fast` ddplay (task/fps-int: L61-L70 and L62b; 13 of 32 shards at
  opt-level 1, the rest at opt-level 0), parallel mode, unattended with
  `DDPLAY_AUTOKEYS="85:enter,95:enter"`. The run goes from launch through:
  - the logos (~58 FPS);
  - the title movie (~24 FPS);
  - PRESS ANY BUTTON;
  - the Auto-Save notice (60 FPS);
  - the save check;
  - the main menu (~12 FPS);
  - Load Game, picked by the second Enter;
  - the loading screen;
  - **gameplay**, in about three minutes (the user watched it).
- Loading runs at 0.2-1 FPS. The profile (E:/ddda-re/notes/profile_loadgame.txt)
  shows:
  - the main thread is two-thirds inside the GPU driver's shader and
    pipeline compiler (L71, claude-2);
  - the two loader threads run shards 15 and 19, still at opt-level 0.
- Build: a full `play-fast` build takes ~6 min cold; a runtime-only relink
  takes ~20 s; the optimised `play` build takes ~25 min.

### L89 deferred backend (2026-10-13, final measurements pending)

- Implemented one owned, bounded FIFO and one backend worker, two outstanding
  Presents, read-result fences, shutdown drain and per-frame presentation TLS.
  `DDRUNTIME_SYNC_BACKEND=1` retains immediate execution.
- Runtime **134**, queue **5**, capture **5**, GPU **33** pass; **3** existing
  GPU tests ignored. Queued/synchronous frame pixels and worker sink match.
- Target-format matrix exceeded the 60-second bound; standalone **0a1776ab**
  shards its unchanged 200 cases. Final four shards and isolated timings wait
  for the head's CPU-quiet build measurement. Preliminary contended producer
  timings improve, but total frame time does not establish an FPS improvement.
- Next: finish the bounded gate/timing, then head-run gameplay A/B. No game or
  lifted-shard build by this lane. Details: `tasks/L89-backend-worker.md`.


### L98 clean resource flushes (2026-10-13)

- Draw reads use a 42-slot stack array and one batched objects access.
  Dirty resource/owner indexing skips clean mip walks, preserving upload
  order and surface aliases; opt-in debug audit checks skipped dirty flags.
- Audit-enabled light suite: **152 passed, 1 failed, 1 ignored**; the sole
  fullscreen-gamma Reset failure also reproduces on the untouched base.
  Two new alias/copy/relock/all-42-read/cube/volume regressions pass.
- Three paired debug draw samples: median **35.03 -> 28.51 us/draw**,
  allocations **12 -> 10/draw**. Synthetic service timing only, no game run.
- Next: head integrates with the relocated L90 snapshot and compares FPS;
  full-suite green also requires resolving the existing gamma/Reset failure.
  Details: `docs/tasks/L98-clean-flush.md`.

### L96 headless encoder-hang investigation (2026-10-13)

- Concurrent cache tests reproduced a driver deadlock: the WGL instance
  thread spins during shutdown while Vulkan device/pipeline creation waits.
  No encoder/readback loop was observed; L88/L95 batching remains intact.
- Headless initialization now selects primary backends by default, keeping
  `WGPU_BACKEND` overrides. Focused regression checks clear/copy submit
  limits, resource tracking and upload ordering.
- Light runtime suite passes; GPU acceptance gate is queued. No game/FPS
  comparison by this agent. Evidence: `docs/tasks/L96-encoder-hang.md`.
- Next: finish the serialized GPU gate and hand the minimal fix to head.

### L90 compact constants (2026-10-13)

- Implemented bytecode-bounded constant snapshots and matching WGSL uniform
  layouts, little-endian bulk float copying, and valid dynamic binding
  ranges across ring rollover. Relative addressing retains full banks.
- Fixture float bytes: **7,680 -> 2,912**. Light suite **151 pass / 1 fail /
  1 ignored**; failure is the baseline fullscreen-gamma Reset issue also
  reproduced by L98. Prefix/rebinding and byte-packing regressions pass.
- GPU compact-layout/ring-rollover regression: **4,096 draws, exact driver
  pixel match**. Full 150-case texture/state matrix and completed draw
  comparisons pass. Next: remaining serialized GPU gate/timings after
  L96's independent WGL teardown fix; snapshot groups stay separate.


### L90 dirty snapshot groups (2026-10-13)

- Implemented setter-driven immutable draw groups, shared texture/stream
  lists, stack draw/read arguments, and reused constant-setter staging.
  Resource flushing and backend worker ordering are unchanged.
- Runtime allocations **10 -> 3/draw**; paired median **39.19 -> 23.05 us**
  (shared-machine load). **152 runtime + 5 frame tests pass**, one baseline
  fullscreen-gamma failure and one existing ignored test remain. New
  binding-transition/UP/old-snapshot regression passes. GPU targets compile.
- Next: complete serialized GPU gate/timings after head's gameplay run;
  integrate snapshot module with L97 render sharing and L98 flush_reads.


### L96 headless encoder-hang investigation (2026-10-13)

- Concurrent cache tests reproduced a driver deadlock: the WGL instance
  thread spins during shutdown while Vulkan device/pipeline creation waits.
  No encoder/readback loop was observed; L88/L95 batching remains intact.
- Headless initialization now selects primary backends by default, keeping
  `WGPU_BACKEND` overrides. Focused regression checks clear/copy submit
  limits, resource tracking and upload ordering.
- Light runtime suite passes; GPU acceptance gate is queued. No game/FPS
  comparison by this agent. Evidence: `docs/tasks/L96-encoder-hang.md`.
- Next: finish the serialized GPU gate and hand the minimal fix to head.

### L127 asynchronous native comparison reduction (2026-10-13)

- GPU color/depth/stencil summaries retain pass and target identity; exact loaded-channel comparisons, differing pixels and maximum difference. Bounded async MAP_READ returns 16 bytes per target without a GPU wait.
- Four focused Vulkan tests and one CPU summary test pass. Scale fixture: 128 pending targets, 16,650,240 pixels, 2,048 readback bytes; 58.73 ms synthetic diagnostic (no gameplay/FPS claim).
- Next: integrate L119 compare sink and L126 pre-oracle shadow lifecycle, run light runtime suite, then head-run in-game comparison. Details: docs/tasks/L127-native-compare.md.



### L121 copy submission boundaries (2026-10-13)

- All StretchRect paths record into the open encoder; matching single-sample
  depth/stencil surfaces copy both aspects directly. MSAA sample selection,
  sRGB resolves and blit filtering remain unchanged; upload hazards tracked.
- Vulkan: **35 distinct GPU tests pass** (38 invocations with all four target
  shards), three existing exploratory tests ignored; **9 CPU units pass**.
  Final expanded copy regressions pass; runtime light **171 pass / 3 ignored**.
- Paired synthetic eight-copy frames: scaled/conversion/depth **9 -> 1**
  submits, resolves **17 -> 1**. Median CPU us/copy: **138.2 -> 17.6** scaled,
  **146.0 -> 26.8** conversion, **990.5 -> 2.7** D24S8, **186.2 -> 12.1** resolve.
- No game or shard run. Next: head integrates and compares gameplay image,
  submit count and copy/FPS profile. Details: `docs/tasks/L121-copy-no-flush.md`.
### L118 step 3: clean draw and COM dispatch (2026-10-13)

- Clean resource index skips read-list population; dirty reads share the device
  borrow. Clean snapshots reuse cached groups without a restore lookup. Dense
  COM lookup preserves rebinding and step precedence; L101 runtime/lock changes
  remain intact, with new nested-switch and unwind lifetime coverage.
- On combo9/L101, three alternating debug pairs: clean median **12.29 -> 11.00
  us**, changed sampler **24.00 -> 20.39 us**; allocations stay **3 / 6**.
  Wide overlapping ranges and mixed clean pairs: no gameplay/FPS claim.
- Light suite **177 pass / one inherited gamma Reset failure / three ignored**;
  all 48 active unit tests pass. Six draw/resource audit tests and ten command/
  frame tests pass. No GPU, game or shard runs.
- Next: head combo9 gameplay/image comparison and render-thread profile after
  integration. Details: `docs/tasks/L118-d3d9-render-thread.md`.
### L125 apply ABI ports, first batch (2026-10-13)

- Targets, fixed-state and stream wrappers match **384 original Unicorn
  cases** and isolated lifted-body replay, including ordered calls, all
  GPR/XMM state, x87 control/status/depth and written bytes at call/return.
- Pure binding decoders agreed with L119; three wrappers enter ddports
  through mtdraw::abi::PORTS. No shard or game build/run, no FPS claim.
- Next: complete shader/constants wrapper, expand coverage and measure.
  Details: docs/tasks/L125-apply-ports.md.

### L125 four apply wrappers and native decoders (2026-10-13)

- DA6FE0/DAA5F0/DAA7C0/DAA8B0 registered through mtdraw/ddports;
  **1,568 Unicorn ABI cases / 15,892 calls** match readable and extracted
  lifted bodies, with host x87 status oracle. All feasible branch edges
  covered. Pure allocation-free decoders checked against **811** L117 cases;
  L117's **3,584 cases** still pass. Details/contract: L125 brief.
- Synthetic opt1/opt3 transition medians (us): targets **0.651/0.167**,
  fixed **0.263/0.158**, streams **0.472/0.243**, shaders **1.395/0.412**.
  The 96-call shader workload has mixed regressions: no reliable gain there.
- Light runtime **171 pass / one existing gamma Reset failure / three
  ignored**; ddports test passes. No game, GPU, shard build or FPS claim.
- Next: head's matched boot/save/frame/profile comparison and L119 adoption
  of the pure shader/constant source iterators.
### L127 asynchronous native comparison reduction (2026-10-13)

### L127 integrated comparison gate (2026-10-13)


### L127 compare producer (2026-10-13)

- DDDRAW_NATIVE=compare now emits pre-oracle begin and single-pass end markers from the L119 sink. Real targets receive D3D9 exactly once; rejected comparisons never redraw them. Attachment/fallback/frame boundaries and dirty uploads close pending comparisons.
- Three pure transport tests pass, including DeferredBackend FIFO and rejection recovery. Earlier light runtime:173 pass,1 inherited fullscreen-gamma failure,3 ignored. Backend-path oracle shares L119 decoder, separately checked against runtime D3D9 calls.
- Next: validate synthetic NativeJobs after L126 shadow hooks land, then head's in-game run. No gameplay equivalence claim.

### L127 integrated comparison gate (2026-10-13)

- DDDRAW_NATIVE=compare producer, L126 shadow worker and asynchronous GPU reduction integrated with L119 cache-2c. Five end-to-end GPU tests pass (nine GPU cases total), one summary CPU test, three producer tests and eight transport tests pass.
- 42 matched and three intentional mismatched target summaries; five explicit rejection diagnostics. Blended pre-pass loads, repeated shadows, mip attachments and D24S8 isolation checked; real targets unchanged, zero pending comparisons on test shutdown.
- Runtime: 180 distinct passes across full suite plus isolated timing retry; one inherited fullscreen-gamma failure, three ignored. No checks weakened; no game/shard run or game image/FPS claim.
- Next: head-run DDDRAW_NATIVE=compare gameplay and inspect mismatch/unverified/pending counts. Details: docs/tasks/L127-native-compare.md.


### L143 backend worker CPU (2026-10-13)

- Typed FIFO commands remove per-draw closure allocation; bulk constant
  packing, recycled scratch, generation-stamped resource bits and collision-safe
  derived-state memo retain the existing command/binding values and ordering.
- Release synthetic median backend CPU **2.97 -> 1.97 us/draw**; direct
  allocations **12.910 -> 0.162/draw**; deferred frame incl GPU wait
  **5.30 -> 3.82 ms**. Queue transport **4,200,003 -> 2** allocations over
  4.2 million draws. Shared-host fixture, no gameplay FPS claim.
- **13 command + 24 CPU + 29 distinct GPU tests pass**, including all
  150 render-state and 200 target-format cases. Light runtime **199 distinct
  pass / inherited gamma failure / 5 ignored** after two isolated timing
  retries. Additional cross-path GPU checks compile but await head's pause.
- `_NLG_Return2` is not proven to be unwind cost: no per-command catch exists,
  and the old profiler discards symbol displacement. Next: finish paused
  checks; head integrates and measures gameplay plus raw-PC attribution.
  Details: `docs/tasks/L143-backend-thread.md`.
