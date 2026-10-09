# lcm-camera: the cutscene camera lists

- **Owner:** codex lane
- **Branch:** `task/lcm-camera`
- **Board area:** `formats/camera`
- **Status:** open

## Goal

A reader for the camera-list resources that cutscene timelines reference
(proposal 5 in `docs/QUESTS.md`; see `docs/EVENTS.md` for how
`st200 ev10` cuts its camera at frames 0, 460, 670 and so on). It is a
prerequisite for an event player.

## Scope

- `crates/mtcamera/**` (new)
- `docs/CAMERA.md` (new)
- `crates/ddtool/src/<name>.rs`, if needed

## Done when

- Every camera-list instance in the install parses with bounds checks.
- The cameras of event `st200 ev10` resolve from its timeline's
  references.
- CAMERA.md documents the keys (position, target, fov, interpolation)
  with loader addresses.
- Fill in Results and report.

## Results

(filled in by the agent)
