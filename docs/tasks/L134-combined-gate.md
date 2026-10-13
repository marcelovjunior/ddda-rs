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
- Combined gate ran through the heavy slot. Dedicated target and logs:
  `C:/ddda-targets/L134/{combined-gate.sh,gate.out.log,gate.err.log}`.
  `NATIVE_PROFILE=play`, `DDNATIVE_REQUIRE_VECTORS=1`,
  `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`.
- The initial script covered ddnative, mtmath, mtcore, mtframe and inventory.
  The supplemental check additionally ran ddnative-base, mtdraw, ddports and mtstate,
  with `MTDRAW_REQUIRE_APPLY=1` and `MTSTATE_REQUIRE_VECTORS=1` so absent
  original fixtures cannot silently skip those checks.
- Original script: **50 passed, one benchmark ignored**, inventory passed.
  Required lifted replay completed in **33.65 s**. Its 15m33s wall duration
  includes long suspensions for head game runs and is not a speed benchmark.
- Supplemental tests exposed a process-wide sink race between the two
  mtdraw differential tests: the sink test consumed type-0/type-2 commands
  from the other test's port execution. The same binaries pass serially;
  isolated old/current lifted bodies reproduce alternating failures under
  the parallel runner. This was not evidence of a port/lifter defect.
  A common test mutex fixes the interference without removing any assertions.
  The owner independently committed the shared fix as `4bcef3fa`.
- Supplemental recheck with test isolation: **38 passed**, including both
  400-frame mtdraw checks (38,865 ordinary calls; 7,876 commands offered and
  1,787 draws taken), 1,568 original apply ABI cases, 811 pure decoder cases,
  and 3,584 mtstate cases / 17,928 original calls. Base helpers: 28 tests;
  registration: one test. The original wrapper first misplaced `--timings`
  after libtest's argument separator; fixed privately before this recheck.
- Expanded `scripts/native_gate.sh` to retain those supplemental checks and
  require their original fixtures. Full expanded-script recheck passed at
  06:46:04: **88 passed, zero failed, one benchmark ignored**, inventory
  59,450 verified / 59,469 non-platform functions; existing five gaps remain.
  Adopted the owner's equivalent test fix as `b2a1355b` to avoid duplicate
  fixes when merging. The next combined gate also checks that exact fixture.
- No game/GPU run, image match, load-time or FPS claim by this agent.

Next: head assigned a second combined pin, `7aa2d159`, in board #11769,
adding L132b packed memory entries, eight L130 ports and native-worker fixes.
Merge and gate that tree separately; it requires another shard rebuild.

### Second combined integration: GREEN

Merged `7aa2d159` as `3ffa376b`, retaining both milestone sections. Commit
`10a484e9` matches `crates/` exactly to that pin: the head's version still
uses ordinary private evaluator inline annotations, so this pin does not
carry the first gate's final-L132 annotation changes.

All 33 shards rebuilt, same required-vector/play settings and target. The
expanded script completed at 07:03:13 with **94 passed, zero failed, four
benchmark tests ignored**. All corpus tests ran; only explicit timing
benchmarks were ignored. Per suite:

| Suite | Passed | Ignored | Result |
|---|---:|---:|---|
| ddnative (including lifted and units) | 32 | 0 | PASS |
| ddnative-base | 31 | 0 | PASS |
| mtcore (including eight L130 ports) | 11 | 4 | PASS |
| mtmath | 5 | 0 | PASS |
| mtframe | 5 | 0 | PASS |
| mtdraw (L125 ABI/decoders and both frame checks) | 4 | 0 | PASS |
| ddports registration | 1 | 0 | PASS |
| mtstate | 5 | 0 | PASS |
| Inventory declarations and summary | — | — | PASS |

Lifted replay: **178,130 cases / 59,734 functions**, 29.93 s. Inventory:
59,450 verified / 59,469 non-platform functions; five existing coverage
gaps and 17 owner-credited fragments unchanged. Logs:
`C:/ddda-targets/L134/gate2.{out,err}.log`; revision receipt:
`gate2-revision.txt`. The 11m10s wall duration includes game-run pauses.
Repository play defaults are opt-level 0 for the 32 address shards and
opt-level 1 for the hot shard; this is not the head's all-shards-opt1 timing.

This gate verifies L132b's packed page-table entries and the eight L130
ports at the assigned integration revision. Later L130/L135 changes are
not in this pin. The separate x87 experiment below was not included.

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
