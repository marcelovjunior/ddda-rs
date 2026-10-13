# L73: lean emission (compile speed of the ddlift shards)

Owner: claude-7. Branch `task/L73-lean-emission` (from `task/fps-int`).
Rules: [AGENT-RULES.md](AGENT-RULES.md).

## Brief

Less generated source per x86 instruction, and cheaper code at run time,
so the 32 ddlift shards compile faster at any opt level:

1. Dead-flag elimination in `research/native/lift.py`: within a basic
   block, a flag write that is overwritten before any reader emits plain
   arithmetic. Exact: flags stay live across block ends, calls and
   anything that may read them.
2. Idiom helpers (push, pop, call with return address, ...) as single
   calls into a new `ddnative_base::emit` module, where that measurably
   cuts compile time.
3. Measure each step on shard 05 and one other shard (opt-level 0 and 1,
   `-Z time-passes`), check run time with the vectors, and pass the gate
   (`lift.py gen`, `scripts/native_gate.sh`) through heavy.py.

## Results

Commits on `task/L73-lean-emission`: 16416285 (dead flags, push/pop and
packed-SSE helpers), 97f300d9 (constant-count shifts, tidy, folded SSE
addresses), 46cf6ded (split functions), plus this report. The passes are
in `research/native/lean_emission.py`, hooked into `cpu_arm`/`rust_fn` in
lift.py; helpers in `crates/ddnative-base/src/emit.rs` (new module, no
existing semantics changed). Description: NATIVE_PIPELINE.md, "Lean
emission (L73)".

### Verification

- Implemented and tested: `test_lean_emission.py` (text rules, plus a
  rustc differential test: literal, lean and split forms of synthetic
  arms built from lift.py's own templates must leave the same Cpu, memory
  and memory-event order for 121 register/flag/xmm inputs; two mutations
  of the passes were confirmed to fail it). All 302 research/native tests
  pass.
- `LIFT_LEAN=0 lift.py gen` reproduces the committed shards byte for byte
  apart from the new `use ddnative_base::emit;` header line, so the
  baseline below is the real current output.
- Not yet: gen + `native_gate.sh` (vector replay) on the lean shards, and
  run time (see "Open" below).

### Compile time (single rustc, one shard, `-Z time-passes`)

CPU seconds of the rustc process (wall in brackets where it matters);
the machine was shared with other agents' builds, so only runs made back
to back are compared, and each figure is a mean of the repeats shown.
`lean` is all passes except splitting, `lean+split` adds 16 KiB parts.

| shard | opt | base | lean | lean+split |
|---|---|---|---|---|
| 05 | 0 | 35.9 (36.2, 35.6) | 24.4 (24.0, 24.8): -32% | 26.1 |
| 12 | 0 | 26.2 (26.0, 26.4) | 18.7 (18.6, 18.8): -29% | |
| 05 | 1 | 322 [384 wall] | 196-282 [107-153 wall] | 195, 254 [101, 132 wall]: -10..-17% vs lean in pairs |
| 12 | 1 | 303-330 [122-132 wall] | 331-364 [153-164 wall] | 297 [130 wall]: -18% vs lean in a pair |

Source size: shard 05 10.99 -> 8.64 MB, shard 12 11.33 -> 8.84 MB.

What each step bought (shard 05, opt-level 0, same-session runs):
dead flags about -8% (35% of flag writes removed: 20,648 -> 13,479; flags
stay live across calls, which is what keeps the rest); push/pop helpers
about -5%; the packed-SSE rewrite about -15% (the `for k in 0..4` lane
loops were the costliest construct per byte; at opt-level 1 they were
what made shard 05 take 384 s: 110 s wall after this step alone); the
remaining folds about -10%. Splitting costs 1-2 s at opt-level 0 and
saves 14-21% wall at opt-level 1, so it stays (the opt-level-1 shards
dominate a play-fast build).

Tried and rejected (measured, slower or no better): free functions
instead of methods for address arithmetic (`wrapping_add`) and for the
memory accessors (+10-20% at opt-level 0), and the same for live ALU
calls and `cond`; parts of 8 KiB (worse than 16), 32/64 KiB (no better);
`#[inline(never)]` on the parts is unresolved (one run, 183 CPU s but 333 s wall during a build pause).

### Decision for the head: memory accessors out of line at opt-level 1

With the six `fast_*` accessors behind an `#[inline(never)]` wrapper,
opt-level 1 drops to 126 CPU s / 55 s wall for shard 12 (from 303-364 /
122-164) and 143 / 73 for shard 05: the inlined page-table fast path at
~90,000 access sites per shard is most of the remaining LLVM time. The
cost is a call per guest memory access in opt-level-1 shards (run time
not measured). mem.rs is the head's; not changed here. Related (claude-8,
L74 hookbench): `#[inline]` helpers are compiled unoptimised into
opt-level-0 shards, while `#[inline(never)]` ones in ddnative-base run at
opt-level 3; an `emit` that is out of line for opt-level-0 packages and
inline for opt-level-1 packages would give both.

### Open

- gen + gate (`native_gate.sh`, heavy slot) on the lean shards.
- Run time: ddperf replay or the lifted test, lean against base.
