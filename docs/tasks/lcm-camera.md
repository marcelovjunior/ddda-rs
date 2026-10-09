# lcm-camera: the cutscene camera lists

- **Owner:** codex-16 (Codex lane)
- **Branch:** `task/lcm-camera`
- **Board area:** `formats/camera`
- **Status:** review

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

Implemented `mtcamera`: bounded borrowed LCM v3 reader; slot-preserving
nullable camera table; eye/target/quaternion/FOV samples; pure fractional
sampler and quaternion-derived base pose. Layout from loader 0x01127660,
writer 0x01127710; interpolation/pose from 0x00fab460, with x87 truncation
checked in disassembly. User data traced through 0x01128120/0x01127c30.
All translation helpers carry source comments and regression tests.

Validated 2026-10-09: **109 paths / 195 archive copies**, 49,920 slots,
1,411 non-null cameras, **195,024 frames**, no parse failures. All numeric
samples finite and quaternions unit within squared-norm error 0.0001.
Five synthetic tests and two install tests pass (`cargo test -p mtcamera
-- --nocapture`). Synthetic cases include every fixture truncation,
out-of-bounds spans/counts, nulls/zero frames, fractional sampling,
clamp/repeat boundaries, antipodal quaternion sign and pose reconstruction.

The actual `event\st200\ev10\st200ev10` SDL resource key resolves
`event\st200\ev10\list\camera\camera`. Cuts at 0,460,670,790,910,1030,
1150,1270 select slots 0..7 (256 slots total); frame lengths
460,210,120,120,120,120,120,433 cover the intervals. Corresponding
`mFrame` keys count from zero. See CAMERA.md for format, addresses and
sampling limits.

No original playback comparison; no event player. Block +0x18 metadata
is preserved without a claimed meaning. Actor/world adjustments after
base sampling, projection and playback/cut advancement remain outside
the reader. Next: event-player integration and trace the st200 ev10
actor-adjusted camera before a recorded pose comparison. No game or
private research bytes committed; no game/process writes.
