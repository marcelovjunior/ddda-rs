# S27: The bow, part 3: in the viewer

- **Branch:** `step/s27-bow-view` (after S26)
- **Files:** `crates/ddview/src/play.rs` (inputs, motions of bank 9),
  `main.rs` key binding, the follow camera's aim mode if traced, a shell
  drawing like the staff's
- **Kind:** one feature

## Do

Bind the bow-mode toggle and the shot to keys (config.ini names, as the
other bindings), play the bow motions (bank 9, `m0006_at`) on the Strider,
draw the arrows, and switch the camera to the original's aim view if the
bow action changes it (trace `uAimCheck` and the camera mode it requests).

## Done when

`--play --kit strider --goblin` lets a player draw the bow, aim and shoot
the goblin; a capture shows the aim pose and an arrow in flight; ddview
tests pass.

## Results — review (codex-62)

The stationary fresh-kit route is implemented and tested. Four focused
bow tests pass (30.26 s compile, 2.32 s tests), including installed data -> configured keys ->
real posed bow model joint zero -> TPS camera ray -> RNG -> arrow ->
ordinary damage at 10 m. Native source and asset dumps remain private.

- Keys read `STG_TOGGLE_BOW_MODE`, `STG_SUB_WEP`, `STG_ATTACK_0` from
  the player's config, names registered by `0x0040A6C0`. P2's Numpad
  minus/divide are independent viewer bindings. Mode/toggle edges are
  latched across render frames. Missing or unsupported config codes
  remain unbound.
- `0x00B1AC90` calls `0x00B1E3A0`, which selects runtime `cameraNo`;
  `0x00421870` requests that index from `sCamera+0xD00`.
  `0x00421010` loads `param/cam/pl/pl06` for secondary categories
  10–12. The installed basic bow selects TPS slot zero. Desired eye
  and target follow `cCamImpTps::vf11`, `0x007CDF70`; distance, height,
  shoulder offset, pitch bounds and FOV are read from the install.
  `uAimCheck::vf91`, `0x00BC1E60`, follows this active camera. The
  non-lock camera ray branch of `0x00B1D010` advances its start to the
  actual bow depth and uses runtime `checkLength` as its end range.
- Body bank 9 is loaded; an optional independent upper Playback layer
  overlays only authored channels after the lower layer, using
  `0x01061DD0` channel priority. Draw/shot interpolate over 3 frames;
  wait/release over 10 (`0x00B16FE0`, `0x00B1C7D0`, `0x00B1CAC0`,
  `0x00B17940`). No skeleton half is invented. Bow posing combines
  both motion clocks and mounts its separately animated model on
  actual drawn joint `0x99`, normalized as `0x00859B10/0x00859BB0`.
- The fresh Strider kit's starting bow is shown separately so its
  string/limb motion runs; duplicate rigid bow parts are removed.
  Both body layer frames contribute to the authoritative mounted pose.
  The simulation pose excludes presentation interpolation and body morphs.
  Per-seat host RNG uses the already verified original generator;
  per-seat seeds are a port session choice, unlike the original's
  shared global stream. Fresh kits have no equipped augments.
- Arrow flight shows an explicitly provisional segment/tip marker
  from authoritative previous/current endpoints and installed collision
  radius. This is **not** the original bow EFL particle effect; the
  installed basic arrow has `isUseModel=false` and uses that effect.
- `DDVIEW_BOW_CAPTURE=1` is an explicit screenshot fixture: it chooses
  the camera orbit toward the first diagnostic `--goblin`, feeds
  ordinary bow/light input and holds the real first-flight snapshot.
  It does not force hits, death, HP, projectile data or motion completion.

Open: full native joint masks/frozen-channel cache semantics, morphed
actor-height and bow attachments, upper-body aim constraints, camera
entry/exit easing and recoil, lock-on/actor sensor acquisition, moving
and air shots, charged/special arrows, terrain projectile death, saved
secondary valuation/augments and LAN bow input/projectile replication.
This does not complete all bow skills or full original visual fidelity.

Validation after merging reviewed main (including S33 and S37):

- Complete release viewer suite: **69 passed, 2 existing ignored**;
  compile 21.39 s, tests 12.10 s. The new upper-layer regression checks
  independent frozen rotation and preservation of lower position/scale.
- Normal release build: **passed in 50.50 s**. Existing warnings concern
  `opening_world`'s Args visibility and `audio::forget_actor`; no new
  feature warnings remain. No other target directory was created.
- Hidden bounded capture used the copied private binary with ordinary
  `--play --kit strider --goblin --players 1 --unattended --size 1600x900
  --shot-delay 15` and the explicit `DDVIEW_BOW_CAPTURE` fixture above.
  Owned PID 27672 exited **0** before its 60 s watchdog. The image at
  private `E:/ddda-re/notes/steps/S27/bow-flight.png` was inspected: TPS
  shoulder framing, the bank 9 body pose, animated bow and authoritative
  first-flight segment/tip marker are visible. It is a selected diagnostic
  orbit and held real snapshot, not an unattended ordinary play route.
- **No original-game comparison was performed.** Native particle visuals,
  aim IK and camera transitions are explicitly outside this checkpoint.
  All owned build/GPU processes exited and their leases were released.

Next action: lane review of the branch and capture, then source-backed
aim constraints and native arrow particles in a separate visual brief.
