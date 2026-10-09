# Player movement

How `ddsim` moves an Arisen, and where each rule comes from. Code:
`crates/ddsim`. Seen working with `ddview --play` / `--demo`.

## From the exe and data

| State | Action class (DDDA.exe) | Motion (bank 0, `m0000_co`) | Movement |
|---|---|---|---|
| idle | `cPlActWait` | 0 | none |
| walk | `cPlActWalk` | 3 field, 0x54 town | velocity: adjust 0xFE / 0xFF (5.0) × factor |
| run | `cPlActRun` | 4 field, 0x55 town | velocity: adjust 0x100 (15.0) / 0x101 (12.0) × factor |
| sprint start | `cPlActDashBegin` | 43 (0x2B) | velocity ramps to the sprint speed |
| sprint | `cPlActDash` | 20 (0x14) | velocity: adjust 0xF1 (22.5) |
| jump takeoff | `cPlActJumpBegin` | 10 moving, 13 standing | none |
| airborne | `cPlActJump` | 11 moving, 14 standing (loops from 41 / 40) | physics, below |
| sprint-jump takeoff | `cPlActDashJumpBegin` | 22 (0x16) | root motion 85.6 cm |
| sprint-jump airborne | `cPlActDashJump` | 23 (0x17) | physics |
| fall | `cPlActFall` | 7 (loops from 20) | gravity |
| landing | `cPlActLand` | 12 moving (root 32.8 cm), 15 standing | root motion |
| sprint-jump landing | `cPlActDashJumpLand` (action 0x0C) | 24 (0x18) | none (velocity zeroed) |
| ledge grab | `cPlCliffHangBegin` (0x34) | 45 (0x2D) free, 50 (0x32) braced; rising 49 (0x31) then 46 (0x2E) | moved by code, below |
| ledge hang | `cPlCliffHang` (0x35) | 47 (0x2F) free; braced keeps 50 | none |
| ledge climb | `cPlCliffHangClimb` (0x36) | 48 (0x30) (44 = 0x2C for a climb started during the grab, not modelled) | moved by code, below |
| second jump | `cPlActWpnDaggerJumpSecond` | 0x611 (bank 6 = `m0004_co` slot 17, assumed) | physics |

**Ground speed.** Walk, run and sprint are not moved by root motion.

- Each frame, the action's `vf06` sets the velocity (uPlayer +0x2530) to
  facing × speed (`cPlActWalk::vf06` 0x00ACA1F0, `cPlActRun::vf06`
  0x00ACA5D0, `cPlActDash::vf06` 0x00ACAF80).
- Speeds are floats in `param\pl\other\m_pl000_00`, read by id
  (`rAdjustParam`, docs/FORMATS.md):
  - walk = id 0xFE, or 0xFF in town (both 5.0);
  - run = id 0x100 (15.0), or 0x101 (12.0) in town;
  - sprint = id 0xF1 (22.5), or 0xF2 (40.0) when a global flag is set.
- Walk and run are multiplied by uPlayer +0xDA8 (`0x00B80690`, `0x00B80710`).
  It was recorded as 0.8471090 on the ground. Sprint is not multiplied.
- **Town variants.** The area query `0x0043DAC0` (uPlayer vf at +0x2F8)
  returns flag 0x50 in towns and some quest areas. Walk and run then use
  motions 0x54 / 0x55 and the speed ids 0xFF / 0x101 (`cPlActWalk::vf05`,
  `cPlActRun::vf05`).
- **Sprint start** (`cPlActDashBegin::vf06`): speed = max(entry speed,
  0xF1 × a ramp). The recording reaches 22.5 in 4-5 frames.

**Recorded** (read-only, uPlayer at ~200 Hz; mage, start area, town variant):
- Run velocity |+0x2530| = 10.165 = 12.0 × 0.8471, path speed 320-338 cm/s.
- Sprint velocity = 22.5, path speed 700-760 cm/s.
- The walk, run and sprint motions advance 34.5 frames/s; other motions
  advance 30.0 frames/s.

**Jump physics** (`param\pl\other\pljumpparam`, 117 rows; read at startup):
- At takeoff, velocity = SpeedY up + SpeedZ forward along the facing
  (`cPlActJump::vf05`, quaternion rotation of (0, 0, SpeedZ)). A standing
  jump gets no SpeedZ (`cPlActJump::vf14` zeroes it when the action's variant is 1).
- Gravity is the per-frame acceleration.
- Per frame (`0x00AC51D0`): horizontal speed loses DampingZ; held input
  moves the position by AddMoveSpeedXZ in the input direction (air control).
- Row selection: normal jump `cPlActJump::vf14` loads rows 0-8 by a player
  field at +0x354C (all nine rows are identical: SpeedY 19, SpeedZ 13.95,
  Gravity -2, AddMoveSpeedXZ 4.25, DampingZ 0.3); sprint jump rows 9-17
  (SpeedY 15.25, SpeedZ 22.9, Gravity -1.25); the dagger second jump rows 1-8.
- The simulation steps a quarter frame (120 steps/s): apex 92.6 cm. Recorded:
  93.1 cm, 0.325 s after takeoff.

**Ledges.** Translated in `crates/ddsim/src/ledge.rs`. Values are
`m_pl000_00` floats by id (0x5A-0x5F, 0x17C-0x17D, 0x189-0x18A,
0x190-0x191, `LedgeParam`); cm, relative to the feet.

- *Who tests.* `uPlayerBase::vf136` (0x00B6D510) runs the test every frame
  and enters action 0x34 when it passes (actions are ids into a 0x44-stride
  table of DTIs, e.g. 0x0C `cPlActDashJumpLand`, 0x0D `cPlActFall`,
  0x0F `cPlActEvasion`, 0x34-0x36 `cPlCliffHang*`). The reach (forward
  probe length) is 150 (id 0x5A) in actions 0x0C and 0x0F, else 100 (0x5D)
  when command slot 0 (the move stick) is active. `0x00B6FD20` then accepts
  only actions 0x0C, 0x0D and 0x0F and needs stick input (uPlayer +0x32C0
  != 0), so **a jump in the air never grabs a ledge**; a fall does, and
  only after dropping more than 100 cm (uPlayer +0x207C, the fall
  distance kept by `0x0088A7E0`, must be < -100; `0x00B6FA00`). The other
  mode of `0x00B6FD20` (uPlayer +0x206D bit 2, apparently AI-driven
  characters: facing within 75 degrees of the wall, adjust 0xA2) is not
  modelled.
- *Detection* (`0x00B6FA00` calling `0x00B6FD20` up to three times):
  forward casts from 200, 125 and 50 above the feet (ids 0x5B, 0x5C) along
  the facing must hit a wall (normal horizontal length >= 0.1); a cast down
  10 cm inside the wall from 225 (200 + id 0x5F) to 50 (30 for the low
  probe) must hit the top. The edge is that top point back at the wall
  face. Then: nothing between 150 and 250 over the player; clear lines
  from 250 over the player to 5 over the top (both ways); nothing 150 up
  from there; no surface within 20 of a point 50 over the top or of a
  point 75 out from the edge; and the ground under the player more than
  100 (id 0x5E) below the edge (cast 500 down). So tops 50-225 over the
  feet are grabbed.
- *Foot point* (`0x00ADBEB0`): step out from the edge in 15 cm steps (up to
  13), cast 210 down; stop at the first point (ground hit + 20, or 210
  down) at least 100 from the edge. Extend the line to 210; if that meets
  something the hang is *braced* (feet at the stop point), else *free*
  (feet 210 from the edge). The line must rise at least 20 degrees to the
  edge; if it is blocked from 20 cm out of the player, a 100 cm line is
  used.
- *Hang pose* (`cPlCliffHangBegin::vf05` 0x00AD9180): hands 22 / 33 out
  from the edge (ids 0x18A / 0x191, free / braced); body point 93 / 60
  (0x189 / 0x190) from the hands toward the feet; if a wall is found behind
  the body point (cast from 30-100 out back to the wall) the body is put
  22 / 33 off it and the feet re-cast 210 / 100 from the hands.
- *Grab* (`cPlCliffHangBegin::vf06` 0x00ADA980): a free hang whose feet
  start 30+ below the foot point (0x17C) rises at 20 cm per frame (0x17D),
  10 cm off the wall, then settles; otherwise the grab motion plays and the
  original places the body by IK.
- *Hang* (`cPlCliffHang::vf06` 0x00ADCDD0): stick push along the facing
  above 0.8 climbs; below -0.8 lets go (velocity 2 cm per frame out from
  the wall, `cPlActFall`). If a vertical cast across the edge 5 cm inside
  the wall finds nothing, the hang (and grab, and climb) lets go too.
- *Climb* (`cPlCliffHangClimb::vf05` 0x00ADD670, `vf06` 0x00ADDC00): the
  target is the body point moved onto the wall face, raised from the feet's
  height to 5 over the edge; the climb ends 10 in from the edge, 5 over it,
  in `cPlActWait`.

**Turning** (traced in part). `cPlActRun::vf06` builds the velocity from
the action's +0x60, which `cPlAction::vf06` copies each frame from uPlayer
+0x2654; `uPlayerBase::vf90` (0x00B61A20) sets that to the stick's angle
(uPlayer +0x32C4) while the stick is held. So the velocity follows the
stick at once; the model's yaw turns toward it through a rotator at uPlayer
+0x2570 (`0x007975D0`: target +0x2578, current +0x257C, rate +0x2580), at
the rate in uPlayer +0x2584 (run). That rate's writer for the player, and
the writer of +0x32C4 (which would explain the recorded ~70 degrees/s turn
of the velocity), are not found.

**Stopping** (traced in part). `cPlActRunEnd` (action 3) is the sprint
stop: `vf05` (0x00ACB570) plays motion 21 (0x15) when the motion shown is
the sprint's (0x14 or 0x4F); `vf06` (0x00ACB6C0) sets the velocity along
the current yaw (+0x257C) to adjust 0xF1 (22.5) x the motion's curve 9 x
uPlayer +0x68 x +0x29A8, and ends with the motion. The curve is motion
data `mtlmt` does not read yet, so the sprint stop is not implemented. The
run stop (a 35-frame motion in the recording) is not in `cPlActRunEnd`
(it sets no motion unless coming from a sprint); not found.

## Assumptions and placeholders

- 30 frames per second for velocities and physics. The jump timing supports
  it.
- **Open discrepancy:** recorded ground speeds are 3-10% above velocity × 30
  (sprint ~3%, run ~10%). The cause is not traced.
- Locomotion playback rate 1.15 is measured, not traced.
- Factor 0.8471 is measured. Its writers (codex-3, docs/PARAMS.md): MOVSS
  stores at 0x00B6B3D6 / 0x00B6B477 / 0x00B6B4D1 in 0x00B6B350, via
  player +0x2E54 -> object +4 -> owner; 0x0081C170 derives a horizontal
  scale from character customization. 0.847 is not reproduced yet.
- Open from the blend brief (claude-2), not applied: walk/run start frames
  are phase-synced (0x00B80260; start frames 18 / 20 / 17 / 0 otherwise);
  the walk/run playback rate is uPlayer +0x333C / +0x3340 (writer not
  found; ids 0x1E4/0x1E5 hold 1.15); walk/run/sprint velocity is also
  multiplied by the rate and by uPlayer +0x29A8, which may explain the
  ground-speed discrepancy.
- Town or field is a per-player flag set by the host. `--play` starts in
  town mode (as recorded); F2 toggles it.
- Integration order: position += velocity, then velocity += gravity.
- Turning is instant. In the original the velocity also follows the stick
  at once and only the model's yaw turns over time (Turning, above); the
  recorded velocity direction turns ~70°/s, cause not traced.
- No run-stop or sprint-stop motions (the sprint stop is `cPlActRunEnd`,
  Stopping above; the recording shows sprint speed decaying over ~0.6 s).
- Step up 60 cm, step down 50 cm (placeholders). Fall gravity = normal
  jump's: `cPlActFall::vf05` (0x00ACCDD0) keeps the gravity of a character
  already falling and otherwise takes the last jump row's (uPlayer
  +0x38DC), so this holds after a normal jump.
- Ledges, still open:
  - collision hits are classified floor / wall by their normal (up
    component 0.7) and count from both faces; the engine's hit flags (bit 0
    floor, bit 2 wall, from `0x00DEA870`) are not traced;
  - the grab's and climb's progress (the motion's curves 9 and 10 read by
    `0x010571C0`) is linear over the motion here;
  - the original poses the hanging body by IK (`0x00850BD0`) toward the body
    point and takes the root from it; here the feet go straight to the foot
    point, and the facing snaps to the wall instead of turning over the grab;
  - a hit attribute check (`0x007C6F90`, bit 7 refuses) and the character
    size scale (uPlayer +0x2E54 +0xF70, 1.0 here) are not modelled;
  - climbing early from the grab (needs the motion event counter uPlayer
    +0x361C) and the climb variant from motion 44 are not modelled;
  - +0x32CC (the hang's stick test) is taken as the stick's push along the
    facing; its writer is not found;
  - `cPlActEvasion` (action 0x0F, motion 6), which also tests for ledges,
    is not in the simulation.
- Double jump: the original allows it for dagger vocations; `--play` gives
  it to both players.
- No animation blending: states switch motions immediately.
- No stamina yet (`param\pl\stamina\plstamina_*`).
