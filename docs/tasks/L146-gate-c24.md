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
L143/L141 final acceptance still awaits the combined CPU/runtime suite.

Generation: **808 s**, 59,734 functions, 1,466 modules, 33 crates, 44 scoped
step functions. All 33 shard libraries rebuilt; fingerprint-start to final
rlib wall span **225.51 s**, aggregate rlibs **434.81 MiB**. This span includes
scheduling and is not summed CPU time. Generated source receipt:
`77e6cc03d4b7334a53aa510c484855aef3af988eed9d7dc6799eb27865f22dfe`.
Private logs, SHA receipts and scripts: `C:/ddda-targets/L146`.

### Profile and experiment

`profile_c23def.txt`: guest thread 0x1038 is busy for 115/347 sampled
stacks (31% CPU). FLD m32 and FSTP m32 generic helper self shares are
16.5% and 10.4% of busy samples. Page-table polling is 15.7%, scalar
load 11.3%, scalar store 3.5%. These are sampled attribution, not an
instruction census or whole-game performance prediction.

Private initial fixed-opcode helper experiments substantially improve
opt0, but m32 is slightly slower at opt1; no production adoption yet.
The experiment preserves per-instruction polling and ordered dword
accesses. Pair fusion is not justified by these measurements.

Next: complete the frozen combo24 gate, then the separately pinned flat
gate before promoting any helper/codegen change.
