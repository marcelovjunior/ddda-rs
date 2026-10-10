# S07: Ground the ambient creatures' root motion

- **Branch:** `step/s07-ambient-ground`
- **Files:** `crates/ddhost/src/ambient.rs` and its test
- **Kind:** one change

## Facts (codex-34, docs/tasks/ambient-creatures.md Results)

- `Population::step` (ddhost/src/ambient.rs) publishes positions straight
  from the creatures' root-motion proposals. The two uEm8300 run their
  idle actions; their scenery/root-motion resolutions are unresolved.
- The host already has the stage collision: `ddworld::Terrain` implements
  `ddsim::Ground`, with `height(x, z, below_y)`.

## Do

1. In `Population::step`, before publishing a creature's new position,
   put it on the ground: `y = ground.height(x, z, y + 50.0)`. If there is
   no ground there, keep the old position and count it as unresolved.
2. Pass the ground in as a `&dyn ddsim::Ground` parameter and update the
   caller(s) (search for `Population::step` and `.step(` in ddhost).
3. Label it in a comment: this is a simple ground snap; the original's
   scenery collision for these creatures is not traced.
4. Extend the existing installed ambient test: after 300 frames both
   uEm8300 are within 5 cm of the ground below them.

## Done when

`cargo test -p ddhost` passes, clippy has no new warnings, and the Result
has the two creatures' final positions and ground heights.
