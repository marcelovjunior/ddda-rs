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

### Follow-up selection from L131 capture

Head's private `profile_c17draw.json`/`.txt`: 1,756 rounds in 60 seconds
(29 Hz achieved). These are per-thread non-wait sample shares, not CPU
seconds or predicted FPS gains. The captured executable predates the final
L132 private evaluator cleanup (its leaf symbols still show `*_value`).

- Main thread: `fast_u32` 237/1,064 (22.3%), `fast_set_u32` 107/1,064
  (10.1%). Worker `0x1074`: reads 235/973 (24.2%). Codex-11 owns the
  corresponding L132b page-table/accessor work; no duplicate memory edit.
- Thread `0x1038`: x87 leaf frames 159/355 (44.8%); inclusive x87 frames
  270/355 (76.1%). All 32 page-table leaf samples (9.0%) have an x87
  caller. Most x87 leaf ownership is `0x01320320` (44), `0x01320700` (25),
  `0x01313780` (17). Main x87 leaf share is only 29/1,064 (2.7%), so an
  x87 improvement would not directly remove its dominant memory cost.
- Rejected dropping the x87 `page_table()` calls: GuestMemory uses them
  as PREEMPT polling points. Keeping only the lifted entry's lookup would
  change scheduling opportunities inside call-free loops.
- Candidate instead: x87 `exec` saves integer flags with PUSHFQ/POP even
  for memory/register operations whose caller discards the result. Compare
  a private execution path without that unused capture while preserving
  public `exec`, `compare_flags`, opcode validation, x87 state and all
  existing preemption points. No implementation or speedup claim yet.

Private helper attribution script/output: `C:/ddda-targets/L134/`.
No capture data or generated game bodies are added to the repository.
