# L66: lifted shard build speed

Assigned by head on 2026-10-12. Worktree `L66`, branch
`task/L66-build-speed`, base `2fc64ef6`, target `C:/ddda-targets/L66`.

## Scope

1. Optional sampled hot-function list in `lift.py gen`, dedicated optimized
   hot shards, cold shards at play opt-level 0. No list preserves today's
   function layout. Cross-package calls retain `call_lifted`.
2. Measure direct flag predicates and a smaller inline memory-access path;
   retain only improvements with unchanged semantics. Do not edit `x87.rs`.
3. Native differential gate, fixed-output single-shard timings, full play
   timings, and ddperf replay/main-loop comparison. Heavy jobs are serialized
   through `heavy.py`; shared native data is read-only.

## Baseline supplied by head

`ddlift-05`, 36,300 lines, play profile: opt1 278 s (213 s LLVM/codegen),
opt0 63 s; with `RUSTC_BOOTSTRAP=1 -Z threads=8`: opt0 27 s, opt1 173 s.
Full play build approximately 25 minutes. These are supplied observations,
not new measurements made by this task.

## Results

### Support batch (2026-10-12, integration pending)

- Seven Python tests pass for deterministic hot/cold ownership, package
  profiles, cross-package routing, unchanged writes and default restoration.
- One Rust accessor equivalence regression passes on the unchanged base:
  aligned/unaligned, page crossings, wrapping addresses, permission and lazy
  fallback, and both atomic modes.
- `scripts/build_speed.py` records command, compiler, environment, source
  hashes, exit status and elapsed time. `NATIVE_PROFILE=play` selects the
  exact play artifacts for the existing native gate; default checks remain
  unchanged. Bash syntax check passes; the full gate has not run yet.
- Shared lift patterns/vectors copied into private `E:/ddda-re/notes/L66/native`;
  SHA-256, sizes and mtimes unchanged. Inventory uses a private SQLite backup.
- Generator integration is drafted and passes synthetic generation checks,
  but is not applied: claude-6 owns `lift.py` and is changing local-call
  override dispatch in L60. Coordination requested on the board. The draft
  accepts an ownership function so those checks can remain intact.
- Single-shard baseline: fixed checked-in `ddlift-05`, 36,300 lines. First
  attempt exited 1 without a compiler diagnostic; retry is in progress with
  direct log capture and explicit compiler stack size. No measured speedup,
  full-build result or runtime comparison is claimed yet.
- Private logs, receipts and unapplied drafts: `E:/ddda-re/notes/L66`.

Next: finish the baseline, measure the same shard at opt0 with parallel
front end, then integrate hot ownership after L60 coordination.
