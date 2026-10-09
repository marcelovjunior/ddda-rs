# cutscene-visuals: camera FOV and near plane, clipping, eyebrows, z-fighting

- **Owner:** codex lane (PRIORITY 1, with viewer-controls)
- **Branch:** `task/cutscene-visuals`
- **Board area:** `render/events`
- **Status:** open

## The maintainer's report (watching a Codex agent play cutscenes)

1. The camera looks too close or too forward.
2. It clips into 3D models.
3. Character eyebrows do not render.
4. Z-fighting.

## Head's findings in `crates/ddview/src/event.rs`

- **FOV:** the event camera uses the camera list's `fov_degrees`
  directly (around line 628) and omits the config offset. The game
  renders with fov + `CameraFov` from `config.ini` [GRAPHICS], 15 on this
  machine (claude-3: `uCameraBase::vf21` 0x007D3E00, global
  `*0x018D1D20+0x24`; `--ref` and `--play` already add it, see
  `stage.rs` / `main.rs`). That alone explains "too close". Check whether
  the offset applies to event cameras in the exe (the event camera class
  may share `uCameraBase`).
- **Near plane:** event cameras use `near: 0.05` (5 cm; lines 372, 554).
  `--play` uses the original's 96 cm (play-visuals). Find the event
  camera's near plane in the exe or data (camera list, `uEventCamera`). A
  5 cm near plane also wrecks depth precision, a likely contributor to
  the z-fighting here.
- **Eyebrows:** `docs/CHARACTERS.md` (character-morphs) notes the brows
  showing as a pale band because their colour isn't applied. They likely
  use an alpha-tested or blended texture plus a colour from the edit
  palette (`etc\editPattern`, 0x00780fa0). Render them as the material
  says (alpha test reference, blend state) and apply the colour.
- **Z-fighting:** shared with `viewer-controls`; coordinate with
  `codex-17` so one fix covers stage overlays in every mode.

## Scope

- `crates/ddview/src/event.rs`, `character.rs` (eyebrow material)
- `crates/ddlook/**`
- `crates/mtcamera/**`
- `docs/EVENTS.md`, `docs/CHARACTERS.md`

## Done when

- At st200 ev10's cut frames, the framing matches the authored shots: fov
  with the offset and the correct near plane, no clipping.
- Eyebrows are visible and coloured.
- No z-fighting.
- Screenshots described. Put a timeout on every GUI run.
- Fill in Results and report.

## Results

Implemented, with visual acceptance still open:

- `CameraView` carries near/far/zoom. Traced uOfsMotionCamera's actual
  vtable dispatch to 0x00faa920, which uses authored FOV + zoom, not the
  gameplay CameraFov option in 0x007d3e00. Constructor 0x01128740 gives
  near 16 cm / far 4,000,000 cm. st200 ev10 has no overriding SDL tracks.
  Applying +15 or 96 cm here would contradict the identified camera class.
- Brow RGB/UV/texture controls follow 0x00843940, NPC record conversion
  0x008442d0 and save conversion 0x00780fa0 / 0x00cb5740. Data/palettes
  remain runtime reads; material blend/alpha state remains authored.
- Removed the unverified minimum-two overlay bias from viewer-controls;
  only the imported rasterizer bias's reverse-Z sign conversion remains.
- Scope extensions for stage depth, ddstory fields/tests and controls
  Results approved on board #1385/#1401/#1410.

Validation so far: release build succeeds; 21 ddlook tests, 9 mtcamera
tests, 3 ddstory event tests and 17 ddview tests pass. Additional
installed-material regression verifies the brow group, NPC mask selection,
alpha blending and zero authored depth bias. Release target reused from
viewer-controls. Unattended safeguards 41c9fe4 and yaw af425c8 merged
before GUI runs. Standalone event has no cursor-grab or focus-pause input.

45-second-bounded CLI captures at event row22 frames800/1200 exited zero:
OS TEMP `cutscene800.png`, `cutscene1200.png` (960x540, shot-delay12).
Both show the authored two-person closeup and subtitles. Quina's brows
remain difficult to see; elder has a hole around the ear. No wall speckle
is obvious in these stills, but temporal z-fighting is unverified. No
fresh original-game comparison. These are not full visual acceptance.

Remaining: trace the facial geometry/material defect and pending camera
joint-displacement adjustment, then compare cut frames against original.
The group5 common-pass face-detail meshes have 176 vertices each with
more than four weights; viewer skinning truncates them to four. This is a
candidate cause, not a proven diagnosis (Quina's separate group6 brow
mesh has no such vertices). Normal-map replacement is not implemented.

Projection checkpoint `6726a57` is committed separately. Agent18's
`432ad60` is integrated: opening captures and restores the full gameplay
projection with a regression, rather than retaining event near/far.

Bounded diagnostic captures (also exit0, 45-second limit) isolate:

- `cutscene1200-nocull.png`: temporarily disabling all character culling
  exposes the elder's ear and brow, but also other rear surfaces. Original
  MRL specifies Back; the diagnostic override was removed.
- `cutscene1200-noface.png`: disabling face morphs leaves the ear/brow
  defect. The diagnostic was removed. Both actors have unique joint IDs,
  positive face scales and rest matrix/TRS error below 0.00000006.
- Rest mesh winding agrees with vertex normals for all 416 common face
  detail triangles and 368 Quina brow triangles. Bevy's transparent pass
  loads shared depth after opaque/alpha-mask passes; it uses GreaterEqual
  for reverse Z and disables transparent depth writes.
- The omitted NPC texture override at 0x00bc1650 was investigated by
  reading table 0x0163c1a8 from the installed executable. Both relevant rows
  (male set0x8a, female0x89) specify no replacements, so it cannot explain
  this scene. The experimental reader/dependency was removed.

Review follow-up: malformed eyebrow preset rows now return an error,
rather than shifting later preset indices through filtering (regression).
Fixed preset offsets were verified against instructions 0x00781b13..93.

### Authored depth-state follow-up (head #1660)

The material pipeline now keys on the MRL depth-test/write/comparison
bits, independently of blending. `DSZTestWrite` retains writes for
`BSBlendAlpha` (hair104); `DSZTest` leaves writes off (brow overlay).
LessEqual becomes GreaterEqual in reverse Z. Alpha testing on blended
materials also keeps the authored comparison/reference. Existing opaque
mask handling is unchanged. Unit coverage checks reversed comparisons
and reads the installed named DSZTest/DSZTestWrite states.

`cargo test --release -p ddview`: 17 pass; release build passes. Timed
frame800 and frame1200 runs exit0 without shader warnings/errors after
correcting a WGSL reserved identifier caught by the first GPU run.
Private `cutscene{800,1200}-depth.png` compares against the earlier
same-frame captures: hair compositing changes (79,523 changed pixels at
frame1200), but the ear hole and unclear brows remain. This state fix
does not meet the visual acceptance gate or prove original parity.

Further eliminated causes: actual post-propagation blended skin matrices
all have positive determinants. Quina's brow has 315/368 camera-facing
triangles under both four- and eight-weight skinning, with zero position
difference. Face detail camera-facing counts also stay equal; maximum
four/eight displacement is 0.055 cm for Quina and 0.013 cm for the elder.
Both brow layouts map UV_Primary to the first TexCoord, as the renderer
uses. Hair104's six MRL copies and hair105's nine copies are identical.
The primitive sort flag/alpha priority and original per-mesh sort center
remain candidates; no unsupported sort bias or culling override added.

### Transparent sort follow-up (head #1704)

The candidate ordering mismatch is now corrected for character meshes:
the original uses ancestor sphere center + current model radius when
primitive sort bit2 is clear, or the first weight-bound bone-space center
when set. It then clamps/truncates centimetre depth and appends authored
alpha priority. Sources: 0x00ea3130 -> 0x00ea1f40, 0x00ea3d00,
0x00ea5320 and 0x00ffc350. Details in CHARACTERS.md. Runtime model bias
+0x154 uses the constructor's zero default (0x00ea3490); runtime overrides
remain unimported. Review also guards zero-count weight-bound records
from borrowing the next primitive's bound.

`cargo test --release -p ddview`: 21 pass after current main integration;
release build passes. The new regression covers clamp, truncation,
priority/depth precedence and f32 packed-key precision loss. Private
`cutscene{800,1200}-sort.png` runs both exit0 under 45-second timeouts,
using screenshot unattended mode with clean logs. Compared with the
depth checkpoint captures, Quina's hair compositing improves visibly.
The elder's ear hole and unclear brows persist in the new captures.
No new original-game comparison or temporal z-fight acceptance yet.
