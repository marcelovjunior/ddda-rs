# L178: semantic job-thread routines

Assigned by head to codex-7 on `task/L178-semantic-jobs`, based on
`task/fps-line` 1fcd6330. Scope: 010A35D0, D7CF70, C24640, their isolated
oracles/tests and semantic registrations. No game runs or shard builds.

## Results: 010A35D0

Implemented `mtcore::semantic_jobs::query_bounds_tree`, registered only
under `DDRUNTIME_SEMANTIC=1`. Source: DDDA.exe 2017-12-13 Steam,
010A35D0..010A3BF8; read-only Ghidra disassembly/decompilation.

The semantic loop replaces four expanded SSE traversal arms. It retains
the guest work stack, per-lane nesting counter, child push/leaf callback
order, four-lane comparison masks, both flag words and observable writes.
The lane >=25 path calls original 010A2460. Callees remain explicit guest
contracts, including callback mutations; neither callback nor fallback is
reimplemented here. Uses `Pages`, including its atomic backing accesses.

Verification:

- **192 original instruction returns**, synthetic trees of varying depth,
  mixed child/leaf masks, NaNs/subnormals, all stack alignments and lane
  0/1/7/24/25/FFFFFFFF, both nesting slots, callback mutation of later bounds.
- **369/369 decoded body instruction addresses**, plus both callee targets.
- ABI comparison: EAX, EBX/ESI/EDI/EBP, returned ESP, all observable guest
  memory (including heap below ESP), callback receiver/arguments/return PC
  and visible memory at every callback. Only discarded stack scratch is
  excluded. Corrupting a heap pointer is deliberately rejected.
- **Four isolated tests pass**: semantic vectors, opt2 generated-body
  vectors, serial/atomic `Pages` replay of both implementations, corruption
  rejection. First benchmark run including replay: **11.57 seconds** after
  build. Generated bodies remain private; no ddlift shard is compiled.
- **77 paired costs**, 63 improve; median paired speedup **1.243x**, median
  per-case ratios **1.074–1.741x**. Some individual pairs regress on this
  shared host. Costs include identical input resets and recorded callback
  checks; these are synthetic caller costs, not frame/FPS measurements.

Private corpus: `E:/ddda-re/notes/native-L178/010a35d0.json`, SHA-256
`1a6b97517053fec344f60103bb8a9c06b6ab65fe6e32f7727908a1b18d7275fd`.
Reproduce with research Python and the shared heavy slot:
`research/native/l178_record.py`, then `python scripts/check_l178.py --bench`.
Build target `C:/ddda-targets/L178`; incremental off, four build jobs.

Runtime light suite initially failed the unrelated timing assertion
`present_runs_without_the_guest_lock_in_parallel_mode` during shared load;
its isolated retry passed. Final light attempt: **108 pass / two timing
failures / five ignored**, stopping before integration tests. The other
failure was `render_tail_avoids_reacquiring_a_busy_guest_lock`. No runtime
code changed. `mtcore`/`ddports` library gate: **five pass**, semantic
vector/corruption tests: **two pass**. A broad legacy `core_edge` test ran
for several minutes and was stopped; no full-crate-pass claim is made.

Not compared against gameplay; head owns the accumulated integration run.
Next: head train acceptance; complete D7CF70/C24640 original-vector gates.

## Remaining routines

- D7CF70: source inspected. Swept collision/simplex routine, including x87
  copies in triangle-reduction branches; must preserve copy quieting and
  exact SSE operation order. No port or acceptance claim yet.

## Results: C24640

Implemented and registered `semantic_instancing::draw_instanced_mesh`,
the complete `uOmSwingBaseInstancing::vf26` body, under SEMANTIC.
Source: C24640..C24BC1, same executable. Retains material scope changes,
revision checks, stream bindings, bounds reporting, instance selection,
and original opaque guest callees. Nonstandard C242C0 fallback consumes
EAX/ESI/EDI; C26310 consumes EDX plus thiscall arguments. Tests check these
register arguments and the callee-visible selection/sphere buffers.

- **256 original returns**, **337/337 body instructions** plus 14 callees.
  Corpus covers missing/inactive instancing, visibility gates, zero/multiple
  primitives, same/changing materials, both revision records, state logging,
  optional bounds reporting, NaNs/infinities/signed zero/subnormal matrices.
- **Five combined isolated tests pass**, including opt2 baseline and both
  serial/atomic Pages. C24640 returns void; EAX is excluded for this routine,
  while all callee-saved registers, ESP and observable memory are checked.
- **77 paired costs**, 58 improve; median **1.076x**, per-case medians
  **1.010–1.299x**. Timing includes the identical mock-call validation and
  input resets. Small gain under shared load, no gameplay claim.
- Corpus `E:/ddda-re/notes/native-L178/00c24640.json`, SHA-256
  `4949abd0da23ec1bae1e489034129c52f0bca33ebd77ae5beff3f8a5c6dc6847`.
  Reproduce via `research/native/l178_instancing.py` in the heavy slot, then
  `python scripts/check_l178.py --bench` (24.25 seconds after build).

Next: head combined train; finish D7CF70 original-vector proof. Prior light
timing-test failures remain the full runtime gate limitation, not a pass.
