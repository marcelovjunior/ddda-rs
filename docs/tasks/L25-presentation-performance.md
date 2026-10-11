# L25: presentation and runtime performance

- **Owner:** claude-9
- **Branch:** `task/L25-performance` (worktree `E:\ddda-rs-wt\L25`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

`crates/ddplay` shows the game in a window. The game's D3D9 commands are
drawn by the wgpu backend (`crates/ddgpu`) on its own headless device.
Every presented frame is read back to the CPU and uploaded again to the
window's device, which is simple but slow. The guest itself runs as lifted
Rust over a `Mem` trait (guest memory) and an x86 `Cpu` struct, and nobody
has measured its speed yet. The player cares about frame rate: an unlocked
FPS is a goal. This brief makes the host side fast and measures the guest
side.

## Task

1. **Zero-copy presentation.** One wgpu device is shared by the backend
   and the window surface. Present copies or blits the back buffer straight
   to the surface texture, with no read-back. Then:
   - frame pacing (present modes, vsync on and off);
   - window resize and fullscreen;
   - an FPS and frame-time readout in the title.
   Keep `ddgpu`'s headless mode, which the tests and the oracle use, and
   its `on_present` callback for capturing frames.
2. **Backend costs.** Pipeline and bind-group caching, buffer reuse
   (dirty uploads), and avoiding per-draw allocations. Measure with a
   recorded command stream; the recording backend in
   `crates/ddruntime/src/d3d9draw.rs` can capture one.
3. **Guest performance measurement.** Build `ddnative` with an optimised
   profile through the heavy slot (one build at a time; it is the largest
   crate). Measure the hot paths:
   - guest memory access through `Mem`;
   - `call_ext` dispatch;
   - indirect calls and flag computation.
   Use a fixed workload: the static initializers (`examples/initializers`),
   plus any function with recorded vectors run many times.
   Report where the time goes, and propose and prototype changes that keep
   1:1 results, such as a faster memory map, devirtualised `Mem`, or lazy
   flags in the lifter output. Prove each with the required-vector replay.
4. Record the measurements before and after in Results.

## Results

(open)
