# camera-collision: The follow camera's collision, as the original

- **Owner:** codex lane
- **Branch:** `task/camera-collision`
- **Board area:** `render/camera`
- **Status:** open

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

(filled in by the agent)
