# L129: flag materialisation in lifted game code

Owner: codex-7. Branch `task/L129-dead-flags`, base `df392af7`
(`task/fps-line`). Rules: [AGENT-RULES.md](AGENT-RULES.md).

## Brief

Measure dead flag setters and single-condition consumers in rendering and
boot hotspots; remove only provably unobservable flag updates and emit
direct comparisons where flags are dead after the consumer. Coordinate
`lift.py` with codex-6's floating-point idiom pass. Keep restartable
instruction entries literal. Full regeneration/native gate needs head's
go and `heavy.py` with `HEAVY_BIG=1`; only head runs the game.

## Results

### Step 1: L73 ancestry and missing predicates

- Contrary to the initial board description, this base already contains
  `16416285`, `97f300d9` and `46cf6ded`: local dead flags, helpers and
  function splitting. Do not cherry-pick those again. The missing
  `aae0bdd8` predicate expansion is restored from L73, preserving
  `Flags::cond`'s predicate order and short-circuit behavior.
- Existing L73 standalone Rust fixture repaired for the current
  `ddnative-base::hooks` dependency; temporary build output respects
  `CARGO_TARGET_DIR` (default `C:/ddda-targets/L129`).
- **15 Python tests pass**, including the compiled differential fixture
  comparing literal/lean/split output for **121 register/flag/XMM inputs**,
  full final Cpu, memory, ordered memory events and call-boundary Cpu.
- L73's historical shard timings are not new measurements. Full vector
  replay, regeneration, native gate, compile-time and gameplay effects
  are not verified by this step.

Next: CFG dead-flag analysis and direct single-consumer comparisons,
then head's integration regeneration/gate and matched gameplay profile.

### Step 2: whole-function liveness and single-condition fusion

- `rust_fn` applies a conservative successor analysis before dispatcher
  splitting; `rust_step` and `LIFT_LEAN=0` remain literal. Literal direct
  edges only; the least fixed point starts with flags live, so cycles
  without a proven overwrite retain flags. Both conditional successors
  must overwrite before any read. Calls, returns, unknown transfers,
  partial writes, variable shifts and INC/DEC's carried CF stay barriers.
- CMP/TEST immediately followed by one Jcc/SETcc/CMOVcc can become native
  integer comparisons when flags are dead afterwards. Width masks and
  signed operand casts preserve 8/16/32-bit comparison semantics; each
  operand is evaluated once in original order. CMOV's eager memory load
  stays in place. Unsupported predicates and arithmetic destinations are
  conservatively retained. No flags are assumed clobbered by an ABI:
  `x86::call_lifted`/`call_ext` share the entire Cpu with callees.
- **23 focused tests pass** after the `rust_fn` hook: existing 15 and
  eight new tests. Two compiled 121-input fixtures compare final Cpu,
  memory, memory-event order and call-boundary state; direct predicate
  truth tables additionally compare against real `Flags` helpers over
  15 edge values per operand, all supported widths and condition aliases.
- Static sample: all named functions in `profile_boot1.txt`, DAB480's
  direct callees, DAA5F0 and FE9580: **44 functions / 1,793 blocks /
  2,445 flag setters / 941 condition consumers**. Existing L73 removes
  **872 (35.7%)** setters; this pass removes **544 more**, including
  **334 fused comparisons**. Together **1,416 / 2,445 (57.9%)**.
  Counts are static sites, not dynamic profile weights.

| Function | Setters | Existing local dead | Additional removed | Fused |
|---|---:|---:|---:|---:|
| DAB480 | 179 | 61 | 43 | 33 |
| DAA5F0 | 43 | 15 | 8 | 8 |
| FE9580 | 110 | 41 | 6 | 1 |
| 012EB690 | 75 | 19 | 15 | 10 |
| 00CF34D0 | 58 | 44 | 3 | 2 |

`research/native/measure_dead_flags.py` streams the private lift cache
without retaining the full 392 MB JSON. Reproduce with `--profile
E:/ddda-re/notes/runs/profile_boot1.txt`; `--all` surveys all functions.
The scratchpad `profile_combo9.txt` path was requested from head but has
not been located. Final compact emission recount confirms the same site
counts: lowered sample source **753,183 -> 753,333 bytes (+0.020%)**
relative to local L73 plus restored predicates. This does not include
dispatcher wrappers or measure compiler work; compilation effect remains
unmeasured. Private count receipt: `E:/ddda-re/notes/L129-flag-sample.jsonl`.

**Not verified at step 2:** regenerated original lifted vectors, full native
gate, shard compile time, original image/gameplay and FPS. Head requested
to batch this with codex-6 `cd080ced` in the next regeneration. No game,
GPU or shard build/run performed by this agent.

Next: head integrates both commits, regenerates under `heavy.py` with
`HEAVY_BIG=1`, runs `scripts/native_gate.sh`, and compares loading/render
profiles with the same image and scene. Do not treat static savings as
an FPS claim.


### Step 3: combined regeneration and native gate (2026-10-13)

- Merged `task/fps-line` at `05b35e35`, including codex-6 float fusion
  `cd080ced`. `lift.py` merged automatically with both hooks: float fusion
  in `cpu_arm`, whole-function flag liveness in `rust_fn`; resolved the
  milestone conflict by retaining both branches' entries.
- Regenerated with `LIFT_DATA=E:/ddda-re/notes/native`, `LIFT_WORKERS=4`
  and `--hot-functions E:/ddda-re/notes/L66/profile-text-hot.txt` under
  `heavy.py`, `HEAVY_BIG=1`. **59,734 functions / 1,466 body modules /
  33 crates**, including **44 scoped instruction-step functions**.
- **26 focused Python tests pass** (8 dead flags, 15 lean emission,
  3 scalar float-compare tests). The combined original-vector replay
  passes **178,130 cases / 59,734 lifted functions**, with **zero failing
  lifted functions**. All 75 CPU-dependent fixtures name this host's
  AMD Ryzen 7 5700X3D processor.
- **Full `scripts/native_gate.sh` passes**, using `NATIVE_PROFILE=play`,
  `DDNATIVE_REQUIRE_VECTORS=1`, `CARGO_INCREMENTAL=0`, dedicated target
  `C:/ddda-targets/L129`, at most four build jobs through the heavy slot.
  **49 tests pass, zero failed/ignored**: 32 ddnative and 17 across
  mtmath/mtcore/mtframe, plus inventory declaration checks. Final lifted
  replay took **28.49 s**; the complete warm recheck took **99.06 s**.
- The first gate passed every lifted vector but failed
  `x87_exact::qword_registers_sqrt_and_host_state`: it inserted host
  `FLD1` while guest state was live and expected per-instruction host
  restoration, predating L62's documented live-state contract. The test
  now calls `park()` at host boundaries, retains the host control-word
  and 1.0-value assertions, and additionally verifies guest control word,
  depth, extended top, and reacquisition across host work. No runtime
  behavior, comparison tolerances, fixture admissions or ignores changed.

Measurements from the clean target (not a baseline speedup comparison):

| Measure | Result |
|---|---:|
| Shard compile wall span, first shard start to last shard end | 264.95 s |
| Sum of 33 crate compilation durations (elapsed, not CPU time) | 1,011.45 s |
| Longest shard compile (`ddlift-26`) | 45.41 s |
| Compiled 33 shard `.rlib` files | 455,943,596 bytes (434.82 MiB) |
| Generated Rust, including module indexes, before | 259,005,953 bytes / 1,494 files |
| Generated Rust after | 259,574,211 bytes / 1,499 files |
| Generated source change | +568,258 bytes (+0.2194%) |

The requested hot-function layout compiles the 32 address shards at
opt-level 0 and the hot shard at opt-level 1 in the play profile. Cargo
HTML timing reports and logs are under `C:/ddda-targets/L129`; summary
receipts are `combined-size-timing.json` and `vector-counts.json` there.
Generation's **24m48.49s wall time includes repeated head-run suspensions**
and must not be used as an uninterrupted generation benchmark. The first
cold ddnative compile/test attempt took 379.76 s including the stale-test
failure; the final full gate reused the same compiled shards.

Inventory retains the existing 5 uncredited verification gaps and 17
fragments credited through owners; census remains 59,450 verified of
59,469 non-platform functions. Passing available vectors does not close
those coverage gaps. No game or GPU run was performed by this agent;
image, loading and FPS effects remain unmeasured.

Next: head measures this committed combined emitter in-game with the same
scene and loading route; later helper/port changes need their own combined
gate on a pinned integration revision.
