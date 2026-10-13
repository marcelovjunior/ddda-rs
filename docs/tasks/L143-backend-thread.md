# L143: backend worker CPU cost

Assigned to codex-11 by head on 2026-10-13. Branch
`task/L143-backend-thread`, based on the FPS integration line. Runtime-only;
no game/shard builds or game execution. Scope: d3d9cmd deferred transport,
ddgpu constants, draw scratch, resource membership and state/pipeline lookup.
L138 owns Vulkan/finisher/pass changes; lib.rs CPU hunks agreed on board
#12096. Native worker modules belong to L141 and remain outside this change.

## Evidence and implementation

The head's private `profile_c21def.txt` attributes 48% CPU to the backend
thread, with allocation/free, constant packing, resource hash insertion and
draw derivation among its leaf samples.

- Typed bounded-channel entries replace heap-allocated erased closures for
  draws, draw batches, uploads, clears, copies, fills, query begin/end,
  release and native jobs/fallback. FIFO capacity, outstanding Presents,
  command marks, ownership, result fencing and panic propagation stay intact.
  Cold arbitrary presentation/result hooks still use boxed callbacks;
  caller-owned upload/inline/native/batched payloads still own their storage.
- Float/int/extra register ranges use bulk little-endian copies. Boolean
  vec4 padding is built in one stack block and copied together. Prefix zero
  filling, raw bit patterns, ring generations and reservation boundaries
  remain unchanged. Already-equal constant blocks still reuse their offsets.
- Vertex-buffer and instance-stream vectors are recycled even after an
  unsupported draw. Buffer handles are cleared before the next command.
- Resource membership uses sparse radix pages of generation-stamped bitsets,
  covering every u32 address without assuming dense IDs or alignment. Clear
  advances an epoch; overflow resets stamps. Pages allocate only on first use.
- Pipeline map keys retain their computed hash across lookup/insertion and
  keep full key equality. Persisted pipeline recipes are unchanged.
- The 256 single-entry derived-state slots were thrashing in the 1000-draw
  release fixture. Identity-hash buckets retain all full equality predicates
  (including targets, strides, instancing and shader/declaration/state Arcs).
  The memo is bounded to 8192 entries, sufficient for the measured ~5700-draw
  scene, and clears on release or capacity eviction. Hash collision is never
  treated as identity. Warmed memo hits avoid re-derivation allocations.

No rendering order, attachment operation, GPU binding value or draw arguments
are intentionally changed. This does not remove wgpu/driver allocations or
cache-miss/payload allocations, and is not a claim that every GPU draw is
allocation-free.

## `_NLG_Return2` attribution

There is no per-command `catch_unwind` in DeferredBackend. Only the normal
thread-root panic boundary exists; worker failure still disconnects waiters.
The former per-command boxed FnOnce call/drop is removed from hot commands.
The existing profiler discards `SymFromAddrW` displacement and raw addresses,
so the aggregate label alone does not establish that exceptions execute.
Private `C:/ddda-targets/L143/profile_nlg.py` preserves displacement, PC and
symbol size for this label; head was asked to run it (#12119). Actual unwind
attribution remains open until those samples are available. No panic guard
or correctness check was disabled.

## Validation and measurement

All builds use CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=4 and
CARGO_TARGET_DIR=C:/ddda-targets/L143. GPU commands require pause.py to report
`not paused`, are serialized by this lane and have a hard 60-second timeout
per executable invocation. Target-format cases use disjoint shards if needed.

- `cargo test -p d3d9cmd --release`
- `cargo test -p ddgpu --release --lib`
- Explicit release GPU binaries: draw, cache, targets, parallel_encode and
  backend_worker; each case invoked separately.
- `cargo test -p ddruntime --no-default-features --no-fail-fast`
- `cargo run --release -p d3d9cmd --example queue_cost -- 200000`
- `cargo run --release -p ddgpu --example backend_cost -- 1000 5`

The CPU-only queue fixture measures producer and completion latency for
prebuilt Arc-backed draws and counts process-wide Rust allocation calls. It
waits on marks without allocating a result channel inside the timed interval.
The GPU fixture uses generated game-sized shaders, six textures, changing
constants and 1000 prebuilt states on a 16x16 target. It separately reports
direct backend CPU time, deferred producer time and complete frame time
including GPU wait. Allocation counts include wgpu and per-frame work.
These are synthetic shared-host measurements, not gameplay FPS or equivalence
against original-game recordings.

## Results

Source baseline: `4933e05846242b46a8a372db7400fffe39e2fdad`.

- d3d9cmd **13 pass**, ddgpu **24 CPU tests pass** (four new CPU regressions).
- **29 distinct GPU tests pass**: draw 17, cache 3, targets 7, the new
  scratch/rejection oracle, and the 1/2/4-worker ordering/feedback oracle.
  This includes all 150 render-state cases and all 200 target-format cases
  across four shards each. Three ignored invocations are existing exploratory
  tests/benchmark. One unsharded render-state invocation hit the hard 60 s
  bound; all four subsequent shards passed (no reduced case count).
- Light runtime suite: **197 pass / 3 fail / 5 ignored** after excluding two
  child-process x87 summaries. Both timing-sensitive failures (Present
  throughput and parallel concurrency) passed immediate isolated reruns:
  **199 distinct pass**, with only the inherited fullscreen-gamma Reset
  failure outstanding. No assertion/threshold changes.
- After the combo23 pause cleared, **36 additional integration tests pass**:
  copy 3, MSAA 3, presentation 5, synthetic frame 3, native 6, native jobs 7,
  native comparison 4 and native comparison jobs 5. One existing exploratory
  MSAA test is ignored. No failure, timeout or unavailable-adapter skip in
  this cross-path gate. Total ddgpu: **24 unit + 65 distinct integration
  tests pass**, four existing ignored integration tests/benchmark. All 13
  integration test binaries were covered; GPU invocations were individually
  bounded to 60 seconds.

Three alternating baseline/candidate release pairs, 1000 prebuilt draws and
five measured frames per process, after warm-up. The other renderer agent
held launches for this window; other machine activity remains uncontrolled.

| Measurement | Baseline pairs | Candidate pairs | Median change |
|---|---|---|---|
| Direct backend CPU, us/draw | 2.97 / 2.71 / 3.58 | 1.97 / 2.29 / 1.46 | 2.97 -> 1.97 (-33.7%) |
| Direct frame incl GPU wait, ms | 5.41 / 5.10 / 6.14 | 4.39 / 4.91 / 3.94 | 5.41 -> 4.39 (-18.9%) |
| Deferred producer, us/draw | 0.19 / 0.19 / 0.18 | 0.15 / 0.15 / 0.13 | 0.19 -> 0.15 |
| Deferred frame incl GPU wait, ms | 4.93 / 5.30 / 5.68 | 3.82 / 4.13 / 3.43 | 5.30 -> 3.82 (-27.9%) |
| Direct allocations/draw | 12.970 / 12.910 / 12.910 | 0.162 / 0.162 / 0.162 | 12.910 -> 0.162 |
| Deferred allocations/draw | 13.992 / 13.932 / 13.932 | 0.184 / 0.183 / 0.184 | 13.932 -> 0.184 |

The remaining allocation rate includes wgpu/driver-side Rust and per-frame
work; it is not evidence of zero allocations for an entire GPU frame.

CPU-only transport, three pairs of seven 200,000-draw samples: pair medians
**331.8 / 431.5 / 475.9 ns** baseline vs **227.0 / 233.0 / 248.4 ns** typed
queue. Over 4.2 million draws: **4,200,003 -> 2** allocation calls (the two
remaining calls were one-offs, not per-draw payload allocations). These queue
measurements preceded the final GPU window and had more shared-host load.

Private logs and binaries are under `C:/ddda-targets/L143`; nothing from
that directory or the player's installation is committed. SHA-256 receipts:

- Baseline GPU benchmark: `0f7902f23b7b25af1cf2d8eb5c350b2e850c3864264a1bb489bdb32232c96416`
- Candidate GPU benchmark: `c4d913c8b1bd8b90587e3db5ac8addb2b68ea7a14381e47aa4d353de4909d5fd`
- Baseline queue benchmark: `13af90773ac13bcfd57c543f240b09cd006ebcdb69d59c4d95ff686a70b28b00`
- Candidate queue benchmark: `27c1ea0b5c55cf2ec33a84efab1d5d17bd9c39b2b1137d682b81a3c734ac9b20`

No game/shard execution, game data edits, push, or gameplay FPS/image claim.
Source commit: `e04b76ceeef0cb413de0d5931678b621644362d2`. The integration
agent has adopted it for L146; this final cross-path gate changes only docs.
Next: head runs the same-scene image/FPS and symbol-displacement profile,
then integrates with L138 (preserve its metadata/recorder hooks and unwrap
`Hashed<PipelineKey>.key` where it needs the full pipeline descriptor).
