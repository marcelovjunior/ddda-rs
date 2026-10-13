# L134: combined native gate and next lifter costs

Owner: codex-7. Branch: `task/L134-combined-gate`.

## Brief

Gate the FPS integration containing L129 emission, L132 flag helpers,
L130 dependency traversal, L109c opt-in geometry, L125 apply ports,
L119 sink, L123/L126 worker and L127 comparison. Run the play-profile
required-vector native gate, readable differential suites and inventory
under `heavy.py`, `HEAVY_BIG=1`, four build jobs. Never run the game.
Then use the head's L131 `profile_c17draw.*` results to select the next
helper-heavy lifter pattern; coordinate ownership before implementation.

## Results

- Pinned integration: `941f5d2c`; merged final L132 `ff98640d` as
  `b2df481f`. The integration initially contained only L132 `9665cda8`.
  The final merge adds two private flag-evaluator inline annotations and
  L132's results; memory helpers remain unchanged from their baseline.
- Combined gate queued through the heavy slot. Dedicated target and logs:
  `C:/ddda-targets/L134/{combined-gate.sh,gate.out.log,gate.err.log}`.
  `NATIVE_PROFILE=play`, `DDNATIVE_REQUIRE_VECTORS=1`,
  `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`.
- The existing script covers ddnative, mtmath, mtcore, mtframe and inventory.
  The wrapper additionally runs ddnative-base, mtdraw, ddports and mtstate,
  with `MTDRAW_REQUIRE_APPLY=1` and `MTSTATE_REQUIRE_VECTORS=1` so absent
  original fixtures cannot silently skip those checks.
- No test result, image match, load-time or FPS claim yet. L131's requested
  profile files are not present at task start; requested them on the board.

Next: finish the pinned gate and report each suite before changing emission.
