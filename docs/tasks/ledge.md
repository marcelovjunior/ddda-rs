# ledge: ledge grab and climb, turn rate, stop motions

- **Owner:** claude lane
- **Branch:** `task/ledge`
- **Board area:** `player/movement`
- **Status:** review

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

Branch `task/ledge` (claude-1). `cargo test -p ddsim -p ddworld`: 24 + 4
tests pass. Details and addresses: docs/MOVEMENT.md (Ledges, Turning,
Stopping, Assumptions).

**Found in the exe.**
- Action ids: uPlayer +0x2DD4 is the current action, +0x2DD8 the requested
  one (`uPlayerBase::vf132` 0x00B540B0 = set action, vtable +0x210). Ids
  index a 0x44-stride table of DTIs at 0x01866C48 (unpacked image):
  0x0C `cPlActDashJumpLand`, 0x0D `cPlActFall`, 0x0F `cPlActEvasion`,
  0x34 `cPlCliffHangBegin`, 0x35 `cPlCliffHang`, 0x36 `cPlCliffHangClimb`.
- Transition: `uPlayerBase::vf136` (0x00B6D510, called every frame from
  vf90 0x00B61A20) runs the ledge test `0x00B6FA00` and sets action 0x34.
  `0x00B6FD20` accepts only actions 0x0C / 0x0D / 0x0F (normal mode) and
  needs stick input, and a fall must have dropped > 100 cm. **A normal or
  sprint jump in the air never grabs.** This contradicts the old
  placeholder (and possibly expectations): worth confirming live (watch
  +0x2DD4 while jumping at a ledge) once claude-4's oracle is up.
- Geometry: 0x00B6FD20 (probe), 0x00B6FA00 (three probes + ground under
  the ledge), 0x00ADBEB0 (foot point, free/braced hang), 0x00AD9180
  (hands / body / feet), 0x00ADA980 (grab phases, rise), 0x00ADCDD0 (hang
  controls, let go), 0x00ADD670 / 0x00ADDC00 (climb). All constants are
  `m_pl000_00` ids 0x5A-0x5F, 0x17C-0x17D, 0x189-0x18A, 0x190-0x191 or
  literals in those functions.
- Turn rate and stops: traced in part (MOVEMENT.md); not implemented
  (the sprint stop's speed is a motion curve that `mtlmt` does not read).

**Implemented and tested** (`crates/ddsim/src/ledge.rs`, `lib.rs`):
`Ground::ray` / `Ground::sphere` (+ `RayHit`), implemented in
`ddworld::Terrain` (Moller-Trumbore, both faces; tests); `LedgeParam`
(+ `from_adjust`); detection, foot point, hang pose, rise, hold test,
climb target and end; states `SprintJumpLand`, `LedgeGrab { braced,
phase }`, `LedgeHang { braced }`, `LedgeClimb { braced }`; fall distance
(`Player::drop_from`). Tests: tops accepted (60, 150, 220) and refused
(40, 230; 90 when standing; reach; blocked top); hang position (free: feet
2 steps out, 210 below the edge; braced: feet 100 below the hands);
climb end (10 in, 5 up); fall -> grab -> hang -> climb -> stand; no grab
without stick or drop; normal jump never grabs; sprint jump -> landing ->
braced grab -> climb; pulling back lets go at 2 cm/frame; vanished ledge.
Not compared against the original (no recording of a ledge).

**Still open** (placeholders, labelled in code): hit classification by
normal (engine flags not traced); progress curves 9/10 (linear here); IK
body placement (feet go straight to the foot point; facing snaps); hit
attribute check 0x007C6F90; size scale; early climb from the grab
(+0x361C); `cPlActEvasion`; the writers of +0x32CC / +0x32C4 / +0x2584.

**For the viewer (head, at merge; not done here).**
- Load motions 24 (0x18), 45 (0x2D), 46 (0x2E), 49 (0x31), 50 (0x32) in
  `USED_MOTIONS` (47 and 48 are already there; 44 = 0x2C is not used).
- Set `sim.ledge_param = ddsim::LedgeParam::from_adjust(&adjust)` next to
  `move_param`.
- `--play`: ledges are now taken by sprint-jumping at a wall (stick held)
  or by falling past one with the stick held; normal jumps no longer grab.

**Next step.** Read the LMT motion curves (`0x010571C0` reads curve k of
the playing motion) so the grab, climb and sprint stop use the original's
progress; then verify the jump/no-grab rule live.
