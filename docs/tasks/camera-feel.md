# camera-feel: The follow camera rotates when it bumps into things

- **Owner:** codex lane
- **Branch:** `task/camera-feel`
- **Board area:** `play/camera-feel`
- **Status:** open (high: the maintainer's worst complaint after playtesting)

## Goal

Maintainer, 2026-10-09 playtest after the camera-near merges: "when
bumping the camera, the camera rotates instead of just moving in a
transform action. it rotates and it's really bad. also it feels like it
clips on too much stuff." In the original, pushing the camera into a
wall slides it in along its line; the view direction does not swing.

Head's lead (not verified): `play::follow_camera` sweeps a sphere of
radius 1.3 x near = 125 cm from the **target** (140 cm above the feet)
toward the near-plane centre (`camera_sphere_cast`, 0x010FDEA0 /
0x010FD710). A 125 cm sphere at the target already overlaps any wall
within 1.25 m of the Arisen and any ceiling below 2.65 m (Cassardis
rooms, porches, alleys). The initial-overlap branch then pushes the stop
along the contact normal by up to 125 cm, off the eye-target line, and
the eye (stop - toward x near) leaves the line, so `look_at(target)`
swings the view. That is also "clips on too much stuff": the camera reacts
to geometry the original ignores.

Find what the original does instead, then match it:
- the collision query's filter for the camera sweep (sCollision::Param
  ctor arguments and mask 0x4081D01C in 0x007E1400, FUN_01163EB0's
  options): does it skip contacts already overlapping at the start,
  back faces, or some attributes;
- which point is really the sweep's start and which its end (re-check
  0x007DE570's ESI/EDI against 0x010FE5E0's argument order);
- the actual near plane and target height while walking in Cassardis.
Check it live: stand the original's Arisen next to a wall and under a
low roof in Cassardis, swing the camera into the wall, and record eye and
target (oracle reads, no save writes). Compare with the viewer at the same
spot (`--probe` logs the eye).

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks follow the safety rules in `docs/CODEX.md`. Test runs use
`--unattended`.

## Scope

`crates/ddview/src/play.rs` (follow_camera and helpers), `crates/ddsim`
sweep helpers if needed, `crates/mtcamera`, `docs/CAMERA.md`, this
brief's Results.

## Done when

Swinging the camera into walls and under low roofs in Cassardis slides it
in along its line, as in the original, with eye/target matched against a
live capture at one wall spot and one low-roof spot. Fill in Results and
report to `head`.

## Results

First checkpoint: the missing caller response was found in **0x007e1400**.
At **0x007e1b11..0x007e1b68**, after **0x007de570** corrects the eye, the
original adds the full eye delta to the look target. Disassembly confirms
ESI is the eye pointer and EDI the collision anchor at the helper call.
The viewer now translates both endpoints, preserving orientation through
off-axis wall/ceiling penetration recovery. Six camera-filtered release
tests pass, including the new wall/roof regression; release build passes
with three existing warnings. This is source-verified and tested, not yet
matched at the two requested original-game spots. Filter/active-camera
near-plane investigation and live comparison continue.

Second checkpoint: the original was found already in Cassardis stage200
(PID23408). Active `uCameraCtrl` near is **16 cm**, while its child
`uCameraGame` near is 96. The sphere query reads the active controller.
Constructor **0x01128740** sets 16, inherited by controller through
**0x007d3a80 / 0x007d6cd0**; **0x007d7210** copies child pose/FOV but
not near. Paused and 40-frame unpaused readings agree; original feet
(3376.76,-163.44,-2168.54), target height about134.52 cm, distance230 cm.
Private captures/readings: `E:/ddda-re/notes/camera-feel-codex33`. Save
backup made before unpausing; no movement/save writes; pause and bgOff
restored. Follow collision and projection now use16. Seven camera-filtered
release tests pass, including clear sidewall/roof that the old radius
wrongly hit. Updated release build and exact live wall/roof tests pending.

Third checkpoint: clean release build passed (27.11s, three existing
warnings). A genuine Cassardis rock-wall collision was captured with a
bounded, main-thread-installed entry snapshot at **0x007e1400**. Original
instruction bytes were restored and read back after each capture. Player
position and camera angles were restored; the game is paused/bgOff.
Replaying the exact candidate pose against installed stage200 collision
matches final eye and target within **0.000788 cm**; two clear angles
match exactly. The collision-only helper accepts explicit candidate
eye/target/near to avoid the viewer's assumed target height contaminating
the comparison. An opt-in ignored diagnostic reads private CSV samples
(`DDDA_CAMERA_POSES`, `DDDA_NATIVE_PC`) and reports errors, with no raw
game capture embedded. One diagnostic run completed; this is a measured
wall match, not a low-roof or full rendered-pose acceptance claim.
Low-roof contact selection and full viewer comparison remain next.

Fourth checkpoint: source material filtering is now translated from camera
Param setup **0x007e178d / 0x00de4710**, SBC traversal **0x01165660**,
and `sCollisionExt::vf10` **0x00422f40**. Mask 0x4081D01C selects exclusion:
triangles with any 0x0081D01C bit are removed before nearest selection.
Ray, sphere-overlap and sphere-sweep now agree. Seven camera regression
tests pass, including an excluded nearer triangle and eligible farther
triangle; a clean release build passes (28.76s, three existing warnings).
The camera-local filtered ray fallback repeats triangle collection only
when the generic nearest ray hit is excluded; this avoids widening the
shared Ground interface in this checkpoint.

A genuine low lintel contact was captured at original feet approximately
(2895.79,-78.45,-2368.56), restoring the bounded same-cell movement,
camera angles and original hook bytes afterwards (paused/bgOff). Its
current collision replay has **13.47934 cm** error; two neighboring clear
poses match exactly. Wall error remains **0.000788 cm**. This is evidence
of an unresolved difference, not roof acceptance. The outer caller can
choose an actor-relative collision anchor separately from the look target
(and switches anchor behavior below pitch -0.8); recording the actual
helper inputs is the next action before assessing convex geometry error.

Fifth checkpoint: direct **0x007de570** entry snapshots resolve the anchor
mismatch. The helper now accepts collision anchor and outward viewing
axis separately and translates the source angular radius allowance.
Original low-lintel collision replay matches eye/target to **0.000702 cm**,
with two clear samples exact; the wall rerun matches to **0.001041 cm**.
Eight filtered tests pass (seven camera plus existing HUD), and release
build passes (35.09s, three existing warnings). The diagnostic accepts
optional actual helper inputs. All hooks were restored and verified;
original restored pause/bgOff; live claim released for ambient research.

Remaining: the viewer's full follow adapter still uses its approximate
look target as collision anchor. Source chooses an actor-relative anchor
below pitch -0.8 (or actor avoidance), using authored camera parameters.
155 cm observed live differs from120 cm constructor default; do not
hardcode the capture. Next action is load `rCameraParamList` settings and
translate anchor selection, then compare the fully rendered follow pose.
The two collision contacts match; this does not close full-pose acceptance.

Sixth checkpoint: added typed `rCameraParamList` reader in mtcamera using
existing mtxfs. Two synthetic tests pass; install test reads60 unique
resources with167 normal records, preserving authored array slots. Strict
clippy passes. Source **0x00420e90** loads `param\cam_param`;
**0x007dc8a0** picks slot0 for default normal camera. Runtime adapter and
translated anchor selection still pending; no game parameter table copied.

Seventh checkpoint: default authored settings now load per PlayState;
normal anchor selection is translated and the viewer's reversed pitch sign
is covered by a regression. Authored distance is used; model/world actor
position currently share simulation position, actor-overlap/special-target
selection and target-height/lag pipeline remain explicit limitations.
Three parameter tests and strict clippy pass; nine filtered viewer tests
pass, one private test ignored by default. Six accepted live samples pass
optional0.01 cm replay gate (max0.001041 cm). Production build passes34.27s.
Bounded opening/skip-intro unattended Cassardis smoke loads data, probes
installed geometry, writes private screenshot and exits0. Direct play
stage200 instead used field origin and is excluded from visual validation.
Next: trace target-height/actor-avoidance pipeline before claiming full
follow-pose equivalence. The original wall/roof collision responses match.
