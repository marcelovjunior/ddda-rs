# camera-feel: The follow camera rotates when it bumps into things

- **Owner:** codex lane
- **Branch:** `task/camera-feel`
- **Board area:** `play/camera-feel`
- **Status:** open (high: the maintainer's worst complaint after playtesting)

## Goal

Maintainer, 2026-10-09 playtest after the camera-near merges: "when
bumping the camera, the camera rotates instead of just moving in a
transform action. it rotates and it's really bad. also it feels like it
clips on too much stuff." In the original, pushing the camera into a
wall slides it in along its line; the view direction does not swing.

Head's lead (not verified): `play::follow_camera` sweeps a sphere of
radius 1.3 x near = 125 cm from the **target** (140 cm above the feet)
toward the near-plane centre (`camera_sphere_cast`, 0x010FDEA0 /
0x010FD710). A 125 cm sphere at the target already overlaps any wall
within 1.25 m of the Arisen and any ceiling below 2.65 m (Cassardis
rooms, porches, alleys). The initial-overlap branch then pushes the stop
along the contact normal by up to 125 cm, off the eye-target line, and
the eye (stop - toward x near) leaves the line, so `look_at(target)`
swings the view. That is also "clips on too much stuff": the camera reacts
to geometry the original ignores.

Find what the original does instead, then match it:
- the collision query's filter for the camera sweep (sCollision::Param
  ctor arguments and mask 0x4081D01C in 0x007E1400, FUN_01163EB0's
  options): does it skip contacts already overlapping at the start,
  back faces, or some attributes;
- which point is really the sweep's start and which its end (re-check
  0x007DE570's ESI/EDI against 0x010FE5E0's argument order);
- the actual near plane and target height while walking in Cassardis.
Check it live: stand the original's Arisen next to a wall and under a
low roof in Cassardis, swing the camera into the wall, and record eye and
target (oracle reads, no save writes). Compare with the viewer at the same
spot (`--probe` logs the eye).

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks follow the safety rules in `docs/CODEX.md`. Test runs use
`--unattended`.

## Scope

`crates/ddview/src/play.rs` (follow_camera and helpers), `crates/ddsim`
sweep helpers if needed, `crates/mtcamera`, `docs/CAMERA.md`, this
brief's Results.

## Done when

Swinging the camera into walls and under low roofs in Cassardis slides it
in along its line, as in the original, with eye/target matched against a
live capture at one wall spot and one low-roof spot. Fill in Results and
report to `head`.

## Results

(filled in by the agent)
