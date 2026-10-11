# L06: lifter: split the generated code across crates

Owner: Codex (same session). Branch `task/L06-crate-split` from the head's
`main` (the head names the base commit), worktree `E:\ddda-rs-wt\L06`.
Do this before more coverage work: every further lifter gain makes the
single generated crate bigger.

## Problem

All lifted functions are modules of one crate, `ddnative`
(`crates/ddnative/src/generated/lifted_NNN.rs`, about 123 MB of Rust for
28,244 functions). One rustc process compiling it peaks at about 18 GB on a
32 GB machine (L05 measured 17.82 GiB), and memory grows with every new
batch. Twice tonight Claude Code killed jobs because the machine ran out of
memory. A rebuild after any regeneration recompiles all of it.

## Task

1. Split the generated code into several crates (e.g. `crates/ddlift/
   lift_00` ... `lift_NN`, a fixed number of shards by address range or by
   size, so a function's shard is stable across regenerations). Shared
   machinery (`Mem`, `Cpu`, `Flags`, `x87`, `platform`, `call_ext`) stays in
   a small base crate the shards depend on; `ddnative` hand ports stay where
   they are or move to the base, whichever keeps the dependency graph acyclic.
2. Direct calls between lifted functions currently use
   `super::lifted::f_xxxxxxxx`. Across shards that needs a crate path, and
   shard dependencies must stay acyclic: either order shards so callees come
   first (calls only go "down" the shard order, e.g. by lifting round), or
   route cross-shard calls through a function table / `call_ext`-style
   dispatch installed at start-up. Choose with measurements; keep the
   generated code exact (the vectors judge).
3. The tests: one integration test (or one per shard) must still replay
   every vector, and `scripts/native_gate.sh` plus `scripts/inventory.py`
   must keep finding the `// Translated from` and `// verifies` lines
   (check how they scan; adjust them if the paths change).
4. Measure peak memory per rustc and wall time for a clean build and for a
   rebuild after regenerating one shard, with `CARGO_BUILD_JOBS=2` and 4.
   Goal: peak per process well under 8 GB, total clean build not slower.
5. Update `lift.py gen`, docs/NATIVE_PIPELINE.md ("Memory", "Generated
   code") and the gate script if needed.

## Rules

As in L05 (`docs/tasks/L05-lifter-x87-arithmetic.md`, "Rules"): own
`LIFT_DATA` (the head seeds it), never write `E:/ddda-re/notes/native`, no
`native_gate.sh` runs against the shared data, commit source/docs and the
new crate skeletons (Cargo.toml, lib.rs, build plumbing) but not the
generated shard contents unless the head says so, git identity janne /
marcelovjunior55@gmail.com, no AI trailer, never push. Memory: never more
than one full build at a time, `CARGO_BUILD_JOBS=2` until the split is in.

## Results

Implemented 16 fixed address shards: `(address >> 20) % 16`, with 400
functions per generated module. Address-band membership does not change
when another function becomes liftable. Current bodies occupy 15 shards
(3 is empty); the largest generated shard is 10,435,929 bytes. Shared
`Mem`, `Cpu`, `Flags`, x87 and platform models moved unchanged into
`ddnative-base`, apart from the new direct-call resolver. `ddnative`
re-exports those modules, preserving hand-port/test imports.

Cross-shard direct calls use an immutable resolver in the base crate;
intra-shard calls remain direct Rust calls. The current cached bodies have
23,256 cross-shard and 6,991 same-shard direct call sites. No shard depends
on another shard or on `ddnative`, so call-graph cycles cannot create Cargo
dependency cycles. A topological split was not needed and would make
placement depend on the selected call graph. Public facade entry points,
`all()` and `dispatch_table()` initialize the resolver automatically; raw
shard users must call facade `init()` first. The resolver copies a function
pointer before invoking it, so nested calls, multiple threads and a prior
unwind cannot leave a borrowed dispatch table. It is separate from
`call_ext`, with no fallback that could silently change mock boundaries.

Generation preserves unchanged file timestamps and removes obsolete
generated modules. A body-only edit touches one shard; its facade and test
dependents relink, while unrelated shard crates remain fresh. The generated
facade preserves public `f_ADDRESS` entry points and the sorted `all()` API.
Generated shard source totals 123,633,044 bytes, plus a 2,836,261-byte facade.
Source-only handoff: the head must run `lift.py gen` before building the
merged skeletons. Generated bodies, facade, test and old-body deletions are
not part of the source commit.

Coverage is deliberately unchanged: **28,244 functions / 84,312 cases**;
no scan, new vectors, rejected functions or modified call boundaries.
The prepared pattern/vector files are hash-checked. The inventory now reads
shard `Translated from` citations, ignores obsolete monolithic bodies, and
retains the existing test declarations. Its read-only declaration check
finds **28,250 ports / 28,250 declarations, zero undeclared ports**, including
hand ports (family expansion is a separate inventory step). The shared
inventory database and native data were not modified, and the gate was not
run. The gate now defaults to two Cargo jobs and still tests `ddnative`,
which compiles all shard dependencies.

Full required replay and all seven Rust units pass after both clean builds.
The unchanged batch remains 28,244 functions / 84,312 cases. No new vector
recording is needed for this build-only change.

Measurements use the test profile, the same worktree target directory and
sequential Cargo runs (no vector workers or second build). Per-process
Windows peak working sets and concurrent working sets were sampled once
per second; these are observed resident memory, not total system commit.
The monolithic row is the recorded L05 reference, not another large build
run during L06. Clean four-job measurement used package-scoped Cargo cleanup
of all compiled packages, including test dependencies.

| Build | Jobs | Cargo build | Full replay | Command wall | Max rustc | Combined compilers/linkers |
|---|---:|---:|---:|---:|---:|---:|
| L05 monolithic reference | 2 | 9m19s | 29.83s | 590.69s | 17.82 GiB | not recorded |
| L06 clean | 2 | 3m34s | 29.20s | 244.19s | 1.59 GiB | 2.94 GiB |
| L06 clean | 4 | 2m02s | 29.42s | 153.29s | 1.59 GiB | 5.30 GiB |
| L06 one-shard body edit | 2 | 10.90s | 29.24s | 41.94s | 1.06 GiB | 1.26 GiB |
| L06 one-shard body edit | 4 | 10.99s | 29.38s | 41.86s | 1.10 GiB | 1.38 GiB |

The maximum process is shard 11; the facade stays below 0.83 GiB. Compared
with the recorded L05 run, the two-job clean compile is about 62% faster
and maximum compiler working set falls about 91%. All measurements/logs
and the monitor script are private under `native-L06`.

Incremental measurements regenerate shard 05 after adding a temporary
guest-state-neutral `black_box` expression to one cached function body.
Only shard 05, ddnative and its integration tests recompile. This exercises
a real body change, not just an mtime or comment; the earlier comment-only
probe took 10.89s at two jobs. All probes pass the full replay. Pattern bytes
are restored immediately after generation, and the original unmodified
generated body is restored before final validation. These timings exclude
generation itself; the command wall column includes compilation and replay.

23 Python regressions pass, including a synthetic multi-crate build with
nested same/cross-shard calls, automatic initialization, external dispatch
filtering, another thread, recovery after a missing-target panic, missing
callee rejection, stable writes, empty/stale shard cleanup and inventory
declarations. No new opcode semantics or coverage changes are included.

Final `cargo test -p ddnative` also passes (32 integration tests, including
the restored 84,312-case replay). The lifted test reads native-L06. Legacy
hand-port tests still have hard-coded shared fixture paths; this additional
suite check reads them without writes and permits absent fixtures to skip.
The required private lifted replay above is independent of that check.

Open limits: release builds and larger future batches are not benchmarked;
the resolver supports one immutable generated program per process. Raw
shard callers must initialize the facade. The head should regenerate after
merging and run its full native gate with the complete fixture collection.
No shared data writes, inventory rebuild, gate invocation or push occurred.
