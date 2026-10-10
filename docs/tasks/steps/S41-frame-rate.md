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
