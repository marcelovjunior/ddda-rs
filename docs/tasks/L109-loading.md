# L109: loading times

Owner: codex-6. Branch: `task/L109-loading`, base `task/fps-line`.
User brief: measure boot/main-menu/save loading, retain original visible
waits, reduce measured runtime costs. Only head runs the game.

## Instrumentation

Set `DDRUNTIME_LOAD_LOG=1`; capture stderr from ddplay. Run
`python scripts/load_timeline.py RUN.log --output RUN.md`.
Every record begins `load-log t=<seconds> event=<name>`.

- Windows process creation is zero; one FILETIME age sample at main entry
  anchors a monotonic clock. Other platforms use main entry.
- Host/guest first windows, runtime/GPU initialization, guest start/end,
  D3D device creation and first guest Present submission.
- Movie reader open begin/end and successful reader lifetime start/end
  (replacement, close or release). Decoder boundaries do not establish
  the first/last visible frame or the original fixed logo duration.
- Cumulative ReadFile **and ReadFileEx** bytes/calls/inclusive elapsed
  time, CreateFileA/W calls/time and successful `.arc` opens. Times include
  runtime allocation/copy/lock work. Errors keep existing semantics.
  Archive opens count opens, not unique paths.
- Once per second: these counters, archive read/inflate, draws/presents,
  and ddgpu module/translation/pipeline counters. Autoplay probes also
  use the process clock; logging does not enable autoplay or skip movies.
- Summed thread durations overlap wall time. Translation is contained in
  module time; inflate in archive read time. In-flight calls appear on
  completion; snapshots are approximate atomic samples.

The parser prints milestones, decoder lifetimes, cumulative costs and
frame changes. A draw threshold cannot prove title/menu/loading state.
Supply head-observed times explicitly with `--mark title_screen=SECONDS`,
`--mark main_menu=SECONDS`, `--mark load_game_pressed=SECONDS`,
`--mark loading_start=SECONDS`, `--mark loading_end=SECONDS`.
Missing UI milestones remain explicitly unmeasured. Legacy stage probes
have a later clock and cannot establish process-start timing. Keep each
process in a separate log. The final sample can precede forced termination.

## Archive evidence and optimization boundary

Read-only Ghidra/installed exe (2017-12-13 Steam), 2026-10-13:

- DTI identifies `rArchive::DecompressStream`, vtable `0x0142e278`,
  constructor `0x00e0d240`.
- Slot 12 (`+0x30`), `0x00e0d320`, reads/decompresses a buffered guest
  stream. It owns two input buffers and issues stream read/wait operations.
  It calls zlib inflate at `0x00cf02e0`, guest z_stream at `this+0x3c`.
- Timing wrappers call original pointers directly, preserving registered
  ports/mods. A per-thread scope restricts inflate accounting to archive
  reads. No decompiled logic is replaced.
- A whole-buffer miniz substitution has **not** been verified equivalent
  to this streaming contract (guest state, partial consumption, errors,
  callbacks, seek and buffer ownership). Output-only tests are insufficient.

The historical `ddgpu/src/pipelines.rs` profile reports 74% samples under
pipeline creation while loading. It predates L83 and is a hypothesis for
new measurement, not this branch's baseline.

## Results

Instrumentation implemented; no game or shard build/run by this lane.
Initial six parser tests passed; final eight tests pass. Wrapper test preserves memory/register/stack effects
and an existing override. ddports: 1 passed. Runtime lib: 29 passed,
1 ignored; focused runtime services: 45 passed; movies: 3 passed.
The full light suite has the known fullscreen gamma Reset failure
(2289436780 versus 0); its input_audio executable stalled after 20 passing
tests and was terminated. No green full-suite claim.
UI phase timing and before/after comparisons await head's run; no loading
speedup claimed. Board requests: 10586, 10623.

Next action: head integrates instrumentation and captures an unskipped
boot to menu followed by loading the same save, with visible milestone
timestamps; then rank measured costs before selecting an optimization.

## Existing logs and scratch-read experiment

Head logs `run_combo2.log`, `run_o1b.log`, `run_combo3.log` in the current
head scratchpad were inspected read-only. First probes with >=1,000 draws
per frame: **99, 83, 99 seconds**, respectively; autoplay declares gameplay
at **102, 86, 102 seconds**. These clocks start after host input setup,
not process creation. Earlier input/scene transitions differ. They are
not paired loading speedups and cannot identify fixed waits or separate
archive/pipeline/I/O time. The parser now supports this legacy format and
labels its time origin explicitly.

A separate host-buffer optimization reuses at most 128 KiB of scratch per
reading thread. It never caches file contents: every read still reaches
the same host handle/offset. Requests larger than the cap retain the old
temporary allocation; nested/reentrant requests get independent storage.
Only successful bytes reach guest memory, so short reads, EOF and failed
ReadFileEx reads cannot expose stale scratch bytes. The 128 KiB retention
policy covers the archive buffer size observed at 0x00e0d320; it does not
limit guest reads. File cloning, locking and APC behavior are unchanged.

CPU-only `load_read_scratch::read_service_cost`, optimized `play` profile,
synthetic 1 MiB file resident in the OS cache, three alternating A/B pairs
with five samples each. Guest pages are warm; seek is outside timing;
ReadFile service, host read and guest copy are inside timing. Allocation
counts use a thread-local counting allocator. Median of pair medians:

| Request | Before (us/call) | Scratch (us/call) | Allocations before/after |
|---|---:|---:|---:|
| 1 KiB | 3.077 | 3.166 | 1 / 0 |
| 128 KiB | 11.569 | 9.643 | 1 / 0 |
| 1 MiB (uncached scratch) | 266.907 | 258.903 | 1 / 1 |

128 KiB is 16.6% lower service time; small reads have no reliable timing
win. Unchanged large reads also vary, demonstrating shared-machine noise.
Unoptimized first pairs had no timing win and were not used as evidence
for production latency. None of these numbers is a whole-game phase time.
Before/after executables and logs remain under `C:/ddda-targets/L109`.
An initial optimized build exited without a diagnostic; retry with direct
toolchain executables succeeded. No shard or GPU build/run was needed.

### Final light validation

`cargo test -p ddruntime --no-default-features --no-fail-fast -- --test-threads=1`
with `DDRUNTIME_LOAD_LOG=1`: **153 passed, 1 failed, 2 ignored**. The only
failure is the preexisting fullscreen gamma Reset assertion in d3d9_device.
The formerly stalled input_audio target passes all **20** tests serially.
The two ignores are the existing dispatch benchmark and the new explicit
file-read benchmark. Targeted scratch nesting/unwind/bounded-retention test
passes after the final reserve_exact adjustment. **8 parser tests** and
**1 ddports test** pass. The parser CLI also rendered the real combo3 log.
Synthetic file tests cover varying request sizes, byte identity, short
reads, EOF and untouched output tails; existing ReadFileEx/APC and
install/overlay tests pass. No game/UI or original archive differential
comparison is claimed: archive code itself remains unmodified.

Head integration must preserve both L109 `wrap_archive` and L111
`wrap_executor` in the `ddports::ensure_with` closure. Full game-feature
wiring compilation belongs to head's integration build, not this light
suite. UI markers, fixed wait durations and before/after loading-phase
numbers remain pending head observations/run.
