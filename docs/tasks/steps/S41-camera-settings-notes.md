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

## Results (full headless acceptance passed)

- Main hook `44b3f0a` adds one `.init_resource::<play::CameraConfig>()`
  after Args insertion; its exact main.rs claim was released immediately.
- Regression exercises two FollowCam perspective views, an unrelated
  perspective view, and an orthographic follow view through two resource
  values, checks FOV/recorded near, preserves far/aspect/unrelated lens,
  and checks the aiming native-lens offset.
- Parent independently reviewed source `203ad96` and startup ordering.
  Under normal M5 lease after codex-71 actual release #6927, full viewer
  session 20362 **actually exited 0: 91 passed / 2 existing ignored**,
  fresh compile 29.69 s, tests 16.64 s. The new
  `camera_settings_update_all_follow_views_and_preserve_other_lenses`
  regression explicitly executed and passed. Release session 6423
  **actually exited 0 / 40.71 s**; only existing private-interface/audio
  warnings remain. No clean or new target was used.
- Private immutable pin
  `E:/ddda-re/notes/steps/s41/ddview-camera-cache-203ad96.exe`, SHA-256
  `9CF6D3FBF38AB2B2FBCD7A4BA0D58AB5AE983F313A6CF0ADC6D9648F121E8F97`,
  was copied before actual M5 release #6976. All owned Cargo/rustc exited.
  No GPU lease or viewer capture was taken; no visual/original comparison
  or measured FPS gain is claimed. Full default viewer suites are for
  reviewed base `959f6a9`, not the older profiler branch.
- Existing CLI CameraFov overrides are now consistently used by the
  standalone per-frame lens, rather than overwritten by another config
  read. No simulation, LOD, content or game-install changes are made.

Next: S42 may integrate source `44b3f0a` + `203ad96` under normal claims,
then test its live settings hooks. Root/head fold this result into
MILESTONES when accepted; the actual opening bottleneck and 5.5 ms S41
gate remain open.
