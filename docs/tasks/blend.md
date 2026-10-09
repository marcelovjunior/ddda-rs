# blend: motion blending and playback rate

- **Owner:** claude lane
- **Branch:** `task/blend`
- **Board area:** `anim/blend`
- **Status:** review

## Goal

When the player's action changes (idle → run → sprint → jump → land),
the original blends from the old pose to the new motion instead of
snapping. Find how in the exe (blend length, curve, per-joint method) and
reproduce it in the viewer. While there, find where each action's
playback rate is set: locomotion motions were recorded advancing 34.5
frames/s against 30 for others (`ddsim::LOCOMOTION_RATE` = 1.15,
measured, not traced).

## Evidence

- Motion requests from player actions:
  - `cPlActWalk::vf05` (0x00AC9FB0) and `cPlActRun::vf05` (0x00ACA390)
    call `0x00B80260(motion, x)` with x = 20.0, 18.0, 17.0 or 0.0.
    Likely blend frames: confirm.
  - `cPlActDashBegin::vf05` (0x00ACA850) calls uPlayer vfunc +0x90 with
    `(motion, 1, 1, -1.0f, 0, 1.0f)`.
  - `cPlActDash::vf06` uses `(0x4F or 0x14, 0, 1, -1.0f, 1.0f)`.
  - Trace both down to the motion player that mixes poses.
- Playback rate: `cPlActWalk::vf06` and `cPlActRun::vf06` write the
  player's motion object (+0xF0C) at +0x3DC, from uPlayer +0x333C /
  +0x3340. The motion frame counter is +0x3CC on the same object, and
  +0x3D4 holds the motion's period.
- MT Framework motion classes in `dti-labels.csv`: search for `Motion`,
  `cMot`, `uModel`.
- Viewer today (`crates/ddview/src/character.rs`): `Playback` samples one
  motion at `hold_frame`; `play.rs` swaps `pb.motion` when the state's
  motion id changes. The simulation steps at 120 Hz and the viewer samples
  at the fractional frame.

## Scope

- `crates/ddview/src/character.rs`, `crates/ddview/src/play.rs`
- `crates/mtlmt/**`, if sampling needs changes
- `docs/ANIMATION.md`
- **Not** `crates/ddsim/**`: claude-1 (ledge) owns it. Blending is
  presentation. If the simulation must expose something (e.g. the
  previous motion), ask claude-1 on the board, or post the exact change
  in Results.

## Deliverables

- The blend rules, translated with source comments where they come from
  the exe, and tests for the blend math (weights over time, quaternion
  interpolation).
- The traced playback rates per action, or confirmation of 1.15 with its
  source.
- ANIMATION.md: how the original blends, and what we do.
- A visual check: `ddview --demo` screenshots (e.g. `--shot`) mid-
  transition. Keep them in your scratch space; do not commit images.

## Done when

`cargo test -p mtlmt` and `cargo build --release -p ddview` pass, the
`--demo` transitions no longer snap, and ANIMATION.md cites the exe.
Fill in Results and report to `head`.

## Results

Branch `task/blend` (claude-2). Static analysis of DDDA.exe 2017-12-13
Steam plus the install's data. Nothing compared with the running game.
Details in `docs/ANIMATION.md` ("Motion requests and blending").

**Found**
- The 20/18/17/0 floats are **start frames**, not blend lengths.
  `0x00B80260(motion, start)` phase-syncs between the locomotion motions
  listed at 0x018763D0. Otherwise it starts at the given frame, chosen by a
  foot test (`0x00AC5FB0`).
- Motion request chain: uPlayer vf+0x90 = `uPlayerBase::vf36` (0x00B800C0)
  `(motion, force, flags, interp frames, start frame, rate)` ->
  `uObjModel::vf36` (0x00886E70) -> `uModel::vf30` (0x0105B480).
- **Blend length.** Every player action we model passes -1.0, which means
  "the motion's default". The defaults are 256 floats per bank in an
  `rAdjustParam` stored at the same path as the motion list. 20 of 1,032
  motion-list paths have one (19 player banks and `e5600_at`). Values
  for `m0000_co`: idle 8; walk, run, sprint, fall and ledge 5; jump
  takeoff 3, airborne 0, landing 2. The exception is `cPlCliffHang`, which
  passes 0.
- **Blend method.** On a motion change the engine freezes each joint's
  current local pose (`0x01061DD0`) and starts the new motion at once.
  Each update, `mInterCount` -= `mDeltaTime`, and the layer rate does not
  apply (`0x01060730`). The frozen pose's weight is
  `ease(curve, remaining / total)`. `ease` (0x00D4EBF0) is a cubic Bezier
  with control values (p0, p1). Per joint (`0x01058A70`): nlerp on the
  shorter arc for rotation, lerp for translation and scale.
- **Curve.** The layer curve is copied from global 0x018D06A0, which is
  zero-initialised data with no writer in the exe. So (0, 0) and weight
  = t³. *Static only*: the next live check should read layer 0
  `mInterCurve` (uPlayer +0x3EC / +0x3F0).
- **Playback rate.** Walk sets layer `mSpeed` (+0x3DC) from uPlayer
  +0x333C, and only when +0x32C0 == 1; otherwise 1.0. Run sets it from
  +0x3340. Sprint never sets it, so it stays at the requested 1.0. That
  conflicts with the recorded 34.5 frames/s for sprint.
  - No direct store to +0x333C / +0x3340 exists.
  - `m_pl000_00` ids 0x1E4 / 0x1E5 = 1.15 match the measurement, but no
    reader was found.
  - So 1.15 is confirmed as a per-action rate field, but its source is
    not traced.
- The walk/run/sprint `vf06` also multiply velocity by uPlayer +0x29A8,
  and walk/run by the rate. Sent to claude-1 (#231). claude-1 listed it as
  open in MOVEMENT.md (#239).

**Built and tested**
- `mtlmt::blend`: `ease`, `Interpolation` (start/advance),
  `mix_rotation`, `mix_vector`, `default_frames`. Each has a translation
  comment and unit tests: weights over time, snap at 0, named curves,
  quaternion mix and sign, vector mix, table lookup.
  `cargo test -p mtlmt`: 8 passed.
- `ddview --play/--demo`:
  - `play.rs` loads each bank's twin table and calls
    `Playback::change_motion(motion, frames)` on every shown-motion change.
    The first motion does not blend; the ledge hang uses 0.
  - `character::animate` freezes the joints' local transforms, advances
    the fade by frame time × 30, and mixes after sampling.
  - `cargo build --release -p ddview` passes.
  - `--demo --players 2` debug log: run->jump 3 frames, jump->air 0,
    land 2, land->run 5, sprint start 5. Screenshots at 12.0, 12.85 and
    13.0 s show intact poses.
  - Single frames cannot show snapping. Not compared with the original.

**Not done / open**
- Start frames and phase sync (a `ddsim` change, which claude-1 owns).
  Until then `set_state` starts each motion at 0, and the blend hides the
  jump.
- Sprint rate: the exe gives 1.0 against the recorded 1.15.
- The writer of uPlayer +0x333C / +0x3340.
- The order inside one update of request, freeze, step and mix.
- Per-joint freeze masks: the viewer freezes all joints.
- docs/MOVEMENT.md still says "No animation blending". That is
  claude-1's file; it should now point to ANIMATION.md.

**Next step:** read layer 0 `mInterCurve` / `mInterCount` / +0x60 live
(claude-4's oracle) during a run->jump change. That confirms the t³ curve
and the 3-frame length.
