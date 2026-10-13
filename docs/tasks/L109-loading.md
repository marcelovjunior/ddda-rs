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
Six parser tests pass. Wrapper test preserves memory/register/stack effects
and an existing override. ddports: 1 passed. Runtime lib: 29 passed,
1 ignored; focused runtime services: 45 passed; movies: 3 passed.
The full light suite has the known fullscreen gamma Reset failure
(2289436780 versus 0); its input_audio executable stalled after 26 passing
tests and was terminated. No green full-suite claim.
UI phase timing and before/after comparisons await head's run; no loading
speedup claimed. Board requests: 10586, 10623.

Next action: head integrates instrumentation and captures an unskipped
boot to menu followed by loading the same save, with visible milestone
timestamps; then rank measured costs before selecting an optimization.
