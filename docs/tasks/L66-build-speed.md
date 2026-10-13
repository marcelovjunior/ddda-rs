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

### Ownership update (head board #9974)

L66 now owns only function-level hot/cold layout, hot-list input, Cargo
package profile wiring and gate verification. Emission is claude-7/L73;
base helpers are claude-8/L74; toolchain/profile-level settings are
claude-9/L75. Private experiment notes and timing receipts were handed to
those owners in board messages #9983–#9985. No game runs by Codex: the head
runs FPS comparisons with builds paused (#9968).

## Baseline supplied by head

`ddlift-05`, 36,300 lines, play profile: opt1 278 s (213 s LLVM/codegen),
opt0 63 s; with `RUSTC_BOOTSTRAP=1 -Z threads=8`: opt0 27 s, opt1 173 s.
Full play build approximately 25 minutes. These are supplied observations,
not new measurements made by this task.

## Results

### Hot/cold layout (2026-10-12)

- `lift.py gen --hot-functions FILE` accepts hexadecimal addresses, one per
  line, with `#` comments. Unknown functions fail before output changes.
  No list preserves the 32 address packages and play opt1. An explicit
  list (including an empty list) selects cold opt0; sorted hot functions
  occupy dedicated opt1 packages, at most 2,000 functions per package.
- Cross-package calls keep `call_lifted`. Shared instruction addresses
  retain their original owner precedence even when hot functions move.
  Package metadata, facade, step lookup and retired hot packages are
  regenerated together; unchanged writes preserve mtimes.
- Current generated corpus: 59,734 functions, 167 modules, 33 packages.
  The provisional 83-function list comes from the two profile text reports
  supplied by head (43 loading owners, 62 movie owners, union 83). It is
  private at `E:/ddda-re/notes/L66/profile-text-hot.txt`. These truncated
  reports are not a complete title/gameplay profile.
- 20 focused Python tests pass (nine hot-layout, seven codegen, four math
  wiring), plus one real Rust synthetic hot/cold call-graph test. The
  unchanged base passes the new accessor equivalence regression.
- `NATIVE_PROFILE=play scripts/native_gate.sh` passes in **106.88 s** with
  required vectors. The lift corpus contains **178,130 cases / 59,734
  functions**; readable-port tests also pass. Compiler inputs unchanged
  during the gate. All ten shared input files retain SHA-256, size and
  mtime; private copies retain their content. No shared recordings changed.

### Measurements

All receipts/logs are private under `E:/ddda-re/notes/L66`. Builds use
`CARGO_INCREMENTAL=0`, target `C:/ddda-targets/L66`, and the heavy scheduler.
The parallel front end works on installed stable rustc 1.99.0 with
`RUSTC_BOOTSTRAP=1 RUSTFLAGS="-Z threads=8"`; it is an explicit experimental
build setting, not a workspace-wide compiler requirement.

| Workload | Before | After | Conditions |
| --- | ---: | ---: | --- |
| Fixed ddlift-05, 36,300 lines | 254.89 s | 21.49 s | opt1/default front end to opt0/eight threads; identical source |
| Fixed ddlift-05, opt1 | 254.89 s | 162.86 s | eight-thread front end only |
| Full play + emu | ~25 min (head observation) | 636.08 s (10m36s) | first full play build in L66 target, 32 cold + one hot package, eight-thread front end, four Cargo jobs |
| Sampled hot replay | baseline pending | 230 cases x 200 passes, 1.616 ms/pass | zero differing functions; concurrent machine load, preliminary timing |

The target already held dependencies from the single-shard experiments;
graphics/Unicorn and the full split were built during this measurement.
This was not a fully empty-cache build.

The observed full build is about 58% shorter than the supplied approximate
baseline. A matched local full-build/runtime comparison is still pending;
these numbers do not yet prove the main-loop FPS criterion. A preliminary
window smoke run presented frames while a foreign build was active; head
closed it. Its frame rates are invalid for comparison and are not credited.
The split binary is private at `E:/ddda-re/notes/L66/ddplay-split83.exe`;
head has the request to compare loading/movie/title phases.

Existing `ddperf replay` does not forward `Probe::page_table()`, so it times
the trait memory fallback. This limitation was reported in board #9973;
replay comparisons cannot establish the actual game's aligned-memory or
main-loop speed.

### Lean emission and ownership

- Head board assignment #9952 transfers `mem.rs` accessors and `x86.rs`
  Flags to head; Codex retains generator layout/emission. `x87.rs` is
  untouched. Head also relayed the user's revised tradeoff: about 20% FPS
  loss is acceptable for about 50% shorter builds.
- Direct flag predicates were measured on fixed ddlift-05 cold opt0:
  18.31 s original versus 23.31 s candidate. The first receipt's aggregate
  audit changed only because Cargo.lock added the hot package; shard
  source was fixed. No demonstrated compile gain, so the candidate is
  not retained. Its five lexical/alias tests passed privately.
- Two aligned-access/cold-fallback prototypes passed the accessor test but
  increased opt1 compile time: 162.86 s baseline versus 189.06 / 188.82 s.
  They were reverted. Some additional runs overlapped an untracked foreign
  build and are excluded from conclusions. Production base semantics are
  unchanged; head owns further helper work.
- L60's local-call change is not merged here. The generator accepts an
  owner callback so its call-site checks can be retained during integration.

### Reproduction

Use a private copy of the lift patterns/vectors and a private inventory:

```powershell
$env:LIFT_DATA = 'E:/ddda-re/notes/L66/native'
$env:INVENTORY_DB = 'E:/ddda-re/notes/L66/inventory.sqlite'
$env:CARGO_TARGET_DIR = 'C:/ddda-targets/L66'
$env:CARGO_INCREMENTAL = '0'
$env:CARGO_BUILD_JOBS = '4'
$env:RUSTC_BOOTSTRAP = '1'
$env:RUSTFLAGS = '-Z threads=8'
$env:RUST_MIN_STACK = '268435456'
$env:HEAVY_BIG = '1'
# Run each command sequentially through the heavy scheduler:
python E:/ddda-re/tools/heavy.py codex -- python research/native/lift.py gen --hot-functions E:/ddda-re/notes/L66/profile-text-hot.txt
python E:/ddda-re/tools/heavy.py codex -- cargo build --profile play -p ddplay --features emu
$env:NATIVE_PROFILE = 'play'
python E:/ddda-re/tools/heavy.py codex -- bash scripts/native_gate.sh
```

### Handoff / remaining measurements

The layout lane is ready for review. Support: `881a977a`; layout:
`509fd759`; test refinements: `74dedfba`, `8229a74b`; scope/measurement
clarifications: `ada62e99`.

A same-source all-opt1/eight-thread baseline and alternating hot replay are
queued through heavy.py (`HEAVY_BIG=1`, four-job request), since 20:21.
At handoff they remain blocked behind L71's GPU draw test; the owner/head
were notified with its process details. No result from this queued batch
is claimed here. Its private driver, `E:/ddda-re/notes/L66/baseline-full.py`,
restores the committed split83 sources/lock, checks that restoration with
Git, and posts success/failure to head and claude-9. Planned receipts:
`full-play-allopt1-threads8.json`, `replay-comparison.json` in notes/L66.
L75 can use those measurements without duplicating the baseline build.

Next: head supplies the complete sampled function list and runs the clean
loading/movie/title FPS comparison. Compare private `ddplay-split83.exe`
with `ddplay-allopt1.exe` after the queued baseline succeeds. No FPS
acceptance claim is made from the incomplete list or the loaded smoke run.
