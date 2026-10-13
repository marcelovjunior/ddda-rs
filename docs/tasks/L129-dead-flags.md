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
