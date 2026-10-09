# Acceptance: recorded routes replayed through ddsim

This is the M2 gate in `docs/MILESTONES.md` ("recorded paths, speeds,
jump arcs and stamina match within tolerance") as a test. Short scripted
routes are recorded in the original through the oracle (`docs/ORACLE.md`).
The same per-frame keys and frame deltas then drive `ddsim` on `ddworld`
collision, and the two runs are compared quantity by quantity.

- **Record:** `python research/ddda-oracle/routes.py record [route ...]`
  (live game, oracle loaded, save backed up first). It writes one CSV per
  route to `E:\ddda-re\notes\acceptance\`. These are recordings of the
  original: they stay private and are never committed.
- **Replay:** `cargo test -p ddaccept -- --nocapture` prints the table
  below. `DDACCEPT_DUMP=<route>[,every]` adds a per-frame dump of
  original against ours.
  - The route test skips without traces or without the install.
  - `cargo test -p ddaccept -- --ignored` runs the strict gate, which fails
    while any quantity is outside tolerance.

## Recording (2026-10-09)

The routes were recorded in stage 100, origin cell st100_58m49n: a rocky
mountain pass, daytime. The party was the Arisen (male, level 7) and three
pawns. That is the save's position after the reload described in
`docs/ORACLE.md`. Four routes start from the same pose, at local
(4355.5, 40157.4, 3315.0).

| Route | Script (frame indices at ~65 fps) | Frames |
| --- | --- | --- |
| `run` | idle 5 frames, then one direction for 3 s, then release (idle 1 s) | 260 |
| `sprint_stop` | direction + Left Shift for 4 s, then release (idle 2 s) | 458 |
| `standing_jump` | Space for 3 frames, then idle 2.5 s | 216 |
| `running_jump` | direction for 1 s, then + Space for 3 frames, direction 1.5 s, idle 0.7 s | 235 |
| `drop` | placed on a narrow raised strip (local (3475, 2210)); walk (Ctrl + direction) toward the lower ground 2.9 m below | 434 |

Keys are camera-relative, and the camera cannot be turned from the
keyboard. So the recorder picks, of the 8 keyboard directions, the one with
the longest walkable run in the stage collision (`ddda-ledges
LEDGES_LINES`). The trace records the camera yaw every frame, and the
replay gives ddsim the same yaw.

Each frame's row holds:

| Column | Source |
| --- | --- |
| keys | held during the frame |
| `dt_main` | sMain `mDeltaTime` +0x70 |
| `dt_unit` | the player's cUnit `mDeltaTime` +0x14 |
| position | uPlayer `mPos` minus sWorldOffset `mWorldAbsoluteOffset`, i.e. stage frame |
| velocity | uPlayer +0x2530 |
| yaw | uPlayer +0x257C |
| action | uPlayer +0x2DD4 |
| motion, motion frame | uPlayer +0xF40, +0x3CC |
| stamina | cPlayerInfo +0x2A8 |
| camera yaw | uCameraGame `mCameraPos` to `mTargetPos` |

The row is sampled right after each sMain::vf06 (the oracle's `trace`).

### Replay setup

`ddaccept::new_sim` configures ddsim exactly as the viewer's `--play` does:

- `m0000_co` and `m0004_co` root motions, with their curves and events
  (`mtlmt::MotionList::curves` / `events`);
- `pljumpparam` rows 0 and 9;
- the `m_pl000_00` ground speeds, fall speed cap and ledge parameters.

The world is `ddworld::Terrain::load_collision` plus `add_scenery` over the
3x3 cells around the start cell.

Each frame is one `Sim::step` of `dt_main / 30` seconds of game time
(sMain's `mDeltaTime`); ddsim scales the player's own time itself
(`Player::unit_rate`, two frames behind the action, as in the traces).

The player starts from the first frame's pose and stamina (taken as the
maximum too); a route that starts in the air (action 13) starts falling
with the recorded velocity. The recorder's Ctrl bit is not mapped to
walking: the original runs with it held (drop route).

The town or field variant (`Player::relaxed`) is not read from the
original yet. The replay takes it from the recorded run velocity: 10.165
cm/frame, the relaxed run (12.0 x 0.8471).

### Action ids seen at uPlayer +0x2DD4 (*observed*)

| Id | Meaning |
| --- | --- |
| 0 | idle |
| 2 | run / walk |
| 125 | sprint start |
| 5 | sprint |
| 3 | stopping after a sprint |
| 6, 8 | standing-jump takeoff, airborne |
| 7, 9 | running-jump takeoff, airborne |
| 13 | falling |
| 14 | landing |

## Results

Tolerances are ours, set for M2, not the game's (`ddaccept::tol`):

| Quantity | Tolerance |
| --- | --- |
| path gap at any frame | 30 cm |
| final position | 30 cm |
| speed | 10 cm/s |
| apex, heights | 5 cm |
| times | 0.067 s (two 30 fps frames) |
| stopping distance | 20 cm |
| stamina rate | 1/s |

"Not modelled" means ddsim has no counterpart, so the row fails.

Current output (task/m2-finish; history: 16 of 44 at recording, 40 after
m2-fixes):

| route | quantity | original | ours | difference | tolerance | ok |
| drop | path length | 402.831 | 276.541 | -126.291 | 30 cm | **no** |
| drop | max path gap (frame 290) | 0.000 | 115.525 | +115.525 | 30 cm | **no** |
| drop | final position gap | 0.000 | 115.525 | +115.525 | 30 cm | **no** |
| drop | final height | -317.570 | -304.375 | +13.195 | 5 cm | **no** |
| drop | fall start | 0.010 | 0.010 | +0.000 | 0.067 s | yes |
| drop | fall time | 0.623 | 0.612 | -0.010 | 0.067 s | yes |
| drop | drop height | 287.355 | 285.055 | -2.301 | 5 cm | yes |
| drop | stopping distance after release | 0.000 | 0.000 | +0.000 | 20 cm | yes |
| run | path length | 1096.067 | 1093.221 | -2.846 | 30 cm | yes |
| run | max path gap (frame 195) | 0.000 | 7.810 | +7.810 | 30 cm | yes |
| run | final position gap | 0.000 | 7.810 | +7.810 | 30 cm | yes |
| run | final height | -152.648 | -153.609 | -0.961 | 5 cm | yes |
| run | speed, frames 69..196 | 341.559 | 340.916 | -0.643 | 10 cm/s | yes |
| run | stopping distance after release | 0.000 | 0.000 | +0.000 | 20 cm | yes |
| run | stamina rate, frames 69..196 | 0.000 | 0.000 | +0.000 | 1 /s | yes |
| running_jump | path length | 722.416 | 728.697 | +6.281 | 30 cm | yes |
| running_jump | max path gap (frame 160) | 0.000 | 8.322 | +8.322 | 30 cm | yes |
| running_jump | final position gap | 0.000 | 6.280 | +6.280 | 30 cm | yes |
| running_jump | final height | -106.855 | -108.102 | -1.246 | 5 cm | yes |
| running_jump | speed, frames 41..77 | 335.761 | 339.482 | +3.721 | 10 cm/s | yes |
| running_jump | jump apex | 93.461 | 92.625 | -0.836 | 5 cm | yes |
| running_jump | takeoff after press | 0.116 | 0.116 | +0.000 | 0.067 s | yes |
| running_jump | airtime | 0.704 | 0.714 | +0.010 | 0.067 s | yes |
| running_jump | landing after press | 0.820 | 0.830 | +0.010 | 0.067 s | yes |
| running_jump | jump distance (press to landing) | 316.589 | 321.022 | +4.433 | 30 cm | yes |
| running_jump | stamina rate, frames 41..77 | 0.000 | 0.000 | +0.000 | 1 /s | yes |
| sprint_stop | path length | 3292.847 | 3281.972 | -10.875 | 30 cm | yes |
| sprint_stop | max path gap (frame 354) | 0.000 | 11.664 | +11.664 | 30 cm | yes |
| sprint_stop | final position gap | 0.000 | 11.664 | +11.664 | 30 cm | yes |
| sprint_stop | final height | 640.543 | 636.059 | -4.484 | 5 cm | yes |
| sprint_stop | speed, frames 80..307 | 746.769 | 747.120 | +0.352 | 10 cm/s | yes |
| sprint_stop | stopping distance after release | 198.061 | 184.411 | -13.650 | 20 cm | yes |
| sprint_stop | stamina hold after sprint | 0.618 | 0.605 | -0.013 | 0.067 s | yes |
| sprint_stop | stamina recovery rate (idle) | 48.750 | 48.803 | +0.053 | 1 /s | yes |
| sprint_stop | stamina rate, frames 80..307 | -45.000 | -44.717 | +0.283 | 1 /s | yes |
| standing_jump | path length | 0.019 | 0.000 | -0.019 | 30 cm | yes |
| standing_jump | max path gap (frame 72) | 0.000 | 0.019 | +0.019 | 30 cm | yes |
| standing_jump | final position gap | 0.000 | 0.019 | +0.019 | 30 cm | yes |
| standing_jump | final height | 0.000 | -0.551 | -0.551 | 5 cm | yes |
| standing_jump | jump apex | 93.449 | 92.625 | -0.824 | 5 cm | yes |
| standing_jump | takeoff after press | 0.120 | 0.108 | -0.012 | 0.067 s | yes |
| standing_jump | airtime | 0.638 | 0.639 | +0.001 | 0.067 s | yes |
| standing_jump | landing after press | 0.758 | 0.747 | -0.011 | 0.067 s | yes |
| standing_jump | jump distance (press to landing) | 0.019 | 0.000 | -0.019 | 30 cm | yes |

**40 of 44 quantities pass.** Runs, sprints, the sprint stop, jumps,
stamina and the drop's fall match. The four failing rows are the drop
route's path after the landing.

## Remaining mismatch

**The drop route's path after landing** (path length, gaps, final height).
After landing the original slides along a rock wall from frame 85 and
frees itself at frame 195; ddsim keeps sliding and ends 116 cm off.
Traced reason: the original moves the character with `cScrAdjust`
(uObjModel +0x19C0), whose radius 35 comes from `collision\pl\m0001`
(rObjCollision group 109, `mIsScrAdj`); read live, its capsule runs from
35 to 145 cm above the feet. The generic MtCollision solver sweeps and
slides it (0x0079E200 -> 0x010B4960 -> 0x010B3000, GJK/MPR callbacks,
several iterations). ddsim stands on a point and slides off walls found on
that capsule from 60 cm up; the solver is not translated
(docs/MOVEMENT.md).

Fixed since the first run (docs/MOVEMENT.md has the sources): the unit
time factor (stamina rows' motion speed, two frames late), slope
projection, the sprint stop and motion curves, stamina with motion events,
the fall condition on steep ground, the landing hold (event bit 0x10000),
walk/run start frames.

Not compared yet:
- ledge grabs: still needs a clean ledge, see the oracle brief;
- walking: no route walks (Ctrl runs in the original);
- the town (relaxed) run against the field run: only the relaxed run was
  recorded, because the area attribute is not read;
- turning: every route keeps one direction;
- running out of stamina (`cPlActTired`, not modelled).

## Next

- Translate the scenery sweep (`0x010B3000`) or record a route along a
  wall to pin down its slide.
- Record a turn, a field run, a walk (find the walk key) and a ledge grab.
