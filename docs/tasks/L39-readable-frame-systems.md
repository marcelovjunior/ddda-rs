# L39: readable ports of the clock and the frame-rate-sensitive systems

- **Owner:** claude-8
- **Branch:** `task/L39-readable-frame` (worktree `E:\ddda-rs-wt\L39`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md), merge-train rule

## Why

The player wants an unlocked frame rate. `docs/FRAME.md` section 1a (the
clock) and `docs/FRAME_RATE.md` (L30/L30b, the audit) found that the
frame rate is a setting, but a few systems break above the original's
cap. A high-frame-rate mode will change exactly those systems. That work
needs readable, verified Rust versions of them first, so each change is a
small, reviewable diff against a 1:1 baseline:
- the clock and frame pacer `sMain::vf06` (0x00DBFA90) and its helpers;
- `sRecognition` (0x00487B00), the frame-counted perception timers;
- cloth `uSimSoftBody::vf11` (0x00F20640), the dropped step remainder;
- the Frozen/Petrify velocity integration (0x0084CCC0);
- `cEm5501ActAtckRushBite::vf06` (0x00A36DF0), the per-frame lunge;
- the effect noise in time-source mode 4 (the sEffect frame counter);
- `uChain::vf09` (0x00F0C160): dt-approximate, its substep loop.

## Task

1. **Port each function** (and the helpers it needs) as readable Rust,
   exactly as in L24/L33:
   - bit-exact;
   - verified by the lifted vectors plus recorded edge cases;
   - wired in through the PORTS table.
   Leave a function lifted when a port would replay dead state (L24's
   audit), and record why.
2. **Document, per function,** what the high-frame-rate fix from
   FRAME_RATE.md section 6 would change in the port. Do not implement it:
   the default stays 1:1. Write it as a short design note in
   `docs/FRAME_RATE.md`.
3. The light checks are yours; the full replay and wiring go through the
   train.

## Results

claude-8, branch `task/L39-readable-frame`. It is stacked on L33, which is in train 2,
and has main merged in. The design note for modders is
[FRAME_RATE.md section 8](../FRAME_RATE.md).

### What landed

The new crate is `crates/mtframe`. Its ports are wired through `lift.py`:
`PORT_CRATES` gains mtframe, and shards 01, 08, 10, 14 and 15 are
regenerated.

| Function | State |
|---|---|
| `sEffect::vf06` 0x00FB64D0 (the frame counter) | strict port, wired |
| time-source selectors 0x011E48E0, 0x00E6C260 | strict ports, wired |
| `cEm5501ActAtckRushBite::vf06` 0x00A36DF0 (lunge) | strict port, wired |
| `uChain::vf09` 0x00F0C160 | strict port, wired |
| character integration 0x0084CCC0 (Frozen, Petrify, Euler) | strict port, wired |
| `sRecognition::vf06` 0x00487B00 | lifted; observable-equivalent model `recognition::update`, not wired |
| `uSimSoftBody::vf11` 0x00F20640 (cloth) | lifted; its step schedule 0x00F20847..0x00F208FC is the model `cloth::schedule`, not wired |
| `sMain::vf06` 0x00DBFA90 (clock) | lifted |

Why three stay lifted (decided with the head): a strict port would have
to replay dead state, as with L24's blend.

- **sRecognition**: about 30 callouts plus four inline array grows of six
  calls each. Scratch registers carry loop state into later callouts: at
  the cEnemyInfo allocation, ecx and edx hold what the previous phase
  left. At the grows, ecx holds the class name pointer from the isKindOf
  walk and edx holds getDTI's output.
- **The clock**: the x87 stack carries mFps and the counter frequency
  across ftol, `__allmul`/`__aulldiv` and Sleep, with a control-word switch
  for truncation; its 19 callouts carry the same kind of state.
- **Cloth vf11**: 11 KB and about 40 callouts, while the frame-rate part is
  the accumulator.

The models are verified on their observable behaviour (see FRAME_RATE.md
section 8) and are the high-frame-rate mode's baseline.

Also landed:
- `smain` (sMain's timing layout) and `fpu` (x87 operations on the
  guest's x87 state, so precision control is honoured).
- `research/native/vectors_frame.py`:
  - fakes for direct exe callees;
  - float-returning stubs;
  - bump-allocator stubs;
  - code-range recordings.
- `docs/NATIVE_PIPELINE.md`: a subsection on the readable ports wired over
  lifted bodies (mtmath, mtcore), OK'd by claude-3.
- The inventory credits mtframe; `native_gate.sh` runs its tests.

Found on the way, and merged into train 2 (430befb): the edge replays
restored the whole recorded stack window after each callout, which hid a
push or spill made at the wrong time. The recorders now keep the window at
each callee's entry and the replays compare it (mtcore, mtmath, mtframe).
This found one break in the L24 port of fromEuler (0x00CFC1D0), now fixed.

### Verified

`cargo test -p mtframe` in release, with the vectors required:
- 2,400 edge cases (400 per strict port), including x87 precision control
  for the chain and NaN inputs;
- the lifted vectors that stay inside a port (3 of sEffect::vf06's enter
  another lifted body and are checked by the integration replay);
- the two models against 400 recordings each.

Mutation checks fail between 19 and 400 cases each. They cover spills,
xmm at calls, the Frozen/Petrify factors, the lunge window, the counter,
the selector, the recognition timers, records and always-list, and the
cloth remainder.

Clippy is clean for mtframe. The Python tests (wiring, shards, inventory)
pass.

Private recordings, in `E:/ddda-re/notes/native/vectors/`:
- `frame_lift.json`
- `frame_edge.json`
- `frame_models.json`

### Pending the merge train (train 3)

The full ddnative replay with the six ports wired, and the benchmark.
After merging, rerun `python research/native/lift.py gen` and check that
no shard `mod.rs` changes.
