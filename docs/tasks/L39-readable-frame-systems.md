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

(open)
