# S41 standalone camera setting cache

Head #6571 approved a mutable CameraConfig resource for the repeated
standalone camera setting read and future S42 settings updates. Root later
approved keeping this source work separate while profiling was queued.
Branch `step/s41-camera-settings`, worktree
`E:/ddda-rs-wt/s41-camera-settings`, uses reviewed committed main `959f6a9`.
No profiler/renderer experiment source or frozen pin is changed here.

## Evidence and scope

Before this change, `play::step` initializes its camera projection once,
then returns for opening/network sessions. The later standalone loop calls
`config_camera_fov()` once per FollowCam every rendered frame, reopening
the player's config.ini. The opening path does not execute those repeated
reads, so this is **not an opening-frame bottleneck or measured gain**
(source correction #6609, acknowledged by head/root).

`main::args()` already resolves the same CameraFov setting once and applies
the explicit `--camera-fov` override. CameraConfig is initialized from that
resolved Args resource; no new file read or fallback table is introduced.
The old duplicate config reader in play.rs is removed. A camera SystemParam
groups projection/config to preserve Bevy's existing maximum argument count
without changing the PawnOrderDevices shared by opening/net systems.

The standalone ordinary/aiming lenses use the cached degree offset. A
resource change also refreshes the initial follow-camera projection;
opening/cutscene systems retain their subsequent projection authority.
This gives S42 an explicit mutable resource to update instead of editing
the game settings file. All FollowCam views use the shared display setting,
while unrelated and orthographic projections are preserved.

## Results (source complete; acceptance pending)

- Main hook `44b3f0a` adds one `.init_resource::<play::CameraConfig>()`
  after Args insertion; its exact main.rs claim was released immediately.
- Regression exercises two FollowCam perspective views, an unrelated
  perspective view, and an orthographic follow view through two resource
  values, checks FOV/recorded near, preserves far/aspect/unrelated lens,
  and checks the aiming native-lens offset.
- `git diff --check` passes. **Full viewer suite and release build remain
  pending** the normal shared M5 queue; no compile/test/visual PASS or
  measured FPS gain is claimed for this source yet.
- Existing CLI CameraFov overrides are now consistently used by the
  standalone per-frame lens, rather than overwritten by another config
  read. No simulation, LOD, content or game-install changes are made.

Next: full viewer suite/release after the higher-priority named-system
profiler acceptance. Root/head fold this result into MILESTONES when
accepted; the actual opening bottleneck and 5.5 ms S41 gate remain open.
