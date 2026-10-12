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

Status: tasks 1-5 are done and tested on the synthetic frame. No game
frame has been compared yet, because the boot does not present one. The
work was stopped at the head's request (quota), with the one open item
in section 4. Usage: [FRAME_ORACLE.md](../FRAME_ORACLE.md).

### 1. What landed (branch `task/L41-frame-oracle`)

- `109639c` **Render-to-texture.** A draw or clear into a render-target
  texture's level before the texture is bound now works on both
  backends. The synthetic frame and the game's G-buffer need this.
  - `d3d9cmd::Target.texture` is now `Option<TargetTexture>`, which
    describes the texture. `TextureBinding.usage` carries D3DUSAGE.
  - ddgpu creates render-target and depth-stencil textures in their
    target format, as render attachments from creation (as the head
    asked), with sRGB views for SRGBWRITEENABLE.
  - The channels a target format lacks are sampled as 1, as D3D9 does,
    through the new `d3d9wgsl::Options::sampler_ones`.
  - In `d3d9ref`, a `Session` keeps device objects across commands,
    keyed by the runtime's resource numbers. Render-target textures are
    created in D3DPOOL_DEFAULT and their levels taken with
    GetSurfaceLevel or GetCubeMapSurface.
  - `d3d9ref::Driver` is a `GraphicsBackend` on the driver. The
    `Oracle::render*` batch calls now run on a `Session`, so every
    existing per-feature test covers it.
- `717e306` **The frame oracle.**
  - `crates/d3d9frame` provides the capture file (`Writer`, `Reader`),
    the `Capture` backend wrapper, `oracle::compare` (per-target
    comparison at each Present, PNG images, bisection to the first
    differing command) and pixel decoding.
  - ddruntime wraps every installed backend, and the default recorder,
    in a `Capture` when `DDDA_CAPTURE` is set. `DDDA_CAPTURE_FRAMES`
    limits the number of frames. This covers ddplay and headless runs.
  - `crates/ddgpu/examples/frame_oracle.rs` is the command-line tool.
  - `docs/FRAME_ORACLE.md` documents it.
- The last commit adds the cube render-target test, the RUNTIME.md rows
  and these Results.

### 2. Compared against the driver

| Test | Result |
|---|---|
| `tests/frame.rs` `a_multi_pass_frame_matches_the_d3d9_driver`: two frames of G-buffer MRT (A8R8G8B8 and A16B16G16R16F textures, D24S8 depth, an indexed draw from buffers and a DrawPrimitiveUP crossing it in depth), then two additive lights into an fp16 texture, then a tone map into 4x MSAA with a blended slanted triangle, then the resolve and Present. Captured to a file through `Capture`, read back, and replayed into wgpu and the driver. | 8 of 8 targets equal as bits, no backend errors |
| `tests/frame.rs` `bisection_names_the_first_differing_draw`: two wgpu backends, one with the second light's colour changed | the report names frame 0 command 8 (that draw) and target 0x321 (the light accumulation) |
| `tests/frame.rs` `the_capture_reads_back_as_the_commands` | equal; 25 commands in 6.7 KB |
| `tests/targets.rs` `render_to_texture_matches_the_d3d9_driver`: 11 target formats; textures drawn before they are bound, then sampled with point or linear filtering and with sRGB writes and reads | 44 of 44 equal |
| `tests/targets.rs` `cube_render_targets_match_the_d3d9_driver`: six faces drawn into level 0 of a 2-level cube (A8R8G8B8 and A16B16G16R16F), each sampled | 12 of 12 equal, six distinct faces |
| `d3d9frame` unit tests: every command round-trips, a cut-short stream ends cleanly, the `DDDA_CAPTURE` switch works, difference counting and half floats are correct | pass |
| The existing driver tests (draw 12, msaa 3, targets, cache) on the `Session`-based oracle | pass |

Measured: the D3D9 driver reads back the unused X byte of an
X8R8G8B8 render-target texture as 0, whatever alpha was drawn, and
samples it as 1. The comparisons ignore that byte.

### 3. Limits

- **No game frame yet.** The boot does not present a frame. Once the
  title presents, the next step is: run `DDDA_CAPTURE=<file>
  DDDA_CAPTURE_FRAMES=<n>` with ddplay or the boot driver, then
  `frame_oracle <file> --frames n-1..n --images <dir>`.
- Multisampled targets and depth-stencil surfaces are not read back
  (D3D9 refuses), so they are compared only through their resolves and
  through the draws that test against them.
- An image upload into a render-target texture is reported as not
  implemented on both backends. D3D9 cannot lock D3DPOOL_DEFAULT
  textures, but UpdateTexture into one is possible.
- `ColorFill` into float targets is not compared yet. ddgpu writes the
  colour as byte/255.
- Bisection reads back two targets per command. Use `--frames` on long
  captures.
- Gamma ramps are recorded but not compared, because both sides compare
  the back buffer before the ramp.

### 4. Open: intermittent access violation in `ddgpu --test draw`

When `cargo test -p ddgpu --test draw` runs its tests in parallel, the
process sometimes exits with 0xc0000005 (2 of 5 runs via cargo during
this work). The same crash was seen once before L41, in the L20/L29
notes.

- Single-threaded (`--test-threads=1`), every test passes.
- Running the built binary directly, 8 of 8 parallel runs passed.
- The pre-L41 binary (L31) was also looped, but that run was cut short
  by me, so it does not show whether L41 made the crash more frequent.

Next step: catch the crash in a debugger (or with a minidump) to see
whether it happens in d3d9.dll or the NVIDIA user-mode driver, with
several devices on several threads. If it is the driver, run the
driver tests with one D3D9 device per process (`--test-threads=1` for
the D3D9 tests).

### 5. Verification

- Run here (light crates, no heavy.py slot):
  - `cargo test -p d3d9frame`;
  - `cargo test -p ddgpu --test frame --test targets --test msaa --test cache`;
  - `cargo test -p ddgpu --test draw`: passes single-threaded,
    intermittent crash in parallel (section 4);
  - `cargo test -p d3d9wgsl` (11 tests including against_d3d9);
  - `cargo test -p d3d9ref`;
  - `cargo test -p ddruntime --no-default-features --lib --test
    d3d9_device`.
- For the integration build:
  - ddplay (heavy). Its `present.rs` literals are unchanged, because
    `texture: None` stays valid.
  - The full ddruntime build.

