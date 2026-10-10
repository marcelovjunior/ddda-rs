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

## Result

Implemented in `11300d7` on `step/s07-ambient-ground` (codex-39).
The prior implementation retained proposals without publishing them; this
step now publishes each supported ground actor's root travel after querying
the host's collision with `height(x, z, y + 50)`. This is explicitly a
provisional simple snap, with an assumed 50 cm margin, not a translation of
the original scenery controller. Missing ground preserves the accepted
position and records a diagnostic while motion/action clocks continue.
Rejected proposals do not accumulate into later catch-up travel.

The related edit to `crates/ddhost/src/host.rs` passes its existing ground
provider to `Population::step`. Bird controllers remain unsupported (17
diagnostics); the two authored em8300 idle controllers now resolve ground.
No renderer edits or viewer build were needed.

Installed Cassardis test after 300 frames (positions and heights in cm):

| Group / record | Final position | Ground height |
| --- | --- | --- |
| 1 / 0 | `[404.71143, -9.342167, -1886.1266]` | `-9.342167` |
| 1 / 1 | `[301.68427, -9.23183, -2026.3726]` | `-9.23183` |

Both satisfy the 5 cm bound with zero observed difference. The test also
checks missing-ground position retention, diagnostics, advancing idle
clocks, duplicate admission, generation rejection and retirement.

Validation, using `--target-dir E:/ddda-rs-wt/item-use-review-target`:

- `cargo test --release -p ddhost --test ambient -- --nocapture`: 1 passed.
- `cargo test --release -p ddhost`: all 55 tests passed; 0 failed/ignored.
- `cargo clippy --release -p ddhost --all-targets --no-deps -- -D warnings
  -A clippy::type_complexity -A clippy::chunks_exact_to_as_chunks
  -A clippy::manual_is_multiple_of`: passed. The last allowance addresses
  existing `tests/replay.rs:39`; the other two are pre-existing allowances.
- `git diff --check`: passed.

Not compared against the original. Full wall/slope collision, bird AI,
door occupancy integration and guest replication remain open. Suggested
next step: trace the em8300 scenery controller to replace this provisional
snap with sourced movement resolution.
