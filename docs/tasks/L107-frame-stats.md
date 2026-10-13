# L107: D3D9 frame command-stream breakdown

Assigned by head to codex-11, branch `task/L107-frame-stats`, based on
`task/fps-line`. Scope: CPU-only d3d9frame example, tests, dependencies and
documentation. No game/GPU runs or lifted-shard builds.

## Results

Implemented `crates/d3d9frame/examples/frame_stats.rs`:

```
cargo run --release -p d3d9frame --example frame_stats -- capture.ddframe --frames 0..3 --json stats.json
```

- Target-defined passes with exact surface/MRT/depth identities, formats,
  dimensions, levels, multisampling, ordinals and distinct state counts.
- Per-draw state-group changes and bit-exact VS/PS float4/register-byte
  deltas, per-pass counts/percentages, aggregate totals across selected frames.
- Consecutive draw run histograms with equal constants and ignoring constants;
  all non-draw commands are barriers. Full other state/topology must match.
- Cached d3d9shader static external float4 references, including matrix rows
  and excluding `def` locals. Relative indexing is explicitly unknown in extent.
- Clear/copy/fill events with rectangles, target metadata and pixel sizes;
  summary counts and pixel visits. No speculative pass purpose or bandwidth.
- Text profiler and incremental JSON; zero-based half-open selection,
  partial-frame labels, one previous draw/current-frame statistics retained.

Validation: `cargo test -p d3d9frame --all-targets`: **5 existing + 7 new
tests passed**. Cases cover Writer/Reader roundtrip analysis, frame selection,
empty/partial frames, 1,000 streamed frames, changed raw NaN/signed-zero bits,
integer/bool byte counts, MRT/depth/revisited targets, query/upload barriers,
range-only batching, matrix/local/relative shader references, and pixel sizes.
Release CLI run on the synthetic Writer capture produced valid JSON matching
the text report. Capture, JSON and report remain in `C:/ddda-targets/L107/`.
Runtime light suite: **151 passed, 1 existing ignored, 0 failed** with
`cargo test -p ddruntime --no-default-features`. Clippy reaches the example
without example warnings; strict `-D warnings` is blocked by existing
`d3d9cmd::deferred` and d3d9frame library warnings (unchanged). Builds used
`CARGO_TARGET_DIR=C:/ddda-targets/L107`, `CARGO_INCREMENTAL=0`, four jobs.

Sample (synthetic, not gameplay):

```
FRAME 0: 11 commands, 2 passes; 5 draws
 PASS 0 commands 0..=5: RT0 8x4 A8R8G8B8 @1/L0 MS0:Q0 + depth none
  4 draws, 8 primitives; 3 predecessor comparisons
  distinct shader pairs 1, texture/sampler sets 1, render-state lists 1
  vs_constants: 1 changed (33.33%); VS 1 float4 / 16 bytes
  runs same constants {1: 2, 2: 1}; ignoring constants {1: 1, 3: 1}
 PASS 1 commands 6..=10: RT0 8x4 A8R8G8B8 @2/L0 MS0:Q0 + depth none
  1 draw, 2 primitives; render_states and targets changed (100%)
SUMMARY: 2 frames, 3 passes, 13 commands, 6 draws, 12 primitives
  2 clears / 1 copy / 1 fill; VS 1 float4 / 16 bytes changed
  clear pixel visits 12; copy source 32/destination 8; fill 4
  runs same constants {1: 4, 2: 1}; ignoring constants {1: 3, 3: 1}
```

Limits: Reader retains its existing global decompressed blob dictionary,
so total RAM is not bounded by one frame even though commands/reports are
streamed. First draw's baseline is unknown (excluded from change percentages).
Static shader references are not executed read counts; candidate runs are not
proof of mergeability. Nothing has been compared to a gameplay capture yet.
Detailed definitions and fixture-generation command: `docs/FRAME_ORACLE.md`.

Next action: head records gameplay and runs the release tool with text output
redirected to a file, then uses target/pass facts to plan native renderer work.
