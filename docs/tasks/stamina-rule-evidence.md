# stamina-rule-evidence: Stamina timing and effective rates

- **Owner:** codex-12 (Codex lane; head assignment #417)
- **Branch:** `task/stamina-rule-evidence`
- **Board area:** `formats/player-stamina`
- **Status:** review

## Goal

Resolve the timing factors and selected modifiers needed to reproduce recorded
sprint drain, recovery delay and regeneration. Deliver source-backed pure rules
for later M2 core integration; do not tune constants to match a recording.

## Evidence

Read PARAMS.md, RECORDINGS.md (005707), METHODOLOGY.md and `ddparam`.
The recording gives roughly -45.3 stamina/s sprint, +48.9/s idle recovery and
0.5-second recovery hold for one mage with max stamina640. These are an observed
case, not universal rates. Start at 0x00b81120,0x00b81270,0x00b81810 and
0x00b6b350; resolve the timing-factor fields and per-action consumers already
identified in PARAMS.md. Use reflected names wherever available.

## Scope

- `crates/ddparam/src/stamina.rs`
- Minimal `crates/ddparam/src/lib.rs` export only
- `crates/ddparam/tests/stamina.rs`
- `docs/STAMINA.md`
- This brief's Results

Coordinate claude-1's movement work and head before any core integration.
Do not modify ddsim/ddworld/ddview or reinterpret the unrelated horizontal-scale
chain as stamina evidence.

## Work and original-game evidence

1. Trace ordinary drain, one-shot cost, regeneration hold, exhaustion and cap
   branches through their selected parameter rows, equipment/weight modifiers,
   and cUnit/sMain delta sources. Identify ordering and rounding.
2. Request existing private recordings or read-only observations through claude-4
   of current stamina/base/level, action, timing factors, delta and relevant
   modifiers for ordinary sprint/stop/idle play. Ask for context sufficient to
   distinguish class, load and augmentation; never request memory patches,
   state setters, injected calls or other process writes.
3. Recompute the documented case from observed inputs. State measurement windows
   and error bounds rather than silently substituting desired rates.
4. Translate only supported pure steps with explicit actor-specific inputs,
   source comments and regression tests for delay, exhaustion and clamp edges.

## Done when

The recorded case has an evidence-backed explanation (or a precisely isolated
remaining input), covered branches have meaningful regression tests, and an
original comparison identifies what was observed versus inferred. Any missing
observation is a reported blocker, not a claimed comparison. No simulation
singleton or core change. Fill Results, release claims and report codex-1.
Head alone merges; never push or write the install/process, directly or through
another agent. Keep all raw/private captures out of git.

## Results

Implemented `ddparam::stamina` arithmetic fragments with explicit actor inputs:
clock scaling, selected recovery/debt, dash and dash-jump costs, locomotion row
truncation, resolved exhaustion/clamps. No core or live-process edits.

Evidence: disassembly of 0x00b81450 establishes recovery-before-drain and
cUnit-delta debt advance versus sMain-delta spending; 0x004c5e10 proves the
factors are sWorkRate.mRate[1]*mRate[0]. 0x00b81810's debt value overrides the
normal recovery rate. 0x00acdac5 spends dash-jump mStamina once, without a time
factor. Modifier, max and tired-action chains are documented in STAMINA.md.

Existing private CSV005707 supports effective per-clock-unit rates -1.5/+1.625:
55/57 sprint changed-sample ratios and 96/96 idle ratios lie within 0.0001.
The exact sprint window 1.50235..2.49526 is -45.62100/s; hold boundaries yield
0.58953..0.61433s, not proof of a fixed 0.5-second timer. Body/load rows,
augments, work-rate factors and recovery gates were not captured; the compatible
row1/neutral-modifier explanation is explicitly an inference, not a proven
historical selection. This is the isolated remaining observation blocker.

Validation: `cargo test -p ddparam -- --nocapture` passes 21 tests (9 new rules,
3 existing synthetic, 9 existing install); 1,021 copies/61 parameter paths
parse. Clippy all-targets with warnings denied passes. No new original-game
capture or claim of complete stamina/action parity.

Next action: claude-4 read-only observation during ordinary maintainer play,
with the exact fields listed in STAMINA.md, to resolve historical-equivalent
row/modifier/hold gates before core integration. Requests expressly prohibit
injected input, setters, function calls, teleport and all process writes.
