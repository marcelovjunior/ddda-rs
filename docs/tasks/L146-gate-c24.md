# L146: combo24 gates and x87/flag code generation

Assigned to codex-7, branch `task/L146-gate-c24`, based on combo23.
Only head runs the game. Shard generation/builds use `heavy.py`,
`HEAVY_BIG=1`, a dedicated target and four build jobs.

## Scope

1. Gate store128 `7eddfd1d`, L130 `a2d0407e` and `0ce6de79`, L143
   `e04b76ce`, plus L141/L144 commits ready at the starting cutoff.
2. Separately gate flat memory `49764236` with `LIFT_FLAT_MEMORY=1`
   and `ddnative-base/flat-memory`; exactness, not gameplay performance.
3. Measure and reduce x87 memory/register and remaining flag-helper cost
   at caller opt0 and opt1. Preserve x87 state, polling, memory ordering,
   permissions, faults and instruction boundaries. Native replay required.

## Results

### Combo24 frozen input

Production pin `006dcd96`. L141 had two ready commits, `784f5a9c` and
`83f9ad22`; both are included. L144 had no new candidate at the cutoff.
L138 is not included, so its Vulkan key integration is not applicable.
The L141/L113 release conflict retains `small_buffers.remove` and
`quads.forget` before L141's held-resource reset. L143's new module and
L113's compute module are both retained.

Expanded required-vector play gate: **104 passed / zero failed / nine
benchmarks ignored**, completed 08:48:49 local. Inventory is 59,450 /
59,469, with the same five existing verification gaps. Thus `7eddfd1d`,
`a2d0407e` and `0ce6de79` are **GREEN** on this pin.

Combined renderer: **38 distinct GPU tests pass / three ignored** across
backend-worker, native-job, cache, draw, target and parallel-encode suites.
All 150 render-state and 200 target-format cases run across four shards.
Each GPU process checks the head's pause and has a hard 60-second timeout.
The 120-frame long-lived cache test initially timed out; its isolated
retry passed in **53.12 s**. No assertion failures or device skips.
Renderer CPU: **37 passed** (13 d3d9cmd, 24 ddgpu). Runtime light,
`--no-default-features --no-fail-fast -- --test-threads=1`: **202 top-level
passed / one inherited fullscreen-gamma failure / five ignored**. Two
nested x87-service subprocess summaries are excluded from the total.
The native-sink oracle and unit tests pass. Thus L143 `e04b76ce` and L141
`784f5a9c` / `83f9ad22` are also **GREEN** for the tested contracts; the
entire light runtime suite is not an unqualified pass.

Generation: **808 s**, 59,734 functions, 1,466 modules, 33 crates, 44 scoped
step functions. All 33 shard libraries rebuilt; fingerprint-start to final
rlib wall span **225.51 s**, aggregate rlibs **434.81 MiB**. This span includes
scheduling and is not summed CPU time. Generated source receipt:
`77e6cc03d4b7334a53aa510c484855aef3af988eed9d7dc6799eb27865f22dfe`.
Private logs, SHA receipts and scripts are archived under
`E:/ddda-re/notes/L146`; the task target is removed after completion per
head's disk cleanup request (board #12750).

### Profile and experiment

`profile_c23def.txt`: guest thread 0x1038 is busy for 115/347 sampled
stacks (31% CPU). FLD m32 and FSTP m32 generic helper self shares are
16.5% and 10.4% of busy samples. Page-table polling is 15.7%, scalar
load 11.3%, scalar store 3.5%. These are sampled attribution, not an
instruction census or whole-game performance prediction.

Typed real-operand helpers preserve per-instruction polling and ordered
dword accesses. Pair fusion is not justified by these measurements.

### Separate flat gate: GREEN

L142 source pin `49764236`, feature `ddnative-base/flat-memory`, generated
with `LIFT_FLAT_MEMORY=1`. The producer confirms its generated files are
frozen (board #12459); gate3 adopted all 1,572 Rust/manifest files with
every file verified before and after copying against
`C:/ddda-targets/L142o1/generated-receipt.json`, then compiled and replayed
independently. This avoids repeating identical gen.

Combined source pin `dd1c0002`: **106 passed / zero failed / nine ignored**.
All 33 feature shards rebuilt in an artifact wall span of **235.37 s**,
aggregate rlibs **454.94 MiB**. The separate runtime flat-memory suite
with `DDRUNTIME_FLAT_MEMORY=1` passes **five tests**. Watched replay memory
uses the trait fallback: this verifies the flat lowering's fallback, not
every direct runtime flat access. Feature/base and runtime tests cover
the direct memory implementation. Existing five inventory gaps remain.
Generated receipt:
`0a519745df752d6b51ae7322e5c057ab893a6184371fde6b12677a9ef35ad64b`.

Head's subsequent game A/B (board #12797) did **not accept flat memory for
performance**: 20.3 FPS average versus its combo23 base's 21.5, with thread
CPU within noise and a higher cold compile cost. That is head's gameplay
result, independent of this exactness GREEN. Flat-generated output stays
private; the final L146 output uses default page-table memory.

Input SHA-256 values match the producer:

- `patterns/lift.json`:
  `fc10bf973d0254615c94cd4ff88f18bd9d36bbac0bc85c80de9ed1890d1e27e9`
- Hot list:
  `2c5e98caaa4806abe37defc36f457e64dc14bc4bb0ce01e71378d9605cb63a94`

Opt-in generated source stays private. Head requested the final **default**
regenerated shards as a separate commit after helpers/generator/tests
(board #12406). No game/FPS acceptance is inferred from either gate.

### Typed x87 and live flag predicates

`X87::real_memory` replaces the byte-array conversion path for exactly
eight measured opcode/size/direction triples: FLD/FST/FSTP m32/m64 and
FADD/FMUL m32. It executes the same hardware opcode with a typed two-dword
operand buffer. Polling, acquire, stack/tag/status semantics and ordered
dword accesses remain per instruction. Whole-function emission selects
the helper; restartable step emission and other opcodes retain `memory`.
Register operations are unchanged: their exploratory specialization
regressed opt1. The inline XOR experiment was also rejected.

For a supported immediate CMP/TEST predicate whose flags remain live,
retain the defining ALU operation and its entire lazy flags representation,
but evaluate the predicate from its stored masked operands. This avoids
flag-query helper calls without rereading memory. Existing dead-flag
fusion remains separate.

Focused validation: **39 base tests** and **33 Python tests** pass. New
x87 comparisons cover **13,824** full architectural image, memory and
access/poll cases, plus **64** fault-position cases (44 faults, 20 controls).
Coverage includes precision/rounding modes, stack depths, special values,
parked/live FPU state, page permissions, unaligned/cross-page/wrapped
addresses, trait fallback and atomic memory mode. The generated flag
oracle compares full CPU/memory/call traces at both opt0 and opt1.

Benchmarks use opt0/opt1 callers with base/host opt3, CPU affinity 10,
100,000 iterations and 33 alternating paired samples per case. Numbers
below are median thread cycles, reference -> candidate; loop overhead is
included. They are synthetic costs, not gameplay FPS predictions.

| Operation / byte offset | Caller opt0 | Caller opt1 |
| --- | ---: | ---: |
| FLD+FSTP m32 / 0 | 186.51 -> 108.35 | 25.69 -> 26.17 |
| FLD+FSTP m32 / 1 | 209.26 -> 118.55 | 30.79 -> 29.94 |
| FLD+FSTP m32 / 4092 | 195.20 -> 116.53 | 25.77 -> 24.46 |
| FLD+FSTP m64 / 0 | 271.30 -> 124.54 | 48.93 -> 49.10 |
| FLD+FSTP m64 / 1 | 257.22 -> 127.60 | 52.09 -> 50.04 |
| FLD+FSTP m64 / 4092 | 263.86 -> 125.14 | 48.96 -> 51.37 |
| CMP / equal | 92.79 -> 88.35 | 14.04 -> 13.17 |
| CMP / signed less-or-equal | 97.02 -> 87.66 | 26.99 -> 9.35 |
| TEST / nonzero | 91.83 -> 87.82 | 16.32 -> 9.31 |

All 198 x87 opt0 pairs improve. **X87 opt1 is mixed**, including regressions
in three medians; no general opt1 x87 speedup is claimed. Offset 4092 is
page-end for m32 and spans two pages for m64. Reproduce with
`research/native/bench_x87_memory.py`, using the task target and heavy slot.

Final default generation completed in **730 s**: 59,734 functions, 1,477
modules, 33 crates and 44 scoped step functions. There are **62,511** typed
x87 call sites and 9,778 remaining generic memory calls; register calls
remain 7,827. Emitted `zf()` calls fall from 161,708 to **45,405**, and
`of()` calls from 6,431 to **1,274**. There are 130,879 stored-operand
predicate references. No flat-memory helper calls remain in this output.
The 1,547 Rust files total 269,874,484 bytes. Receipt SHA-256:
`f078f515d28fbbe9a17622263efb9cbe0ccf11ca0b771fec025da8491ed8525c`.

The final launch script hit a shell parse error after generation, before
either gate started. A gate-only launch reuses the completed frozen
generated output. All 33 opt0 artifacts are confirmed by Cargo metadata;
the opt0 native gate passes **107 / zero failures / nine ignored**, including
the inventory audit with the same five gaps. Artifact wall span is
**613.43 s**, including shared gameplay pauses; libraries total **432.52 MiB**.
The all-shard opt1 gate also passes **107 / zero failures / nine ignored**,
with all 33 Cargo artifacts confirmed at opt1. Artifact wall span is
**558.42 s**, libraries **356.06 MiB**. Both profiles keep base/host at
opt3, use four jobs and disable incremental compilation. Private profile
overrides and compiler-artifact receipts are archived with the logs.
No new inventory gap, gameplay run or image-comparison claim.

Implemented and tested: typed x87 operand path, live CMP/TEST predicates,
focused architectural/access regressions and both full native replays.
Not measured here: original-game image or FPS improvement. The final
default generated output is committed separately from helper/generator
changes as requested by head (board #12406).

Next action: head integrates the helper/codegen and default-generated
commits together, then profiles gameplay, especially guest 0x1038. X87
opt1's mixed synthetic result remains a performance limitation.
