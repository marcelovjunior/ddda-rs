# L41: whole-frame oracle (capture the game's frame, replay it on the driver)

- **Owner:** claude-5
- **Branch:** `task/L41-frame-oracle` (worktree `E:\ddda-rs-wt\L41`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md), merge-train rule

## Why

Every D3D9 piece (shaders, states, formats, MSAA) matches the driver in
isolation. Once the boot presents frames (the title is next), the proof
that a whole frame is right is a frame-level comparison: the same command
stream drawn by the host's D3D9 driver and by the wgpu backend, compared
image to image.

## Task

1. **Capture:** a backend wrapper (`d3d9cmd::GraphicsBackend`) that
   records every command of a frame (uploads, draws, clears, copies,
   presents) to a compact file. It needs the resources each frame
   references (textures, buffers, shaders), deduplicated across frames.
   Make it switchable in ddruntime/ddplay (an environment variable).
2. **Replay on the driver** with `d3d9ref`, which already does whole draws.
   Extend it to a whole frame, render targets included, reading back the
   back buffer and, optionally, every intermediate target.
3. **Compare** with the wgpu backend's output for the same file: a
   per-target diff image, plus a summary that names the first draw whose
   target differs (draw-level bisection).
4. **Test it on a synthetic multi-pass frame now:** G-buffer → light
   accumulation (fp16) → tone map → MSAA resolve → present. Use the real
   game frames as soon as the boot presents.
5. Results, the RUNTIME.md row, and `docs/FRAME_ORACLE.md` (how to use
   it).

## Results

(open)
