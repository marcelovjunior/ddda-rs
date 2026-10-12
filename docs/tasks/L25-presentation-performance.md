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

Status: merged with train 1 (main 59f55a9). Stopped on 2026-10-12 when the
agents were scaled down; see "State and next steps" at the end. Machine: the maintainer's (Ryzen 7 5700X3D,
RTX 4060, Windows 11), other agents' builds running at the same time, so
every timing is the best or typical of several runs and is labelled.

### 1. Zero-copy presentation (done; game frames not yet shown)

- `ddgpu::WgpuBackend::new(device, queue, adapter_info)` is the one
  constructor (`headless()` uses it), and `required_features()` is
  exported for hosts that create the device. At every Present the backend
  hands a `PresentSink` the back-buffer texture, its format and size, and
  the gamma ramp, on its own device and queue, after the frame's commands.
  `read_frames`, `last_frame` and `on_present` (headless capture, tests,
  oracle) are unchanged (ddb4f29).
- `ddgpu::present::Blitter` draws a back buffer into any target: aspect
  kept with black bars; exact texel copy at 1:1, bilinear when scaled;
  the gamma ramp is applied through a 256-entry lookup exactly as the
  read-back path does; output drawn into a non-sRGB view, so the bytes
  are shown as they are. `tests/present.rs`: the pixels shown equal the
  read-back frame (plain, with a gamma ramp, letterboxed); doubled, the
  corner pixels are exact.
- `ddplay` creates one device for the window surface and the backend
  (adapter compatible with the surface and with the backend's features).
  Present draws straight into the surface texture on the guest thread.
  With vsync (FIFO, the default) the surface paces the guest at Present,
  as a D3D9 Present with interval one does. `--no-vsync` uses Immediate,
  then Mailbox, then FIFO, whichever the surface has. On resize the
  window only records the size and the next Present reconfigures; while
  the game is not presenting (250 ms) the window repaints the last frame
  itself. F11 or Alt+Enter toggles borderless fullscreen. The title shows
  FPS, mean and longest frame time, the present mode and the frame count,
  updated every 500 ms. `--test-pattern` (a diagnostic, labelled in the
  title) drives the backend with clears and ColorFills.
- Measured (`cargo run --release -p ddgpu --example present_cost`, per
  Present on the guest thread, 300 frames): 1280x720 read-back 2.9-3.5 ms
  against 0.36-0.57 ms for the GPU path; 1920x1080 6.2 ms against
  0.37-0.50 ms. The old ddplay also cloned the frame, sent it to the
  window thread and uploaded it again, which this does not count.
- Not shown with the game: on this branch's base the game stops in
  `__mtinit` (call to untranslated 0x012F71E0, before any D3D9 use), so
  ddplay presents no game frame yet.

### 2. Backend costs (done)

Measured with `examples/draw_cost.rs`: the game issues no draws yet (see
above), so the stream is synthetic. Each frame has 1000 textured
triangles from a vertex buffer, four textures, constants varying per
draw and half the draws alpha-blended; the commands are built before
timing.

| | per draw (guest thread) | per frame, GPU waited for |
|---|---|---|
| before (one pipeline, layouts, uniform buffers and submit per draw) | 475 us | 478 ms |
| after (decdcd7) | 18 us | 21 ms |

What changed (try_draw restructured on top of claude-5's L20 4159466,
as agreed with them):
- shader input declarations are parsed once per shader;
- bind-group layouts, pipeline layouts and render pipelines are cached by
  everything they are built from (module keys with the depth-bias
  variant, vertex layouts with step modes, topology, cull, fill mode,
  colour targets with blend and write masks, depth-stencil state);
- texture bind groups are keyed by stage, sampler number, view kind,
  texture, sRGB read and the sampler states; target views are cached;
  `release()` drops both;
- constant blocks go into an 8 MiB uniform ring bound with dynamic
  offsets; DrawPrimitiveUP vertices and indices and triangle-fan indices
  go into a 4 MiB ring; ring bytes are written once per submission;
- draws are recorded into one encoder and submitted every 128 draws, or
  before anything else uses the queue (clear, copy, fill, read-back,
  present, query results, release, and uploads to a resource the open
  draws read);
- a buffer upload of the same size is written into the existing buffer.

Verified: claude-5's gate (`tests/draw.rs` 12 passed;
`DDGPU_FUZZ=1000 ... render_states_match`: 0 of 982 compared cases
differ from the D3D9 driver). A new `tests/cache.rs` runs 120 random
frames (formats, sampler and render states, strips, fans, indexed and
UP draws, buffer resizes, textures released and recreated) on one
long-lived backend and requires each frame to equal a fresh backend's.
With cull deliberately dropped from the pipeline key, 14 of 120 frames
differ, so the test does detect a missing key part.

### 3. Guest performance

Workloads (`crates/ddperf`, release build):
- `init`: the 5,662 static initializers in CRT order on a full session
  (5,655 complete, 6 stop, 1 not translated);
- `replay`: all 175,425 recorded vectors (`lift.json`, 14.7k functions)
  through their lifted functions on the runtime's own `GuestMemory`,
  each checked against the original's result (registers, xmm0, x87, the
  recorded calls, every byte written). Only the lifted call is timed.
  Before and after every change, 0 of 175,425 differ;
- `--count`: guest memory accesses by width and `call_ext` targets;
- `micro`: a `call_ext` round trip, and a guest read through `dyn Mem`
  against a direct call.

Where the time goes (baseline counts, one pass):
- replay: 11.6 M reads (5.8 M u32, 5.7 M u8), 16.8 M writes (9.6 M u8,
  7.0 M u32), 3,188 page-crossing, 354 k `call_ext`;
- initializers: 0.97 M reads, 0.14 M writes, 9,080 `call_ext` (half of
  them 0x012ED8D8/0x012ED89C).

Lifted code has about 1.3 M `m.set_u32`, 1.08 M `m.u32`, 270 k
`cpu.fl.cond("..")`, 133 k `call_ext` and 51 k `call_lifted` call sites,
plus about 300 k scalar SSE helper calls. None of the ddnative-base
helpers were `#[inline]`, so each `cond("ne")` was a cross-crate call
that matched the condition string at run time.

| step | replay, ms per pass | initializers, ms |
|---|---|---|
| baseline (5985a5b + L20 merge) | 550.7 | 14.6 |
| 1: GuestMemory single-page fast path, Registry address hash (ec4af23) | 356.8 | 9.6-11.8 |
| 2: ddnative-base helpers `#[inline]` (flags, cond, SSE), `Mem::read_bytes` (c811c62; main 59f55a9 after train 1) | not measured | 7.2, 9.1 (2 runs) |

Step 2's optimised build was stopped three times: twice by the memory
guard while the player's game was running, once by the build pause. It
needs one release build of the integration tree:
`cargo run --release -p ddperf -- replay --passes 3`, `-- init` and
`-- micro`, compared with the table above. claude-6 (L22) is changing the
scalar SSE helpers to non-pure `asm!` for exact MXCSR flags, keeping
`#[inline]`, and saves and restores MXCSR at service boundaries. Measure
after both land; they may cost a little against the intrinsic bodies.

Proposals not yet prototyped (each changes the lifter output, so it needs
`lift.py gen` and the required-vector replay; the lifter is shared with
L22, so they wait for a decision by the head):
- **Dense ids for cross-shard calls.** `x86::call_lifted` goes through a
  `OnceLock`, the facade's shard `match` and a binary search over the
  shard's 1,600-3,000 entries, at 51 k call sites. The lifter knows every
  target at gen time, so it could emit `call_lifted_id(m, cpu, ID)` with a
  dense program-wide index into a `Vec<LiftFn>` the facade fills in
  `init()`: O(1), same callee.
- **One concrete guest-memory type.** Every access goes through
  `&mut dyn Mem`, a virtual call that blocks inlining of the fast path.
  If lifted functions took a concrete `ddnative_base::mem::Guest` (the page
  table moved there, with `Sparse`'s fixture behaviour as its slow-path
  mode), the single-page fast path would inline at each of the 2.4 M
  access sites. `ddperf micro` measures what the virtual call costs.
- **Flags.** They are already lazy: each ALU operation stores its kind,
  operands and result, and a flag is computed only when read. With the
  helpers `#[inline]` (step 2), `cond("ne")` folds to the flag test, so
  emitting `cpu.fl.zf()` directly from the lifter would change code size,
  not speed.

### Unverified (for the integration build)

- c811c62 is not compiled: ddnative-base `#[inline]` attributes,
  `Mem::read_bytes` (ddnative-base, `GuestMemory`), `d3d9draw::read_bytes`
  using it, `ddperf micro`, and `crates/ddruntime/tests/memory_fast.rs`.
  Gate: required-vector replay (`DDNATIVE_REQUIRE_VECTORS=1 cargo test -p
  ddnative --test lifted --test units`) and `cargo test -p ddruntime`,
  including the new memory_fast test.
- ec4af23 (GuestMemory fast path, Registry hash): measured and checked
  with the 175,425-vector replay on `GuestMemory` and the initializer
  census; ddruntime's own tests not run since.
- 9896b64 (ddplay): built and run at 437415c; the later edits (adapter
  info, optional features, `--test-pattern`) are not compiled. To check
  by hand: `cargo run --release -p ddplay -- --test-pattern`, with and
  without `--no-vsync`, then resize and press F11 (the title shows FPS).
- ddgpu (ddb4f29, decdcd7): fully tested (light crate). claude-5 builds
  L29 on decdcd7 and will replace ddplay's copied optional-feature list
  with `WgpuBackend::optional_features()`.

Commits: ddb4f29, f95ca1b, 1c99b8a, 3ab3378 (merge of claude-5's
4159466), decdcd7, ec4af23, 9896b64, c811c62, and this Results commit.

### Prototypes (branch `task/L25-perf-prototypes`, for a later train)

Written during the build pause and not compiled yet. Each keeps results
1:1 and is proven by the required-vector replay plus `ddperf`:

- **924ce18, cached call targets.** Most direct calls in the lifted
  shards cross shards and resolve through `x86::call_lifted` (in one
  shard, 235 such call sites against 4 calls within the shard). A
  4096-slot thread-local direct-mapped cache now keeps resolved targets.
  The program's lookup never changes once installed, so entries cannot go
  stale, and a miss resolves exactly as before. `ddperf micro` times
  cached against searched resolution.
- **f034c8a, inline guest-memory fast path (devirtualised `Mem`).**
  `PageTable` moves to ddnative-base, and `Mem::page_table()` (default
  `None`) is added, with `GuestMemory` returning its table. lift.py lowers
  at gen time (agreed with claude-6; cached arms unchanged): each function
  reads `let pt = m.page_table();` once, and `m.u32(a)` becomes
  `m.fast_u32(pt, a)` (all six accessors), an inline page-table access when
  the table allows it, else the same trait call as before. Fixtures and
  `Sparse` return `None`, so the replay gate runs the old path.
  `test_lift_codegen.py` gets a rustc fixture comparing both paths on the
  same generated function, and `ddperf micro` times a read both ways.
- **Flags: no prototype needed.** They are already lazy (kind, operands
  and result are stored; a flag is computed when read). With the step-2
  `#[inline]`, the constant `cond("..")` folds to the flag test, and
  LLVM can drop a flag write that the next one overwrites with no call or
  read in between (`cpu` is a unique `&mut`). The lifter keeps every flag
  write, because a callee can observe the machine state; that stays as it
  is.
- **Measured without the lifted program** (light release build,
  `cargo run --release -p ddruntime --no-default-features --example
  perf_micro`, 4 runs, tight loops within one mapped page): guest u32
  read 1.6-1.8 ns through `dyn Mem`, 1.5-2.1 ns through a direct call,
  0.58-0.63 ns through the inline fast path; u32 write 2.7-2.9 ns against
  0.76-1.06 ns. A direct call costs about as much as the virtual one, so
  the gain comes from inlining. Estimate for one replay pass (11.6 M
  reads, 16.8 M writes): about 45 ms saved of 357 ms (12%). This is an
  upper bound from tight loops; the replay itself will tell.
  `call_ext` round trip through the Registry to a returning service:
  12.9-15.7 ns, so the replay's 354 k calls cost about 5 ms (1.5%) and
  are not worth a prototype. In a real session a service call also
  reads the clock for the time slice.
- **`call_ext`: measured, left as it is.** Each call costs a
  thread-local `Rc` clone, a boxed-closure call and a hash lookup, and a
  service call also reads the time-slice clock (`threads::maybe_yield`).
  At 13-16 ns a round trip, this is about 1.5% of a replay pass (above), so
  a binding cache is not worth it now.

### State and next steps (2026-10-12, run ended)

- On main (train 1): everything in sections 1-3 up to step 2, including
  beacf8f (memory_fast test fix). The integration gate passed.
- `task/L25-results` (this commit, from main): ddperf now exits after its
  report. Guest threads that a session starts kept the process alive, and
  `ddperf init` hung after printing, holding a heavy slot. Also the main
  numbers above: init 7.2 and 9.1 ms on main's release build.
- Not measured: `ddperf replay --passes 3` and `ddperf micro` on main
  (the run was cancelled for the wrap-up). Next: run
  `C:/ddda-targets/L25/logs/measure.sh`-style (build, then each workload
  with a timeout and output to a file) on main and fill in the step-2 row.
- `task/L25-perf-prototypes` (train 2, not merged): 924ce18 call-target
  cache, f034c8a inline guest-memory fast path, deebc5b test update,
  cd2dfa7 memory_fast fix, 9ab4a6e perf_micro example, docs; merged with
  main at fb3d5c6. Light checks pass (ddnative-base; 53 lifter tests
  including the fast-vs-trait rustc fixture; ddruntime runtime 38,
  memory_fast 2, d3d9_device 5, movies 3). It needs gen, the replay gate
  and ddperf before and after.
- claude-4's L42 (task/L42-parallel-threads) builds on f034c8a's
  `PageTable` with atomics. By their measurement in perf_micro, fast
  reads go from 0.8 to 1.5 ns, and the serial job workload is about 9%
  slower. Their suggestion for getting it back: two monomorphised accessor
  variants per function, chosen at entry like `pt`.
- claude-10's L34b makes ddplay follow the game's resolution, full-screen
  choice and present interval (hooks listed in board #9323).
- Still open: game frames in ddplay (the game stops before its first
  Present); a recorded game command stream for `draw_cost` (synthetic so
  far); render-pass merging of consecutive draws to one target (not
  done).
