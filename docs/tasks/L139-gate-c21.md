# L139: combo21 native gate and memory helper cost

Owner: codex-7. Branch: `task/L139-gate-c21`.

## Brief

Gate FPS line `335318fc` with the expanded native gate and next-batch
`f4bfb866` x87 discard, `4d53d724` FEDC00 and `546a052d` FED200 candidates.
Use the head's c21 profile to select helper/codegen work that also benefits
opt-level-0 generated callers. Never run the game. All shard builds use
`heavy.py`, `HEAVY_BIG=1`, four jobs, dedicated `C:/ddda-targets/L139`.

## Results

### Candidate integration

- Base: `335318fc`. Merged `task/L134-combined-gate`, retaining the expanded
  gate and x87 candidate. No earlier discarded helper experiment restored.
- FEDC00 depends on the FEDB80 test/recorder additions in `382e5f35`, absent
  from the base; dependency reported to head and included separately.
- Original candidate commits `382e5f35`, `4d53d724`, `546a052d` map to
  `9093d1ba`, `f6dacdcf`, `981fb77a`. Combined production pin: `981fb77a`.
- **GREEN** at 07:32:12: expanded play-profile gate, **100 passed, zero
  failed, seven timing benchmarks ignored**. All 33 generated crates rebuilt.
  Inventory passes: 59,450 / 59,469 non-platform functions; existing five
  coverage gaps and 17 owner-credited fragments remain.
- Per candidate: `f4bfb866` x87 discard **GREEN**; prerequisite `382e5f35`
  FEDB80 **GREEN**; `4d53d724` FEDC00 **GREEN**; `546a052d` FED200 **GREEN**.
  This is correctness acceptance, not measured gameplay performance.
- Required vector switches: `DDNATIVE_REQUIRE_VECTORS=1`,
  `MTDRAW_REQUIRE_APPLY=1`, `MTSTATE_REQUIRE_VECTORS=1`.
  Logs: `C:/ddda-targets/L139/gate.{out,err}.log`; start 07:22:02,
  end 07:32:12. Shared-machine elapsed time is not a build-speed comparison.

### Profile evidence and experiment selection

Head's private `profile_c21draw.txt` (SHA-256
`02732e5af0784063d474e6103daab2f409355baf1fb61fb748297747f4cb4940`),
30.1 seconds, 319 rounds: achieved
11 rounds/second, despite requested 40 Hz. Main thread has 180 busy
samples; `fast_u32` is 23.9% (43 samples), `fast_set_u32` 8.3% (15).
Job threads 0x1070 and 0x1074 show respectively 20.6%/8.4% and 24.8%/8.3%
for those same two helpers. These are sampled self costs, not call counts.
The profile's nearest-lifted grouping reports no frame because its symbol
recognition misses the mangled names; it is not evidence of no lifted work.

Flags and external dispatch are smaller secondary candidates. Main CF/ZF
self shares are 2.2%/1.7%; call_ext itself is 1.7%, with additional TLS
frames listed separately. Inclusive call_ext is not useful as a leaf cost.
The highest CPU thread remains wgpu-finish (74% CPU); head owns game/FPS
comparison, other agents own renderer work.

Selected experiment: compile existing SSE multiword memory helpers once
in the optimized base crate and inline their private dword implementation
within that crate. Preserve each permission/table lookup, atomic operation,
fallback call, address wrap and access order. No page-check coalescing or
cross-access pointer cache. Public scalar helpers remain out of line;
no `inline(always)` additions in generated-call interfaces.

Private experiments and opt0/opt1 caller drivers are under
`C:/ddda-targets/L139/emit-experiment`. The retained result follows.

### Narrow store128 follow-up: GREEN

The broad load64/load128/store64/store128 experiment is not retained:
load64 regressed across the first opt1 pairs, and other cases were mixed.
The narrow patch only changes `emit::store128` to an out-of-line base-crate
helper and shares its existing scalar write implementation through a
crate-private inline method. No access checks or stores are combined.
No generated source or lifter emission changes are needed. The current
generated tree contains 2,378 `emit::store128` call sites in 508 files;
these are static counts, not measured execution frequencies.

Initial separate-process timing favored store128 at opt0, but unchanged
load128 controls varied substantially. Those timings are exploratory,
not acceptance evidence. A longer same-process run timed out during a
machine slowdown and is discarded. The reproducible benchmark driver uses
shorter bounded runs, with alternating reference/new paths in one process,
caller opt0/opt1, base opt3, and explicit logical-CPU affinity.

The 2,304 ordered-access comparisons include 576 store128 cases and controls
for the other unchanged multiword helpers. They cover wrapping addresses,
page crossings, backing/permissions, atomic/serial modes, trait fallback,
and fallback side effects that change following accesses. An additional
fault test checks that the first two stores survive a third-lane fault
and the fourth store never occurs.

- Base debug: **36 passed** (including both new tests).
- Runtime light, serial, no-fail-fast: **198 passed / one inherited
  fullscreen-gamma failure / four ignored**. Nested subprocess summaries
  are excluded from these top-level counts. No checks were weakened.
- Expanded required-vector/play gate: **102 passed / zero failed / seven
  timing benchmarks ignored**, all 33 generated crates rebuilt. Inventory
  remains 59,450 / 59,469 with five existing gaps. Completed 08:04:14.
- Frozen production/test sources match `gate2-source.json`; logs are
  `base-debug.log`, `runtime.log`, `gate2.{out,err}.log`, and
  `validate.out.log` under `C:/ddda-targets/L139`. Parent is `15c75516`.
- Private compiled-code inspection confirms four distinct page-entry and
  permission checks, in-order scalar stores, and no public accessor calls
  on the successful fast path. No raw disassembly is committed.

### Retained synthetic measurement

Final same-process reference-loop/helper pairs use logical CPU 10,
200,000 stores per sample, 10,000 warmup stores, 11 alternating AB/BA pairs,
caller opt0 or opt1 and base opt3. `QueryThreadCycleTime` reports cycles
spent by the thread, excluding unscheduled time; wall-time outliers remain
in the logs. The reference loop is compiled at the selected caller level.
Both paths include an indirect benchmark call and loop overhead. The
working set is synthetic, with atomic guest accesses enabled.

| Caller | Access | Reference cycles/store | Helper cycles/store | Median paired ratio |
|---|---|---:|---:|---:|
| opt0 | aligned | 153.486 | 61.833 | 0.4095 |
| opt0 | unaligned | 156.404 | 65.226 | 0.4149 |
| opt0 | page split | 155.550 | 61.170 | 0.4028 |
| opt1 | aligned | 29.657 | 15.197 | 0.5527 |
| opt1 | unaligned | 28.625 | 22.476 | 0.7868 |
| opt1 | page split | 26.991 | 15.741 | 0.5818 |

All 33 opt0 pairs improved; paired ratios range 0.3522–0.5246. Opt1
aligned improves 11/11; unaligned 10/11, page split 9/11, with maxima
1.0190/1.1091 for the latter two. This is a synthetic loop/helper
comparison; gameplay FPS, loading and image comparison remain for head.

Reproduce outside a game run, through `heavy.py` (no BIG/shards needed):
set the usual dedicated target/incremental/build-jobs environment, then
`python research/native/bench_emit_memory.py --caller-opt 0 --iterations
200000 --cpu 10`, and repeat with `--caller-opt 1`. The final cycle-enabled
benchmark was compiled and run in both caller modes from private source
before copying it here; production/test source stayed frozen through gate2.
Final logs: `cycles.{out,err}.log`, `emit-experiment/cycles-o{0,1}.txt`.

L142 owns broader memory/flat-view work. Board #12051/#12058/#12064 agree
that this narrow private-method split is disjoint; mem.rs/lift.py claims
were released to that owner. No L142 implementation is in this branch.

Next: head's same-scene image/FPS/loading run with this helper; combine
with L142 only under a separately pinned gate.
