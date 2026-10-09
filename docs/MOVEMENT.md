# Player movement

How `ddsim` moves an Arisen, and where each rule comes from. Code:
`crates/ddsim`. Seen working with `ddview --play` / `--demo`.

## From the exe and data

| State | Action class (DDDA.exe) | Motion (bank 0, `m0000_co`) | Movement |
|---|---|---|---|
| idle | `cPlActWait` | 0 | none |
| walk | `cPlActWalk` | 3 | root motion: 198.5 cm / 37-frame loop |
| run | `cPlActRun` | 4 | root motion: 423.0 cm / 21 |
| sprint start | `cPlActDashBegin` | 43 (0x2B) | root motion: 387.5 cm / 14 |
| sprint | `cPlActDash` | 20 (0x14) | root motion: 344.4 cm / 17 |
| jump takeoff | `cPlActJumpBegin` | 10 moving, 13 standing | none |
| airborne | `cPlActJump` | 11 moving, 14 standing (loops from 41 / 40) | physics, below |
| sprint-jump takeoff | `cPlActDashJumpBegin` | 22 (0x16) | root motion 85.6 cm |
| sprint-jump airborne | `cPlActDashJump` | 23 (0x17) | physics |
| fall | `cPlActFall` | 7 (loops from 20) | gravity |
| landing | `cPlActLand` | 12 moving (root 32.8 cm), 15 standing | root motion |
| ledge hang | `cPlCliffHang` | 47 (0x2F) | none |
| ledge climb | `cPlCliffHangClimb` | 48 (0x30) | moved by code, not root motion |
| second jump | `cPlActWpnDaggerJumpSecond` | 0x611 (bank 6, file not identified) | physics |

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
- At 30 fps the normal jump peaks at 100 cm (19 + 17 + ... + 1).

## Assumptions and placeholders

- 30 frames per second (not traced; supported by the walk speed).
- Integration order: position += velocity, then velocity += gravity.
- Turning is instant (the original turns over time).
- Sprint is no faster than run per frame in the data (20.3 vs 20.1 cm per
  frame); the original's extra sprint speed (playback rate or code) is not
  traced.
- Step up 60 cm, step down 50 cm, fall gravity = normal jump's.
- Ledge grab detection (look 60 cm ahead, ledge 90-230 cm above the feet,
  hands 190 cm above the feet) and the climb path (rise, then step on) are
  placeholders: `cPlCliffHangBegin::vf05` (0x00AD9180) holds the original's
  geometry tests, not yet read.
- Double jump: the original allows it for dagger vocations; `--play` gives
  it to both players. Its motion (0x611) uses the normal airborne motion.
- No animation blending: states switch motions immediately.
- No stamina yet (`param\pl\stamina\plstamina_*`).
