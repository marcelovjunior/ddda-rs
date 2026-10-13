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

### L109c: boot cost continuation (2026-10-13)

Branch `task/L109c-boot`, base `task/fps-line`. Head's updated brief
(board 11084) drops fast boot entirely: original logo/movie timing remains,
and head presses Enter through the game's own input path for run tests.

| Process-relative boundary | instr1 | combo6a cold | combo6b warm |
|---|---:|---:|---:|
| Guest start | 0.718 s | 0.794 s | 0.725 s |
| Device created | -- | 1.491 s | 1.466 s |
| First guest Present | 6.002 s | 5.837 s | 5.858 s |
| Movie reader open | 16.095 s | 14.617 s | 14.638 s |
| Movie reader release | 44.805 s | 44.824 s | 44.786 s |
| Visible title / main menu | unmeasured | unmeasured | unmeasured |

The **5.043 / 5.134 s guest-start-to-first-Present** gaps are mostly
unattributed by existing counters. Cold combo6a has unchanged completed
I/O and inflate totals between 2.023 and 5.024 s: 32 reads / 3.73 MB /
9.3 ms inclusive, 21 zlib calls / 120.7 ms. There are **zero GPU modules
and pipelines** through that interval. Warm combo6b has the same plateau
(4.2 ms reads, 127.5 ms inflate); its precompile finished 538 pipelines
by 2.025 s. Completed shader creation, reads and inflate do not explain
the plateau. Totals exclude unfinished calls and overlap across threads.

The 2,979 boot archive inflates now total **1.39 s**, versus instr1's
**4.85 s**. This is decoder reduction, not a 3.46 s first-Present saving.
Later all-zlib totals reach **11.04 / 11.03 s** for 27,135 calls, versus
**1.592 / 1.580 s** for the archive subset's 3,740 calls. Nonarchive zlib
is a separate remaining cost, not evidence for the pre-Present gap.

Head's `profile_boot1.txt` / `boot1.log` (11034): main-thread CPU 7.5 s
of 9.1 s, 62.4% inclusive under sSceneExt constructor `0x0049adc0`, 60%
under geometry builder `0x00fe9580`. A nested `0x012eb690` symbol suggests
Vorbis, but FE9580's original disassembly constructs subdivided geometry
and has no call to that entry. Private split helper/public symbol
attribution needs resolution before selecting a readable port. The
profiled run's slower first Present is not an optimization comparison.

The inflate port was **already default-on in fps-line**; production
registration stays unchanged. **632 original exact-machine cases** pass
again with vectors required. New registration regression passes with the
environment unset and `DDRUNTIME_ARCHIVE_INFLATE=0`, and verifies explicit
override precedence. Read-ahead stays **off by default**; its sequential
regression has not been fixed. No shard or game run by this agent.

Next: exact geometry/Vorbis boot attribution, original-verified port of
the actual hotspot, and head's visible title/menu/save-loading milestones.

### Continuation: readable inflate fast loop (2026-10-13)

Rebased onto `task/fps-line`; baseline instrumentation/read scratch is
`45b89e6f`. Both L109 archive and L111 executor wrappers are retained.
Head was sent "ready for a load-log run" with that hash (board 10786).

The entire zlib `inflate_fast` leaf at **0x00cf2cb0** is now readable Rust
in `ddruntime/src/archive_inflate.rs`. It decodes literals, lengths,
distances, nested tables, overlapping output matches, all circular-window
copy paths, end-of-block and the three invalid-code/distance exits. The
adapter preserves general/XMM registers, final arithmetic flags, stack
locals and the overwritten argument. Error messages remain pointers into
the player's executable. No code/data table was copied into the repo.

The original **0x00cf02e0** streaming state machine still controls header
validation, checksums, table construction, partial buffers, allocation and
window maintenance. The original **0x00e0d320** archive reader still owns
its two buffers and read/wait/seek callbacks. This is a whole fast-loop
port, **not a replacement of those two outer functions**. Runtime setup
installs it unless `DDRUNTIME_ARCHIVE_INFLATE=0`, and preserves any explicit
override already at its address.

`research/native/vectors_inflate.py`, run through heavy.py, records **559
cases** from the original executable with Unicorn. Fixed-Huffman streams
are generated from synthetic inputs; constructed tables exercise window
wraps, all copy arms, lengths 3/4/5/8/17/258, history offsets, extra bits,
nested lookups and errors. The Rust test checks **every scratch, stack and
heap byte, all eight GPRs, all XMM lanes and defined arithmetic flags**.
All 559 pass. Vectors stay private at
`E:/ddda-re/notes/native-L109/vectors/inflate_fast.bin`; the test requires
them with `DDNATIVE_REQUIRE_VECTORS=1`.

`scripts/bench_archive_inflate.py` prepares a private isolated leaf crate:
the existing generated leaf and its two split helpers at opt-level 1,
readable/runtime at opt-level 3. **No ddlift crate is compiled.** Synthetic
vectors are replayed over paged runtime memory; setup is outside timing,
the complete function call inside; both implementations match the oracle
memory. Three alternating rounds, 1,000 calls per implementation/case:

| Produced bytes | Generated median us | Readable median us | Ratio |
|---:|---:|---:|---:|
| 1 | 0.449 | 0.195 | 2.30x |
| 2 | 0.590 | 0.212 | 2.78x |
| 349 | 18.209 | 2.260 | 8.06x |
| 512 | 16.274 | 3.239 | 5.02x |

This is decoder CPU cost, not whole-archive throughput or game loading.
Shared-machine variance is visible in the samples. Full light serial
suite after the port and cache integration: **168 passed, 1 existing
fullscreen gamma failure, 2 ignored**. The subsequent explicitly ignored
cache benchmark adds one test, not included in those counts. Head owns
the game-feature integration build and paired game loading run.

Next: compare head's process-relative baseline with the same route and
save using the readable fast loop, leaving the original waits intact.

### Head baseline: instr1

Head supplied `E:/ddda-re/notes/runs/instr1.log` (board 10874): combo5,
opt-level-1 generated functions, loading and nDraw instrumentation enabled.
This precedes the readable-inflate port. The parser output is private at
`C:/ddda-targets/L109/instr1-timeline.md`.

| Observed boundary/sample | Process time | Evidence |
|---|---:|---|
| GPU initialization | 0.059–0.618 s | 0.558 s wall |
| Runtime initialization | 0.618–0.694 s | 0.076 s wall |
| Guest start | 0.718 s | runtime event |
| First guest Present | 6.002 s | submission, not visible frame |
| Movie decoder lifetime | 16.095–44.805 s | 28.710 s; not fixed movie duration |
| Before movie, last sample | 16.032 s | archive inflate **4.854 s / 2,979 calls**; file reads **0.056 s**; pipelines **0.007 s** |
| Final sample | 175.271 s | archive inflate **5.482 s / 3,740 calls**; file reads **0.699 s / 377,123,971 bytes**; pipelines **1.526 s** |

About 89% of the recorded archive inflate time accumulates before the
movie opens. Inflate is the dominant **instrumented archive cost** in
that interval; the log does not attribute every second of boot. Between
69.072 and 100.105 s, archive inflate rises only **0.306 s** over **31.032 s**
wall time. Missing title/menu/Load Game/loading-end observations prevent
semantic phase timings or a paired loading-speed claim. Existing inflate
counters are archive-scoped. The next build additionally reports
`zlib_calls/zlib_ns` for all calls to 0x00cf02e0, including calls outside
the archive reader; those totals contain the archive subset and must not
be added to it.

### Bounded archive read-ahead (opt-in)

`DDRUNTIME_FILE_READ_AHEAD=1` enables a content cache for **ReadFileEx on
read-only `.arc` handles resolved to the install itself**. Overlay/home
files and writable opens are excluded. Each handle retains one forward
256 KiB window; least-recently-used eviction bounds retained buffer
**capacity** to 8 MiB per runtime (in-flight readers can temporarily retain
an evicted window). Buffers are reused when no reader holds them. Closing
a handle drops its cached window. Speculative read failure retries the
original requested range. Host file position, guest short-read/EOF bytes,
untouched tails and next-alertable-wait APC delivery are preserved.
Requests above 256 KiB bypass caching. ReadFile still uses host reads.

CPU-only optimized host-read benchmark: synthetic warm 1 MiB file, five
alternating samples of 800 calls each; excludes guest copying, metadata
and service dispatch. The small-range pattern cycles eight adjacent 1 KiB
requests; the large sequential pattern cycles eight adjacent 128 KiB
requests. Results after buffer reuse, median us/read:

| Pattern | Host read | Cache | Hits/misses per 800 calls |
|---|---:|---:|---:|
| Sequential 128 KiB | 7.601 | 12.903 | 400 / 400 |
| Eight adjacent 1 KiB ranges | 2.925 | 0.508 | 799 / 1 |
| Repeated 128 KiB range | 7.559 | 4.352 | 799 / 1 |

Large sequential reads **regress** despite halving host read calls; small
and repeated ranges improve. Earlier unreused-buffer sequential median
was 6.064 -> 17.092 us. No cold-disk or whole-game benefit is established,
so caching stays **off by default**. `io_totals` adds cache hits, misses
and fetched host bytes for head's optional experiment. The baseline's
0.699 s inclusive file time gives no evidence to prioritize this over
inflate. Byte/range/EOF/seek/eviction tests pass; actual ReadFileEx tests
pass for deferred APCs and uncached changing overlay contents. Final
focused checks also enable `DDRUNTIME_LOAD_LOG=1`.

### Original visible waits: retained unchanged

Read-only exe/Ghidra evidence, 2026-10-13. `aInitialize` update at
**0x004fc370** creates `uGUITrialLogosManager`. Its init **0x0072d5c0**
selects a sequence from **0x017eeec8**. **0x0072d670** reads records at
**0x01515f48**, `(input mode, hold base frames, resource pointer)`;
**0x0072d410** passes the timer to the GUI. The shared update
**0x0067d4c0** subtracts `cUnit+0x14` while in hold state. `sMain`'s base
rate is **30 Hz** (0x0041cec9; FRAME.md), so nominal durations at speed 1:

| Display | Hold | Timer record | Input mode |
|---|---:|---|---:|
| Capcom | 60 frames = **2 s** | 0x01515f48 | 4 |
| MT Framework | 60 = **2 s** | 0x01515f54 | 4 |
| Trial font | 60 = **2 s** | 0x01515f60 | 4 |
| Trial disclaimer (`menseki`) | 60 = **2 s** | 0x01515f6c | 0 |
| ESRB | 120 = **4 s** | 0x01515f78 | 0 |
| Caution disclaimer | 120 = **4 s** | 0x01515f84 | 0 |
| CERO | 120 = **4 s** | 0x01515f90 | 0 |

Selector 0 uses CERO, caution, Capcom (**10 s holds**); selector 1 uses
ESRB, Capcom (**6 s holds**); selector 2 uses Capcom (**2 s hold**).
These are alternative lists, not cumulative. Mode 4 allows the existing
input skip path; mode 0 reaches the timer gate. GUI entry/exit sequences
and frame-boundary transitions add time. The active region selector and
observed visual timings in instr1 were not supplied. Do not infer them
from draw counts or sum these holds with the decoder lifetime.

Separately, `uGUICapcomLogo` constructor **0x0067d3a0** sets the same
60-frame hold. Another embedded `uIdInitialize` path has **35-frame
(1.167 s) holds** at **0x00735120 / 0x00735230**, with 15-frame fade
requests (0.5 s each) through **0x0043f4c0 / 0x0043f570**. Its reachability
in this run is unestablished; it is not added to the startup totals.
Movie/menu user waits have no fixed duration established by this log.

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
