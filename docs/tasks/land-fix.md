# land-fix: a body sinking under higher ground

- **Owner:** claude-1
- **Branch:** `task/land-fix`
- **Board area:** `player/movement`
- **Status:** review

## Goal

claude-10 (#1149, #1152) saw a guest Arisen in a two-window run sink
below the ground and fall forever (y to -181,000). The host's own sim did
it from plain demo commands. The ground exists along the path (about
41,000 high, nothing below it), and the body ended at 38,393, under it.

Likely a landing or step case where a jump or step meets higher ground.
Reproduce it headlessly from the coordinates in #1149, fix it with the
traced sweep rules, and add a regression test. Also: never let a body
fall more than a set distance below any ground without being found again
(and say what the original does: does it respawn or warp?).

## Scope

- `crates/ddsim/**`, `crates/ddworld/**`
- `docs/MOVEMENT.md`

## Done when

- A regression test reproduces it and passes.
- The rule is documented.
- Fill in Results and report.

## Results

Done (branch `task/land-fix`).

**Cause.** The m3-lan build (6980ad8) carried ddsim from before the
capsule sweep (f892a7e is not in its history). Its air move looked for
landing ground only below the body. A jump coming down into rising ground
passed under the single-layer surface and fell forever.

**Reproduced headlessly.** On stage 100 by 58m50n (radius 2, collision plus
scenery, as the net demo loads it), replaying the demo's inputs:
- the old ddsim loses bodies within seconds;
- main's ddsim, which sweeps whenever the ground has triangles, lost none
  in 4,500 random 60-second demo runs.

**Regression test:** `ddworld/tests/land.rs::demo_jumps_onto_higher_ground_stay_on_it`.
It replays two found runs (a jump landing into rising ground on the
plateau; a second jump into a rising slope). The old build sinks them 390
m below the ground; main keeps them on it.

**Point rules fixed too.** The rules that test grounds without triangles
use now look a step above the body: coming down it lands on rising ground,
going up it is held back (`jumps_into_rising_ground_land_on_it`).

**Lost-body rule (traced; docs/MOVEMENT.md "Bodies lost below the ground").**
The original has no kill height. It keeps a ring of 4 safe ground
positions (`0x00B62980`) and warps the body back to one of them
(`0x00B62D20`) when `0x00B63520` finds it:
- more than 200 m into a fall;
- 90 frames in the air without moving down;
- or on a stalled slope slide.

For the Arisen the warp runs behind a 15-frame fade (stage mode 0x27).
Falls of more than 10 m deal HP damage, which can kill. Surfaces with
attribute 0x80800 kill on contact.

ddsim now keeps the ring and runs the 200 m and 90-frame checks and the
warp. `Player::warps` lets a host show the fade. Tests:
- `a_body_falling_200_m_is_warped_back_to_safe_ground`
- `a_body_stuck_in_the_air_is_warped_back`

**Not modelled:** fall damage, kill surfaces and water, the slope-slide
stall, the check's exemptions, and the fade.

**Note for ddnet.** The new `Player` fields (`safe`, `stuck`, `warps`)
are not in the snapshot. A warp corrects the client through the next
snapshot.

**Tests:** ddsim 56, ddworld (incl. the install regression), ddhost,
ddnet, ddaccept and ddenemyrt all pass.
