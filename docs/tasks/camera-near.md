# camera-near: What the game camera does close to walls and the Arisen

- **Owner:** codex lane
- **Branch:** `task/camera-near`
- **Board area:** `play/camera-near`
- **Status:** open

## Goal

Playtest (maintainer, 2026-10-09): when the follow camera is pushed in by
a wall, the viewer shows the inside of models and cuts the Arisen open.
Head implemented the original's collision rule (0x007de570 from
0x007e1400, `play::follow_camera`): a ray target->eye, then a sphere
of radius 1.3 x near + 10 x (1 - |cos|), one near plane ahead of the
eye, swept toward the target. The near plane is the recorded 96 cm
(`CAMERA_NEAR`). That keeps the near plane out of *collision*, but not
out of drawn geometry that has no collision near it, or out of the
Arisen's body.

Trace and implement what the original does here:
1. Whether the near plane changes: who writes the current camera's
   +0x34 (`*(DAT_018D0B6C + 0xCE0) + 0x34`) and when (indoors, collided,
   by camera parameter files). Check with the oracle in a tight spot
   (Cassardis, the porch outside the class-selection room).
2. The Arisen fading out when the camera comes close (the original
   fades the player model; find the rule: distance, alpha, which
   models, pawns too).
3. The 0x007e1400 branch at +0xB98 (the eye moved toward the target by a
   signed distance when the pitch passes conditions) and the stage
   attribute bit 0x10 hits the original ignores (0x007c6f90).
4. Whether the camera's collision includes layout objects' collision
   (houses, pillars placed as objects), not only the stage SBC. Head's
   probe (`--probe`, F9) now logs each hit's distance to collision.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs use `--unattended`.

## Scope

`crates/ddview/src/play.rs` (follow_camera, camera_projection),
`crates/ddview/src/character.rs` (fade), `docs/CAMERA.md`, this brief's
Results.

## Done when

The maintainer's spots (porch pillar outside the class-selection room)
no longer show model insides, and the Arisen fades as in the original.
Fill in Results and report to `head`.

## Results

(filled in by the agent)
