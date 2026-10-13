# L135: runtime floating-point boundaries

Assigned to codex-11 by head/user, 2026-10-13. Branch
`task/L135-x87-switch`, base `0d5ccfb82b88aa1a1ea5fdd13430d0f04e0ba7d1`.
Scope: sampled x87/SSE attribution, audited runtime boundary reductions,
light exactness tests; no shards, game runs, or GPU work.

## Evidence

Input: private `E:/ddda-re/notes/runs/profile_c17draw.json`, SHA-256
`8b395ae3e6c1a56dc0e021fb98be6cdc0a5c9a2957ea2d3504a82db8be2ec98a`.
1,756 rounds / 60.024 seconds. Reproduce with
`python research/native/profile_x87.py <profile.json>`.

Inclusive **sample counts**, divided by non-wait samples as in L131:

| Guest thread | Busy samples | park | acquire | image | guest FRSTOR | guest FNSAVE | emulator FRSTOR | LDMXCSR | STMXCSR |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| main | 1064 | 8 | 1 | 0 | 3 | 0 | 1 | 0 | 2 |
| 1034 render | 1009 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 19 |
| 1038 | 356 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 1070 | 346 | 2 | 1 | 0 | 2 | 0 | 1 | 1 | 1 |
| 1074 | 973 | 4 | 4 | 3 | 4 | 2 | 3 | 1 | 3 |
| 1078 | 182 | 0 | 4 | 0 | 0 | 0 | 0 | 0 | 0 |
| 107c | 663 | 5 | 5 | 0 | 1 | 0 | 0 | 1 | 1 |
| 1080 | 456 | 0 | 1 | 0 | 0 | 0 | 1 | 0 | 1 |
| 1084 | 658 | 0 | 3 | 0 | 1 | 0 | 1 | 0 | 2 |

Other threads have no named switching samples. Categories overlap; do not
sum them as exclusive time. Inline FNSAVE/FRSTOR in park/acquire/image cannot
be timed separately from their containing symbol. Likewise, inline LIVE
checks are inside x87 memory/register helpers, not separate symbols.
Thread 1038 has 47 samples in hot-shard FLD memory, 29 in shard09 FSTP
memory, and 22 in hot-shard FMUL memory helpers: these include useful guest
work and cannot all be credited to switching. The earlier 20% claim is not
re-established by this profile.

Main park: 0.75% of busy samples (0.234 CPU seconds if scaled to its measured
31.062 CPU seconds); render STMXCSR: 1.88% (0.540 of 28.672 CPU seconds).
The script prints these **estimates**, not instruction timings. Sparse
sampling and asynchronous waits make the scaling especially weak for 1078
(182 busy samples despite 21.828 measured CPU seconds).

All 21 park samples are under `Registry::run_service_as`. The two render
samples have nearest guest `00dac090`. Main callers include `00dbe560`,
`00dbd690`, `00fee320`, `00ff1920`, `0076d730`, `010fe5e0`, `00fefb90`,
and `00d0fff0`. The service function has not yet been entered at park, so
its target/name is absent from the stack. Exact API attribution or calls
per frame cannot be reconstructed from this JSON (no call/frame events).

Read-only Ghidra follow-up identifies direct API candidates: `00dbd690`
calls GetCurrentThreadId (the only API in that body); `00dac090` calls
WaitForSingleObject; `00dbe560` calls WaitForMultipleObjects. `00dbdc90`
mixes WaitForSingleObject and InterlockedIncrement, so the two samples
under that caller cannot distinguish those services. `00fee320` and
`00ff1920` contain memcpy calls and allocation helpers. These are static
call-path observations, not dynamic service-call frequencies.

All 12 guest FRSTOR samples, both guest FNSAVE samples, and all three image
samples are under `emu::run`; image is used by CPU cloning there. These
overlap L130's emulator-removal work. The seven emulator FRSTOR samples
are inside Unicorn/JIT stacks. Acquires resume ordinary x87 helpers in
many guest functions, including `00d13ab0`, `00ff1920`, `00fee320`,
`01067790`, `00f1f2b0`, and `00ffc9f0`.

## Implementation and audit

`Binding::Integer` is an explicit opt-in for eight existing KERNEL32
services: EncodePointer, DecodePointer, GetCurrentThreadId, and the five
Interlocked operations. Audit source: `services::encode_pointer`,
`threads::get_current_thread_id`, and `win32::interlocked_*`. These bodies
use guest integer memory, TLS integer identity, and SeqCst atomics, with
ordinary integer memory fallback for unaligned operands. No callbacks,
timers, float conversion, host driver calls, or waits in the bodies.

The existing lock-free service entry/scope/time-slice behavior remains.
With diagnostics off on Windows x86-64, guest x87 and MXCSR remain live;
one STMXCSR updates the Cpu mirror. Relative to a full service boundary
with live x87 followed by another guest x87 instruction: removes one
FNSAVE, one FRSTOR, park/acquire bookkeeping, one STMXCSR, and up to two
LDMXCSR instructions per call. Tracing or lock statistics takes the full
boundary; other platforms also retain that boundary.

The step executor now honors Integer and the existing DeviceState policy,
instead of unconditionally routing DeviceState through a parking boundary.
No production ddnative-base/host edits and no generated-code changes.

Both existing dispatch boundaries already condition MXCSR reloads on the
actual live value. `call_guest`, session entry/exit, and serial preemption
still have unconditional reloads; their distinct callback/scheduling
contracts were not changed in this batch. The mtdraw sink itself has no
park/MXCSR wrapper to remove. No FXSAVE substitution: its abbreviated tag
word and combined SSE state need separate exactness work. Allocating
memcpy/memset and general thread/sync services remain on the full boundary;
their wider allocator, diagnostic, callback and wait paths were not admitted
by the narrow integer-body audit.

## Results

- Debug FP regressions: 1,152 combinations (8 imports, direct/stepped,
  9 x87 depths, 4 rounding modes, aligned/unaligned). Complete normalized
  108-byte x87 image and live ownership unchanged, MXCSR sticky flags,
  FTZ/DAZ and rounding preserved, service return values checked.
- Ordinary-service isolation, diagnostic fallback/cache (both tracing and
  lock statistics), and stepped DeviceState isolation pass. All **four**
  FP tests pass in debug and optimized `play` profiles; benchmark ignored
  during ordinary verification. Parser synthetic wait/dedup/owner checks pass.
- Final runtime light serial suite: **192 passed, one inherited fullscreen-gamma
  failure, four ignored**. Failure is
  `a_gamma_ramp_is_shown_on_a_full_screen_swap_chain_only`; no checks weakened.
- Base/host suite: **28 passed**, including all four L62 live-state tests.
  Five `ddnative/tests/units.rs::x87_exact` tests pass via standalone rustc
  against these same base/host rlibs; only `ddnative::` reexport paths changed
  to `ddnative_base::` in a temporary test file. No shard build. Full ddnative
  units/vector native gate remains the next combined integration gate.
- Build settings: `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`,
  `CARGO_TARGET_DIR=C:/ddda-targets/L135`.

### Paired microbenchmark

`cargo test -p ddruntime --no-default-features --profile play --test
x87_services -- --ignored --nocapture --test-threads=1` runs the actual
InterlockedIncrement body via Registry, with FLD1 before and FSTP afterward.
One process pinned to CPU 2; 11 measured pairs alternating AB/BA after
one warmup pair, 20,000 boundaries per sample. Guest MXCSR has the precision
sticky flag (`0x1fa0`); x87 is live at each service. Both modes execute the
same integer memory side effects (480,000 increments per run). Three final
repeats on the shared machine, no quiet-machine claim:

| Repeat | baseline min / median ns | Integer min / median ns | median paired ratio |
|---|---:|---:|---:|
| 1 | 194.87 / 245.34 | 75.47 / 81.67 | 0.3977 |
| 2 | 194.75 / 209.84 | 73.33 / 77.19 | 0.3871 |
| 3 | 195.69 / 205.41 | 75.22 / 76.53 | 0.3866 |

Median of repeat medians: **209.84 -> 77.19 ns/boundary**; median paired
ratio **0.3871**. This measures the removed live-state boundary, not every
service configuration or whole-game FPS. The preceding exploratory repeat
was 278.11 -> 114.80 ns (paired ratio 0.4157), showing shared-machine noise.

Head request posted for matched gameplay FPS/image comparison and a separate
`DDRUNTIME_LOCK_STATS=1 DDRUNTIME_LOCK_STATS_EVERY=10` window with Present
counts. Existing statistics supply per-thread service counts and render
per-frame counts; they deliberately use full boundaries and must not be
used as optimized FPS measurements. Actual non-empty parks/frame still
need explicit live-state counters if required. No measured gameplay gain.
Next action: combined native gate, then head's diagnostic/FPS runs.
