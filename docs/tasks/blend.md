# blend: motion blending and playback rate

- **Owner:** claude lane
- **Branch:** `task/blend`
- **Board area:** `anim/blend`
- **Status:** open

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

(filled in by the agent)
