# m2-fixes: close the M2 acceptance mismatches

- **Owner:** claude lane
- **Branch:** `task/m2-fixes`
- **Board area:** `player/movement`
- **Status:** open

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

(filled in by the agent)
