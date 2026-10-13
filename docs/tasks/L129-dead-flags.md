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
not been located. Initial verbose candidate source was 2% larger; final
compact emission size and compilation effect still need measurement.

**Not verified yet:** regenerated original lifted vectors, full native
gate, shard compile time, original image/gameplay and FPS. Head requested
to batch this with codex-6 `cd080ced` in the next regeneration. No game,
GPU or shard build/run performed by this agent.

Next: head integrates both commits, regenerates under `heavy.py` with
`HEAVY_BIG=1`, runs `scripts/native_gate.sh`, and compares loading/render
profiles with the same image and scene. Do not treat static savings as
an FPS claim.
