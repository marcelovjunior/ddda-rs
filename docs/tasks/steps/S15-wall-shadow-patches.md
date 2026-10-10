# S15: The dark patches on Cassardis walls

- **Branch:** `step/s15-wall-shadows`
- **Files:** `crates/ddview/src/stage.rs`, `sky.rs`, `main.rs` shadow
  setup, `dd_material.wgsl`
- **Kind:** diagnose and fix one visible defect

## Context

docs/tasks/render-fidelity.md Results: the sun shadow now uses the
original's three cascades, the 10 cm caster offset (0x00C6C7A0 / vertex
shader 0x0A75E0) and its 3x3 comparison filter. The street edge now
matches, but a right-hand wall in the same-pose Cassardis street capture
shows dark patches the original doesn't have. Candidates codex-1 listed:
which meshes cast (casters that the original excludes), slope bias, the
filter. Captures: codex-1's same-pose pair (see that brief's Results for
paths).

## Do

Find why that wall darkens (try toggling casters by model/part, the
receiver's normal offset, the cascade the patch falls in), compare with
the original's shadow-cast pass rules, and fix it without eye-tuning.

## Done when

The same-pose capture no longer has the patches, the street edge stays as
good, tests pass, and the Result names the cause with its source.

## Result (codex-40, diagnostic checkpoint)

**The visible defect is not fixed; the proposed shadow cause is ruled out
at the measured wall regions.** No rendering change is included in this
checkpoint. Board question 3208 asks head to choose the next material/pass
trace rather than tune shadow settings that have no effect there.

The context above overstates the earlier result: the three cascade ranges
and equivalent 10 cm depth offset were implemented, but the source's 3x3
PCF was traced, not reproduced exactly in Bevy. The street edge was closer,
not accepted as an exact match.

Controlled captures used the previous private `ddview-shadow-bias.exe`,
the recorded `render-fidelity-codex1/wall-reference.json`, explicit stage
200, fine weather, clock 16:06:11 and 1920x1080. Every process used
`--unattended`, screenshot delay 10 seconds and a 60-second watchdog; all
exited normally. No original-game state was changed.

- `DDVIEW_NO_SHADOWS=1` removes the street shadow but leaves both right-wall
  rectangles **pixel-identical** to `wall-shadow-bias.png`: half-open pixel
  rectangles `(1600,250)-(1850,750)` and `(1730,560)-(1890,820)` have mean
  and maximum absolute RGB differences of zero. The street control
  `(400,500)-(800,900)` changes 91.44% of pixels by more than one byte in
  at least one RGB channel. Caster inclusion, cascade/filter and shadow
  receiver bias therefore cannot cause the defect inside these wall ROIs.
- A temporary diagnostic disabled only the `FLAG_NORMAL_MAP` selection
  in `Loader::material`. The wall changes, but its mean absolute RGB error
  against the original increases from `(8.55,8.24,7.85)` to
  `(12.71,12.36,11.87)` in the first ROI. The second ROI also worsens,
  from `(10.54,10.02,9.18)` to `(11.12,10.67,9.94)`. This is an isolation
  experiment, **not** evidence to disable authored normal maps. The
  temporary code was reverted; its executable remains private.
- A separate, deliberately redirected probe ray identifies
  `scr\\st200\\model\\st200_md19` at the wall. It intersects a hidden LOD
  4 mesh and two drawn meshes (LOD mask 3 and all LODs) within 2.54 cm.
  This does not establish an erroneous overlap; material/pass selection
  must be identified before removing any authored geometry.
- The existing ordinary bump path was checked against the previously
  decoded Stage200 PS `0x6171f8` and material PS `0x242be4`, VS `0x1381d4`:
  alpha/green decoding, Z floor, `cross(T,N)*W`, raw T/B lengths and
  normalized interpolated-result behavior remain consistent with that
  source. Whether those are the exact variants used by every visible
  wall layer remains unresolved. Deferred normal/light-buffer use and
  post-processing are also still possible differences, not proven causes.

Private evidence lives in `E:/ddda-re/notes/steps/S15/`:
`no-shadows.png`, `no-normal.png`, `probe-wall.png`, matching `.out/.err`
logs, and `diagnostic.exe`. The original remains
`E:/ddda-re/notes/camera-feel-codex33/wall-street-reference.png`.
No captures or game data are committed.

Validation: temporary diagnostic `cargo build --release -p ddview
--target-dir E:/ddda-rs-wt/m5-cassardis/target` passed in 32.68 seconds
after a narrow `cargo clean -p ddview --release` removed a stale shared
binary. Only the two existing viewer warnings appeared. Three bounded
viewer runs exited 0 with no shader error. This final checkpoint changes
documentation only, so no crate-test run was necessary. Suggested next
step: identify the exact md19 wall material/primitive and its original
normal-buffer/material-pass variants, then separate shading from the
original's depth-of-field response before choosing a fix.
