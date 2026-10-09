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

- `m0000_co` and `m0004_co` root motions;
- `pljumpparam` rows 0 and 9;
- the `m_pl000_00` ground speeds and ledge parameters.

The world is `ddworld::Terrain::load_collision` plus `add_scenery` over the
3x3 cells around the start cell.

Each frame steps ddsim by `dt_main / 30` seconds of game time. That is
sMain's `mDeltaTime`: the player's own delta is scaled by its motion rate,
see mismatch 1.

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

| route | quantity | original | ours | difference | tolerance | ok |
|---|---|---|---|---|---|---|
| run | path length | 1096.067 | 970.811 | -125.256 | 30 cm | **no** |
| run | max path gap (frame 195) | 0.000 | 124.177 | +124.177 | 30 cm | **no** |
| run | final position gap | 0.000 | 124.177 | +124.177 | 30 cm | **no** |
| run | final height | -152.648 | -138.133 | +14.516 | 5 cm | **no** |
| run | speed, frames 69..196 | 341.559 | 302.215 | -39.343 | 10 cm/s | **no** |
| run | stopping distance after release | 0.000 | 0.000 | +0.000 | 20 cm | yes |
| run | stamina rate, frames 69..196 | 0.000 | not modelled | - | 1 /s | **no** |
| sprint_stop | path length | 3292.847 | 2798.442 | -494.405 | 30 cm | **no** |
| sprint_stop | max path gap (frame 354) | 0.000 | 494.390 | +494.390 | 30 cm | **no** |
| sprint_stop | final position gap | 0.000 | 494.390 | +494.390 | 30 cm | **no** |
| sprint_stop | final height | 640.543 | 517.395 | -123.148 | 5 cm | **no** |
| sprint_stop | speed, frames 80..307 | 746.769 | 670.754 | -76.014 | 10 cm/s | **no** |
| sprint_stop | stopping distance after release | 198.061 | 0.000 | -198.061 | 20 cm | **no** |
| sprint_stop | stamina hold after sprint | 0.618 | not modelled | - | 0.067 s | **no** |
| sprint_stop | stamina recovery rate (idle) | 48.750 | not modelled | - | 1 /s | **no** |
| sprint_stop | stamina rate, frames 80..307 | -45.000 | not modelled | - | 1 /s | **no** |
| standing_jump | path length | 0.019 | 0.000 | -0.019 | 30 cm | yes |
| standing_jump | max path gap (frame 72) | 0.000 | 0.019 | +0.019 | 30 cm | yes |
| standing_jump | final position gap | 0.000 | 0.019 | +0.019 | 30 cm | yes |
| standing_jump | final height | 0.000 | -0.551 | -0.551 | 5 cm | yes |
| standing_jump | jump apex | 93.449 | 92.625 | -0.824 | 5 cm | yes |
| standing_jump | takeoff after press | 0.120 | 0.108 | -0.012 | 0.067 s | yes |
| standing_jump | airtime | 0.638 | 0.639 | +0.001 | 0.067 s | yes |
| standing_jump | landing after press | 0.758 | 0.747 | -0.011 | 0.067 s | yes |
| standing_jump | jump distance (press to landing) | 0.019 | 0.000 | -0.019 | 30 cm | yes |
| running_jump | path length | 722.416 | 686.885 | -35.531 | 30 cm | **no** |
| running_jump | max path gap (frame 86) | 0.000 | 36.293 | +36.293 | 30 cm | **no** |
| running_jump | final position gap | 0.000 | 35.631 | +35.631 | 30 cm | **no** |
| running_jump | final height | -106.855 | -101.703 | +5.152 | 5 cm | **no** |
| running_jump | speed, frames 41..77 | 335.761 | 300.518 | -35.243 | 10 cm/s | **no** |
| running_jump | jump apex | 93.461 | 92.625 | -0.836 | 5 cm | yes |
| running_jump | takeoff after press | 0.116 | 0.116 | +0.000 | 0.067 s | yes |
| running_jump | airtime | 0.704 | 0.714 | +0.010 | 0.067 s | yes |
| running_jump | landing after press | 0.820 | 0.830 | +0.010 | 0.067 s | yes |
| running_jump | jump distance (press to landing) | 316.589 | 321.022 | +4.433 | 30 cm | yes |
| running_jump | stamina rate, frames 41..77 | 0.000 | not modelled | - | 1 /s | **no** |
| drop | path length | 402.831 | 183.184 | -219.647 | 30 cm | **no** |
| drop | max path gap (frame 290) | 0.000 | 203.521 | +203.521 | 30 cm | **no** |
| drop | final position gap | 0.000 | 203.521 | +203.521 | 30 cm | **no** |
| drop | final height | -317.570 | -291.344 | +26.227 | 5 cm | **no** |
| drop | fall start | 0.010 | not modelled | - | 0.067 s | **no** |
| drop | fall time | 0.623 | not modelled | - | 0.067 s | **no** |
| drop | drop height | 287.355 | not modelled | - | 5 cm | **no** |
| drop | stopping distance after release | 0.000 | 0.000 | +0.000 | 20 cm | yes |

16 of 44 quantities pass. **Jumps match**: apex, takeoff, airtime, landing
and jump distance are within tolerance for both the standing and the
running jump. Ground movement, the sprint stop, slopes and stamina do not.

## Mismatches

1. **Ground speed is 15% low** (run -39.3 cm/s, sprint -76.0 cm/s, and the
   path gaps that follow from it).
   - In the original, while the action is run (2), sprint start (125) or
     sprint (5), the player's own `cUnit.mDeltaTime` (+0x14) is exactly
     **1.15 x** sMain's in every frame of every trace. In jumps, landings,
     the stop and idle it is 1.0.
   - Each frame's horizontal step is |velocity +0x2530| x that player
     delta. Medians are 0.983 on the downhill run and 0.957 on the uphill
     sprint, consistent with the speed applying along the ground (*not
     verified in the exe*).
   - So the original moves at the parameter speed x 1.15 in game time:
     run 10.165 x 1.15 x 30 = 350.7 cm/s.
   - ddsim applies `LOCOMOTION_RATE` (1.15) to the motion frame only,
     moving by `speed x dt`.
   - Applying the factor to ddsim's run speed gives 302.2 x 1.15 = 347.5
     against 341.6 measured. That residual and the sprint's (771 against
     747) are within what the different paths over sloped ground explain,
     but not proven.
   - Where the unit delta is scaled (the writer of +0x14 for the player) is
     not traced.
2. **No stop after a sprint.**
   - The original enters action 3 on release and keeps moving for 0.98 s
     (56 frames, median velocity 6.2 cm/frame), stopping 198.1 cm further
     on.
   - ddsim goes to `Wait` at once: 0 cm.
   - After a plain run the original also stops at once (0 cm), as ddsim
     does.
3. **Stamina is not modelled in ddsim.** The original, in game time:
   - sprint start and sprint spend exactly **-45.000/s** (1.5 per 30 fps
     frame);
   - during the stop (action 3) stamina recovers at +19.29/s, partly;
   - after a **0.618 s** hold, idle recovery is exactly **+48.750/s**
     (1.625 per frame);
   - running costs nothing.

   These agree with `docs/STAMINA.md` (Dash row 1, RecoverNormal body 1,
   and the 0.59-0.61 s hold). `ddparam::stamina` has the arithmetic
   fragments, but ddsim has no stamina state.
4. **Steep slopes are walked down instead of fallen from.**
   - On the drop route the original falls (action 13) from frame 0, 287.4 cm
     in 0.623 s, then lands (14).
   - ddsim instead steps 34 cm down at frame 4, then walks down the slope
     to the same lower ground and never enters `Fall`. `ground_move`
     accepts any descent up to `STEP_DOWN` (50 cm) per quarter-frame step,
     so it has no slope limit.
   - The fall itself therefore cannot be compared yet: fall time and drop
     height are "not modelled" in this run.
5. **Collision difference at the drop.** The original stood on a surface
   at y 40253.3. Under the same point ddsim's ground (stage collision plus
   placed scenery) is 34 cm lower. That is placed scenery the original has
   and ddworld either misses or places differently; not investigated.

Not compared yet:
- ledge grabs: still needs a clean ledge, see the oracle brief;
- walking: covered only by the drop route;
- the town (relaxed) run against the field run: only the relaxed run was
  recorded, because the area attribute is not read;
- turning: every route keeps one direction.

## Next

Head assigns the fixes, which are outside this brief's scope:
- the locomotion delta factor;
- the sprint-stop action;
- a slope limit or slide-to-fall rule;
- stamina in ddsim.

After each fix, re-run `cargo test -p ddaccept -- --nocapture`. Add routes
(ledge, turn, field run) by recording them with `routes.py`.
