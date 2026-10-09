# m2-sweep: the character collision sweep (last 4 acceptance rows)

- **Owner:** claude-1
- **Branch:** `task/m2-sweep`
- **Board area:** `player/movement`
- **Status:** review

## Goal

Translate the character capsule sweep-and-slide that claude-1 traced:
`0x0079E200` → `0x010B4960` → `0x010B3000` (about 530 lines plus geometry
callbacks). Use the `cScrAdjust` capsule (r35, 35-145 cm, ground check
-20). Replace ddsim's wall slide with it, so the 4 failing drop rows in
`docs/ACCEPTANCE.md` pass (the wall slide that breaks free at frame 195,
and the rest on a strip edge).

## Scope

- `crates/ddsim/**`, `crates/ddworld/**`, `crates/ddaccept/**`
- `docs/MOVEMENT.md`, `docs/ACCEPTANCE.md`

## Done when

- `cargo test -p ddaccept` reaches 44 of 44 rows, or the rest have traced
  reasons.
- The sweep is translated with its sources and tests.
- Fill in Results and report.

## Results

Branch `task/m2-sweep` (claude-1): f892a7e (sweep), 545889a (sprint
release frame), 18cc838 (docs).

**40 of 44** (`cargo test -p ddaccept`; was 40 with a different set). The
drop's fall start, fall time and drop height now pass on the translated
rules; the four drop path rows after frame 195 still fail (below).
`cargo test -p ddsim -p ddworld`: 45 + 5 pass.

Translated (`crates/ddsim/src/sweep.rs`, sources in the code and in
docs/MOVEMENT.md, Scenery collision):

- the loop of `0x010B0CA0` (10 moves, back-off 0.01, skin 0.1, stall rule
  with sCollision +0x188..+0x194) and the slide `0x010AFC60` (floor slide =
  the measured slope law, straight-down stop/undo, slope/ceiling
  projection, wall no-climb, corner and crease rules);
- contact categories `0x00DEFD00` / `0x00DEF0C0` with the game's sCollision
  values read from the exe: floor and wall angle 45 degrees, ceiling -10
  (`0x00422D00`, `0x00FB37F0`..), material masks 0 / 0x80000000 / 0x2002;
  `ddworld` now keeps each triangle's material flag word;
- the axis check (`0x011C5E90`, sphere 15 down from the lower centre) with
  its 0x400 bit and group switch, and the grounded flag of
  `uObjModel::vf90` (`Swept::grounded`), which drives falling off and
  landing;
- also found on the way: `cPlActLand::vf05` picks the standing landing
  unless the stick is within 45 degrees of the facing; `cPlActDash::vf06`
  lets a released sprint move one more frame before the stop.

The capsule is `cScrAdjust`'s (r35, 35-145 cm), the ground check -20 x the
unit delta. The original's GJK/sphere narrowphase is replaced by exact
capsule-triangle distance (with its back-face skip).

Remaining, with reasons: after frame 195 the original moves along the rock
edge it crawled along at 2.2 instead of 0.45 cm/frame; the direction and
slope fit the slide's crease branch, the speed and trigger do not, and no
other path in `0x010B0CA0` / `0x010AFC60` explains it statically. Not
translated and candidates: the sbc per-edge flags (bytes +0x18..+0x1A,
`mtsbc` does not read them), the query's check filter, depenetration
retries, the per-frame (not per quarter frame) sweep. A live trace of
`0x010AFC60` for frames 190-200 is requested (board #1090, claude-4 idle
on a paused game).

API (claude-10 / ddhost): additive only. `Ground` gains a defaulted
`triangles_in`; grounds without it keep the old rules. New `ddsim::sweep`
module, `Sim::sweep_param`, `GROUND_CHECK_DROP`, `FALL_CHECK_UP`;
`ddworld::Terrain::add_flagged`.
