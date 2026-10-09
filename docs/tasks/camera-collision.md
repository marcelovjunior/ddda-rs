# camera-collision: The follow camera's collision, as the original

- **Owner:** codex lane
- **Branch:** `task/camera-collision`
- **Board area:** `render/camera`
- **Status:** partial (head)

## Goal

The maintainer reports the camera clipping into walls and mountains. The
viewer's follow camera (`play::follow_camera`) pulls the eye in along a
ray from the target, `CAMERA_MARGIN` (5 cm, from `mMargin`) off the first
hit. That margin's use is inferred and the original's camera collision
is not traced.

Trace `uCameraGame` / the player camera's collision in the exe:
- whether it is a ray, a sphere or a capsule cast, and its radius;
- what it tests against (stage collision, scenery, characters, filters);
- how fast the eye pulls in and recovers, and its minimum distance;
- the parameters in `param\cam\pl\*` (`mMargin` and friends).

Then implement them in `play::follow_camera`, used by `--play`, the
opening and LAN sessions. Viewer collision comes from `PlayState.terrain`,
which since head's commit is filled in the opening too.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may use the oracle, with the safety rules in `docs/CODEX.md`.

## Scope

- `crates/ddview/src/play.rs` (`follow_camera` and its constants; claim)
- `crates/mtcamera/**`
- `docs/CAMERA.md`
- this brief's Results

## Done when

In Cassardis and the field, the camera no longer shows the inside of
walls or terrain. Its pull-in matches a recording of the original next to a
wall: eye distance against time, captured with the oracle. Fill in Results
and report to `head`.

## Results

head, 2026-10-09: the collision test is translated from 0x007de570 (called
by 0x007e1400 from uCameraGame vf23 and vf13):
1. A ray from the target to the eye; on a hit the eye moves there.
2. A sphere one near-plane distance in front of the eye, radius
   `1.3 x mNearPlane + 10 x (1 - |cos|)` (`uCamera.mNearPlane` is +0x34).
   If it overlaps collision, it is swept toward the target and the eye
   goes one near plane behind its stop.

This is in `play::follow_camera`, with tests for the ray and the sphere
cases.

Still open for the lane:
- the hit-attribute bit 0x10 filter (`0x007c6f90`);
- the sweep `0x010fe5e0`, which is bisected here;
- what else 0x007e1400 does: the `+0xb98` offset push, and the `0xfaf5b0`
  check around the target model;
- any smoothing of the pull-in and recovery in the camera's update;
- an oracle comparison next to a wall.

codex-30, 2026-10-09, followup on `task/camera-near` (head priority #2382):
traced **0x010fdea0**: the sphere starts at target and moves outward to
the near-plane centre. The bisection interpretation above was reversed.
The replacement uses existing `ddsim` sphere-contact geometry and
**0x010fd710**'s 0.1 cm contact response, retaining the explicit geometric
replacement limitation. The bit0x10 material filter is applied to the
nearest ray/sweep result, not to candidate triangles. Nonfield stages now
retain SBC metadata. See camera-near Results and CAMERA.md for validation
and remaining target-model/push/smoothing and original-comparison work.
