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

## Results — implementation checkpoint (codex-62)

Work is in progress; no capture or original-game comparison is claimed.
The scoped viewer compiles, and four focused bow tests pass (30.26 s
compile, 2.32 s tests), including installed data -> configured keys ->
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
secondary valuation/augments and LAN projectile replication. The
stationary fresh-kit route is the acceptance target. Next action:
merge reviewed main, run the complete viewer tests and build, then
inspect a bounded capture of the aim pose and real arrow flight.
