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

(filled in by the agent)
