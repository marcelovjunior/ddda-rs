# L170: opt-level 2 helper cost and inline ALU state

Owner: codex-7. Branch: `task/L167-critical-callbacks` (merged
`task/fps-line` first). Task assigned by head through the user, 2026-10-13.
Follow `AGENTS.md`, `AGENT-RULES.md`, and `NATIVE_PIPELINE.md`.

## Scope and implementation

Audit DD42C0 and uSkyStableShadow's EFD240/DB63E0/FEE320 at opt-level 2.
Implement one independently selectable generator change, with no shard
build, GPU test or game execution. Coordinate with codex-10's register
locals work; board #13786 approves only the final `shard_fn` hook.

`LIFT_INLINE_FLAGS=1` retargets surviving ADD/SUB/AND/OR/XOR lazy-flag
operations to `Flags::alu_*_inline`. The extension methods live in
`ddnative-base/src/emit/flags.rs`, with normal `#[inline]` hints. Neither
`x86.rs` nor existing out-of-line methods change. The switch is off unless
its value is exactly `1`; unset/0 generation is unchanged. Regeneration
is required to enable it; this is not a runtime environment switch.

All operands, widths, results and six lazy-flag fields remain identical.
No liveness assumption, condition folding, register writeback rule, guest
memory access, guest call or x87 operation changes. Retargeting supports
both `cpu.fl` and codex-10's `l161_fl` receiver. It runs after shard call
rewriting and applies to split function parts too. Instruction-step
generation is unchanged. The independent exhaustive helper comparison
protects the duplicate formulas against future drift.

## Cost ranking

These are static call-site counts, **not executed counts or nanoseconds
per guest instruction**. Original gameplay input coverage and dynamic
frequencies belong to the head/codex-6 corpus. Out-of-line guest-memory
access also forces argument setup and register reloads around calls.

The head's `C:/ddda-targets/head-asm/f_o2.s` contains DD42C0 compiled in
shard 27 at opt-level 2. Its 40 memory calls comprise 25 u32 reads,
13 u32 writes, one u8 and one u16 read. Integer flags account for ten
calls: five `alu_sub`, one `alu_and`, three `zf`, one `cf`; SSE comparison
has one further helper call. Push/pop wrappers are already inlined.
There is one page-table virtual lookup at function entry, a cross-shard
call, and one same-shard callee with its override slow-path branch.

The same shard's DB63E0 has 127 memory calls (64 u32 reads, 56 u32 writes,
four u8 and three u16 reads), 15 flag calls (seven SUB, three ADD, two OR,
three ZF), ten `call_ext` sites and one page-table lookup. Neither of
these two function bodies calls x87 helpers.

Ranking for the measured bodies: memory calls first (claude-9 owns this),
then surviving flag helper calls (this change), then guest-call dispatch
and entry setup. Push/pop need no separate opt2 wrapper fix. x87 cannot
be ranked as a cost in a function where it has no sites. The PREEMPT check
is inside ddnative-host's cross-shard `call_lifted` path; it is not an
unconditional check at every function entry. In-shard `call_local` already
inlines its override-bit check. This patch leaves those contracts intact.

## Reproduction

Use `CARGO_TARGET_DIR=C:/ddda-targets/L170-codex7`, `CARGO_BUILD_JOBS=4`,
`CARGO_INCREMENTAL=0`. Respect the head's pause while the game A/B runs.

```
E:/ddda-re/tools/pyenv/Scripts/python.exe -m unittest discover -s research/native -p test_inline_flags.py
E:/ddda-re/tools/pyenv/Scripts/python.exe -m unittest discover -s scripts -p test_l170_check.py
cargo test -p ddnative-base --test inline_flags
E:/ddda-re/tools/pyenv/Scripts/python.exe scripts/l170_check.py --bench
```

The isolated check compiles only four selected functions and four closed
support leaves, using opt2 bodies and opt3 base/host helpers. Both A/B
variants share the same recorded external-call boundary, complete strict
original-vector replay, page-backed atomic fixture timing and output
checks. The whole-function fixture paths are synthetic, not recorded
gameplay. Generated Rust, input vectors, assembly and logs stay under
the private target directory. Existing shards are not changed or built.

## Results

**Implemented and isolated-tested, not compared in game.** Helper tests
pass: 429,370 baseline/candidate ALU pairs
cover every byte operand pair with dirty upper bits, wider boundaries,
and deterministic random operands. Each checks result, all flag fields,
16 conditions and LAHF. Seven emission tests pass, including default-off,
suffix-only, local-flags and shard-hook checks. Two assembly-audit tests
pass (local branches are not helper calls; split parts count with roots).
Full base suite: **43 pass, zero failures, one ignored**.
Fresh-target light runtime suite with `--no-default-features` and serial
test execution: **255 pass, zero failures, seven ignored** (37 result
groups). No generated shard or GPU test was built/run by this task.

The four functions each have three original-instruction fixtures:
**12 baseline and 12 candidate returns pass** at opt2. The exact original
register/call/memory replay remains enabled. The atomic page-backed
benchmark additionally checks complete lazy-flag fields, all XMM lanes,
exit registers, DF/FS/system flags and every fixture memory byte across
both variants, outside the timed calls. All 84 paired samples pass those
state checks. These fixtures are narrow synthetic paths, not full branch
coverage or recorded gameplay.

Opt2 isolated assembly, counting the root **and any out-of-line split
parts** (FEE320 baseline has two host functions, candidate one):

| Function | Guest instructions | Fast memory calls | Flag calls, before → after | x87 calls | Host instructions, before → after |
|---|---:|---:|---:|---:|---:|
| DD42C0 | 94 | 40 | 10 → 4 | 0 | 463 → 466 |
| EFD240 | 259 | 194 | 17 → 3 | 6 | 1,885 → 1,881 |
| DB63E0 | 236 | 127 | 15 → 3 | 0 | 1,256 → 1,267 |
| FEE320 | 763 | 447 | 35 → 8 | 23 | 5,370 → 5,302 |

Fast-memory call-site densities are respectively 0.426, 0.749, 0.538,
0.586 per admitted guest instruction. They remain unchanged. Flag-site
densities fall 0.106→0.043, 0.066→0.012, 0.064→0.013, 0.046→0.010.
These ratios characterize static code shape, not runtime instruction
cost. Removing a call does not necessarily reduce caller instruction
count: the small DD42C0/DB63E0 increases include explicit flag stores.
No surviving `alu_*_inline` out-of-line call remains in these assemblies.

EFD240/FEE320 include real x87 sites (six/23 direct helper calls), below
their fast-memory counts and unchanged by the switch. Indirect calls
include memory/hook paths, so the table is not a complete dynamic cost
attribution. The isolated guest calls are recorded external seams in both
variants; they do not benchmark production dispatch/cache/PREEMPT costs.
DD42C0 and DB63E0 counts were also cross-checked against the head's actual
opt2 shard-27 assembly. EFD240/FEE320 use the isolated opt2 compilation;
their production shard assembly was not available, and no shard was built.

Seven alternating rounds per fixture yield 21 timing pairs per function.
Median baseline/candidate ratios: DD42C0 **0.981x** (9/21 faster), EFD240
**1.113x** (16/21), DB63E0 **1.000x** (10/21), FEE320 **1.022x** (13/21).
Timing is noisy on the shared host; across samples the range includes
regressions for every function. No general function-speed or FPS gain is
established. Keep the switch opt-in for the real-input corpus and train.

Private receipts from the isolated run are under `C:/ddda-targets/L170`:

- `check.log`: SHA-256 `502c53b5b401c8db7e9cd693433516546c3abf81cd36690a67fc77141a0ce04d`.
- `isolated/assembly-counts.json`: `c00eafd0f63ebeb07c972c3569aa9fa66b3062136e1e23365130483b3c581dd5`.
- Head `f_o2.s`: `7f0302f35465f04e0b4bd39ba6b171b0d64fd60b6744841d5c23c3e12dc903f7`.

The JSON is the corrected audit receipt; the earlier table printed inside
`check.log` incorrectly counted local jumps and omitted FEE320's split
part. `--analyze-only` regenerates the corrected JSON without compilation.
The two new audit regression tests cover those mistakes.

The first isolated replay exposed a harness omission: EFD240's closed
FED8F0 size getter must execute, as it did in the original recorder.
Both baseline and candidate failed the same call-target assertion. The
support dispatch now includes that real lifted leaf; no assertion or
fixture is weakened. An initial full-base run in the first target returned
stale helper definitions (source line locations differed from this tree).
A fresh `L170-codex7` target passes all base tests. The cause of the stale
artifact is unconfirmed; do not reuse the old debug target for validation.
That old target's concurrent runtime run also reported the two previously
known timing-sensitive tests failing (Present/interlocked). The fresh
target's complete serial light suite passes both; no runtime code changed.
Fresh-target log hashes: `base.log`
`4a0cf7a642007a440bb223d21a2318aa1364f84c0caeb355520166eb3d10877c`;
`runtime.log`
`d83b323b2f47f0532f1173a09de2db3b79450fba40c1583a083e49b17a671253`.

Head combined regeneration/native gate and live image/FPS comparison
remain necessary. No gameplay or FPS gain claimed. Next: head's combined
generation with `LIFT_INLINE_FLAGS=1`,
exact native gate and recorded-gameplay corpus before a default change.

Milestone update is queued with head/codex-9 (board #13804, #13828,
#13832): codex-9 holds `docs/MILESTONES.md`; no conflicting edit or forced
claim was made. Suggested entry: opt-in inline ALU helpers; 429,370 exact
pairs, 12 original fixtures in each variant, opt2 flag sites 77→18 across
four functions with unchanged memory sites; nine Python, 43 base and
255 light-runtime tests pass; noisy synthetic timings/no FPS acceptance;
next head shared generation, exact gate and real-input corpus.
