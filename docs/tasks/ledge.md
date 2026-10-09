# ledge: ledge grab and climb, turn rate, stop motions

- **Owner:** claude lane
- **Branch:** `task/ledge`
- **Board area:** `player/movement`
- **Status:** open

## Goal

Replace the placeholder ledge rules in `ddsim` with the original's rules,
traced in the exe:
- when a jump or fall grabs a ledge (geometry tests);
- where the hands and feet end up;
- how the hang and the climb move the player.

If time allows, also trace the turn rate (the original turns over time)
and the run and sprint stop actions. Read `docs/MOVEMENT.md` first: it
lists every current rule and its source.

## Evidence

- `cPlCliffHangBegin::vf05` at 0x00AD9180 (526 decompiled lines of
  geometry tests) and `vf06` 0x00ADA980, `vf07` 0x00ADD510,
  `vf08` 0x00ADCB50, `vf12` 0x00ADBDE0.
- `cPlCliffHang::vf05` 0x00ADCC50, `vf06` 0x00ADCDD0.
- `cPlCliffHangClimb::vf05` 0x00ADD670, `vf06` 0x00ADDC00.
- Who enters `cPlCliffHangBegin` from the jump and fall actions: find
  the transition (xrefs to its DTI or its action id).
- Collision queries used by those tests: identify them, and model what
  you need on `ddsim::Ground`. It already has `height` and `wall`; add a
  ray or capsule query if the original uses one, and implement it in
  `ddworld::Terrain`.
- **Turn rate:** `cPlActRun::vf06` (0x00ACA5D0) builds the velocity from
  the action's direction field (+0x60) through 0x012F0906 / 0x012F0A55.
  Find where that direction approaches the input direction. The
  recording shows the velocity direction turning about 70°/s.
- **Stops:** `cPlActRunEnd::vf05` 0x00ACB570 and `vf06` 0x00ACB6C0; find
  the sprint equivalent. Recorded: after a sprint, a 31-frame motion with
  speed decaying 22.5 → 4 cm/frame over about 0.6 s. After a run, a
  35-frame motion at about 45 cm/s.
- **Motion ids** are `bank << 8 | slot`; bank 0 is `m0000_co`
  (ANIMATION.md).

## Scope

- `crates/ddsim/**`
- `crates/ddworld/src/lib.rs` (the `Ground` impl and new queries only)
- `docs/MOVEMENT.md`
- **Not** `crates/ddview/**`. If you need new motions loaded (the viewer's
  `USED_MOTIONS` in `play.rs`), list their ids in Results; head adds them
  at merge.

## Deliverables

- Translated rules, each marked with its source
  (`// Translated from 0x00AD9180 (DDDA.exe 2017-12-13 Steam)`), with
  regression tests in `ddsim`: grab heights accepted and refused, the hang
  position, the climb end position.
- MOVEMENT.md updated: the placeholders replaced, or listed as still open
  with the reason.

## Done when

`cargo test -p ddsim -p ddworld` passes, and every ledge rule in
MOVEMENT.md cites an exe address or is marked placeholder with the
reason. Fill in Results and report to `head`.

## Results

(filled in by the agent)
