# m2-fixes: close the M2 acceptance mismatches

- **Owner:** claude lane
- **Branch:** `task/m2-fixes`
- **Board area:** `player/movement`
- **Status:** review

## Goal

Fix the mismatches claude-4's acceptance replay found against the
original (board #717; `docs/ACCEPTANCE.md` and the `ddaccept` test, merged
from `task/m2-acceptance`). Raise the score from 16 of 44 quantities in
tolerance:

1. **Ground speed is 15% low.** In the original, the player's
   `cUnit.mDeltaTime` (+0x14) is 1.15 × sMain's during run (action 2),
   sprint start and sprint, and each step is |velocity| × that delta. So
   `LOCOMOTION_RATE` scales the movement, not only the motion frame.
   Trace the writer of the scaled unit delta if you can; at least apply
   the measured rule with its evidence.
2. **Sprint stop:** on release the original enters action 3
   (`cPlActRunEnd`, motion 21) and moves 198 cm over 0.98 s. Its speed
   comes from a motion curve (`0x010571C0` reads curve k of the playing
   motion). Read that curve data in `mtlmt` (coordinate with whoever owns
   it; it is unclaimed now) and drive the stop from it.
3. **Stamina in `ddsim`:** the rules in `docs/STAMINA.md` / `PARAMS.md`
   (`ddparam`):
   - sprint -45.000/s;
   - +19.29/s during the stop phase;
   - a 0.618 s hold;
   - idle recovery +48.750/s;
   - run 0;
   - exhaustion behaviour if traced.

   Per player, host-owned.
4. **Slope and drop:** the original falls off a 2.9 m drop (action 13,
   287.4 cm in 0.623 s), while ddsim walks down it because `STEP_DOWN` is
   applied per 0.25-frame tick. Trace the original's ground-follow and
   fall condition and implement it frame-rate independently.
5. **The 34 cm ground difference** at the drop's start: find which placed
   collision we miss or misplace in `ddworld` (Euler order? `_p`
   layouts?).

## Scope

- `crates/ddsim/**`
- `crates/ddworld/**`
- `crates/mtlmt/**` (curve reading)
- `docs/MOVEMENT.md`
- `crates/ddaccept/**`, only to update the expected values, never to loosen
  a tolerance without evidence

## Done when

- `cargo test -p ddsim -p ddworld -p ddaccept` passes.
- The acceptance score is reported before and after.
- Each change cites its source address or the recording.
- Fill in Results and report to `head`.

## Results

Branch `task/m2-fixes` (claude-1). Acceptance score (`cargo test -p
ddaccept -- --nocapture`): **16 of 44 before, 40 of 44 after**. `cargo
test -p ddsim -p ddworld -p ddaccept -p mtlmt` passes (33 + 4 + 4 + 10);
clippy clean. Details: docs/MOVEMENT.md (ground speed, stopping, motion
curves and events, stamina, falling off, assumptions).

1. **Ground speed** (run, running jump, sprint: all rows pass). Traced:
   `uObjModel::vf90` (0x00885E30) sets the unit delta to sMain's x work
   rates x uPlayer +0xEE4..+0xEF8, and `0x00B81920` writes +0xEE4 = the
   `mMotSpd` of the action's stamina row (Walk / Run / Dash table, row =
   load class; run and dash row 1 = 1.15). ddsim moves and animates
   walk/run/sprint in that time (`Sim::time_rate`). The residual was the
   slope: the horizontal step is |v| dt / (1 + slope^2), fitted to the
   traces within 0.5% (velocity projected onto the ground plane;
   *measured*, engine code not traced).
2. **Sprint stop** (passes; 184 cm against 198). `mtlmt` now reads motion
   curves and evaluates them as `0x010571E0` does (`Curve::value`; all
   4,313 lists parse). `cPlActRunEnd` (motion 21): 22.5 x curve 9 (1 at 0,
   0.5 at 5, 0 from 25). The sprint start's ramp is curve 9 of motion 43,
   and the ledge rise / climb use curves 9 and 10.
3. **Stamina** (all four stamina rows pass: -44.7/s vs -45.0, hold 0.605 s
   vs 0.618, idle +48.80/s vs 48.75, run 0). `ddsim::stamina`, per player:
   recovery `0x00B81810` (RecoverNormal rows), dash drain `0x00B81920`,
   order and clamps `0x00B81450`. The hold is the motion events: `mtlmt`
   reads event groups (`event_bits`, translated from 0x01057060); group 1
   bit 0x20000000 holds recovery (sprint motions throughout, the stop for
   frames 0-17). Running out is flagged; `cPlActTired` is not modelled.
4. **Slope and drop** (fall start, fall time, drop height pass). Fall
   condition `0x00B6E5C0`: not grounded and ground > 30 cm below
   (`STEP_DOWN` 30). Steep ground (normal y < 0.7, *assumed* stand-in for
   the grounded flag) is never walked and holds a fall up without landing,
   so slopes are fallen down independently of the step size. Fall entry
   speed capped at adjust 0xD1 (7.0, `cPlActFall::vf05`). ddaccept: routes
   that start in the air start falling with the recorded velocity; the
   recorder's Ctrl bit no longer maps to walk (the original runs with it).
5. **The 34 cm** is not missing scenery: the original rests on the strip's
   edge 15-20 cm from its origin (strip top 40254, steep face under the
   origin 40219); no instance in the cell's layouts has collision there.
   It is the character collider's radius.

**Still failing (4, all `drop`):** path length, max gap, final gap, final
height. After landing the original slides along a wall and frees itself
at frame 195; ddsim keeps sliding. Needs the character collider (shape,
radius, slide) traced. Also seen: the original's landing lasts ~0.1 s with
the stick held; ddsim cancels it at once.

**Not done:** the 1.15 factor's two-frame lag; claude-2's walk/run start
frame phase sync (0x00B80260); the turn rate; load / body class selection
from weights; stamina modifiers and exhaustion.

**For head at merge (ddview, not touched):** load motion 21 in
`USED_MOTIONS`; build roots as `RootMotion::from_motion(&m)
.with_curves(list.curves[slot].clone()).with_events(list.events[slot]
.clone())` (as `ddaccept::new_sim` does) so sprint start/stop, stamina
holds and climb curves work in `--play`. `mtlmt::MotionList` has two new
fields, `MoveParam` one (`fall_max`, from adjust 0xD1), `Player` stamina
fields. docs/ACCEPTANCE.md's results table is now stale (claude-4's file):
the new table is in the test output.
