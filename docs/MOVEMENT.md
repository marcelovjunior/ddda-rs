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
| ledge hang | `cPlCliffHang` | 47 (0x2F) | none |
| ledge climb | `cPlCliffHangClimb` | 48 (0x30) | moved by code, not root motion |
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

## Assumptions and placeholders

- 30 frames per second for velocities and physics. The jump timing supports
  it.
- **Open discrepancy:** recorded ground speeds are 3-10% above velocity × 30
  (sprint ~3%, run ~10%). The cause is not traced.
- Locomotion playback rate 1.15 is measured, not traced.
- Factor 0.8471 is measured; its writer is not traced (no direct store to
  +0xDA8 exists in the exe).
- Town or field is a per-player flag set by the host. `--play` starts in
  town mode (as recorded); F2 toggles it.
- Integration order: position += velocity, then velocity += gravity.
- Turning is instant. The original turns over time: the recorded velocity
  direction turns ~70°/s.
- No run-stop or sprint-stop motions (`cPlActRunEnd`; the recording shows
  sprint speed decaying over ~0.6 s).
- Step up 60 cm, step down 50 cm, fall gravity = normal jump's.
- Ledge grab detection (look 60 cm ahead, ledge 90-230 cm above the feet,
  hands 190 cm above the feet) and the climb path (rise, then step on) are
  placeholders: `cPlCliffHangBegin::vf05` (0x00AD9180) holds the original's
  geometry tests, not yet read.
- Double jump: the original allows it for dagger vocations; `--play` gives
  it to both players.
- No animation blending: states switch motions immediately.
- No stamina yet (`param\pl\stamina\plstamina_*`).
