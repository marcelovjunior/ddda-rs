# S41: Frame rate: profile the opening and the field, aim for 180 fps

- **Branch:** `step/s41-frame-rate`
- **Files:** wherever the time goes (`crates/ddview` rendering, streaming,
  animation, host frame); `docs/` perf notes; perf tests/benchmarks
- **Kind:** measure, then fix the biggest costs (repeatable)

## Context

The maintainer's machine: Ryzen 7 5700X3D + RTX 4060, 1080p. The game
targeted the PS3, so 180 fps or more should be reachable. Today the
field (`--perf-route`, stage 100) logs median ~31 ms, p95 ~39 ms (about
30 fps), with ~1050-1080 visible meshes, ~450 visible DD materials of
2003 allocated, ~4800 entities (perf.rs). The older opening-perf brief
(docs/tasks/opening-perf.md) has background; this step supersedes its
goal.

## Do

1. Measure first: CPU frame breakdown (Bevy systems, our own sections in
   perf.rs: host frame, character::animate, stream, ...) and GPU passes
   (render diagnostics already logged), in Cassardis and on the field.
   Use Tracy or Bevy's trace if needed. Write the breakdown down.
2. Fix the largest costs, one commit each, re-measuring: e.g. draw-call
   and material batching (unique meshes vs instances, material
   duplication), CPU animation/skinning work for off-screen or distant
   characters (the original's LOD and update culling), streaming work on
   the main thread, shadow-map cost, per-frame allocations.
3. Uncap the frame rate in the viewer (present mode, no artificial
   limiter) while the simulation stays at its fixed 30 Hz with
   interpolation; check nothing gameplay-related depends on render fps.
4. A repeatable benchmark (`--perf-route` numbers) recorded in the Result
   before/after each fix.

## Done when

The Result shows the before/after breakdown and the field route's median
frame time at or under 5.5 ms (180 fps) on the maintainer-class GPU, or
names exactly what still blocks it with measurements.

## Results (in progress)

- Branch `step/s41-frame-rate`, worktree
  `E:/ddda-rs-wt/s41-frame-rate`; reviewed source base `10da58b`.
  No renderer optimization has been measured or claimed yet.
- `--perf` and `--perf-route` already select `AutoNoVsync`; unattended
  runs already use continuous Winit updates. The host keeps its existing
  fixed-step simulation. There is no remaining assumed refresh-rate cap
  to remove in these modes.
- Added optional `profile-trace` feature, using Bevy 0.19.1's
  `trace_chrome` system/schedule/render spans. Set `TRACE_CHROME` to a
  private JSON path. This adds tracing overhead and is **not** the build
  used for acceptance frame times.
- `DDVIEW_PERF_SECONDS=<positive seconds>` makes a perf capture exit
  through ordinary `AppExit::Success`, flushing the trace without a
  screenshot or forced stop. Its clock starts at the first perf update,
  after startup preparation. Timing windows are labelled with their
  current stage and reset at stage handoff.
- Repeatable launcher `scripts/perf_capture.ps1` records a private pinned
  executable's SHA-256, arguments, elapsed duration, numeric OS exit,
  and read-only GPU engine samples including the original game's PID.
  The caller must hold `live/ddview` until actual exit. Example arguments
  and private paths are in the script. It does not control the original.
- `scripts/perf_trace.py` summarizes bounded warm CPU trace windows by
  thread and span; inclusive and exclusive spans are kept separate and
  overlapping threads must not be summed as frame time. Four regression
  tests pass (nested/parallel work, boundary clipping, distinct threads,
  broken capture rejection); launcher PowerShell syntax check passes.
- Initial source trace build **failed with exit 1**: merged S34's new
  `Victim::Npc` was missing from three `ddnet/src/mirror.rs` matches
  (431/446/489). Reported on board #5007/#5012 to the S34 owner; no
  renderer performance conclusion follows from this build failure.
- Head #4984 requires the original game to remain running for maintainer
  probes. Read-only original PID 23408 3D samples before our captures were
  25–31%. Relative before/after samples must record its concurrent share;
  isolated absolute 180 fps acceptance needs a later maintainer run with
  the original closed. Head currently owns the separate route capture;
  no S41 GUI capture has started and no GPU lease is held.
- Next: build the opt-in profiler after the S34 compatibility fix, obtain
  the free GPU lease, measure identical Cassardis/field scenes and the
  ordinary route, then optimize the largest measured cost.
