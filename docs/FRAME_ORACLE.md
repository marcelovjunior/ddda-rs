# Frame oracle

The frame oracle checks a whole frame. It takes the command stream the
runtime gave its graphics backend for that frame and draws it twice: once
with the host's Direct3D 9 driver and once with the wgpu backend. Then it
compares every target image by image and names the first draw whose
output differs. Brief: [L41](tasks/L41-frame-oracle.md).

Each D3D9 piece (shaders, states, formats, MSAA, render-to-texture) is
tested against the driver on its own (`crates/ddgpu/tests`). The frame
oracle is the check that they also combine correctly in a real frame.

## Pieces

| Piece | Where | What it does |
|---|---|---|
| Capture | `crates/d3d9frame` (`Capture`, `Writer`) | A `GraphicsBackend` that wraps another one. It writes each command to a file and passes it on unchanged. |
| File format | `crates/d3d9frame` (`Reader`) | See "File format" below. |
| Driver replay | `crates/d3d9ref` (`Driver`, `Session`) | A `GraphicsBackend` on the host's D3D9 HAL device. Commands run as they arrive, and device objects persist across commands, looked up by the runtime's resource numbers. |
| Comparison | `crates/d3d9frame` (`oracle::compare`, `image`) | Replays a stream into two backends, compares the targets and writes the PNG images. |
| Tool | `crates/ddgpu/examples/frame_oracle.rs` | Runs the comparison with the wgpu backend and the D3D9 driver from the command line. |

The file stores commands, not pixels: uploads, draws (with their complete
state), clears, copies, fills, Presents, queries, gamma ramps and
releases. These are the same `d3d9cmd` values the backend received.

## Capturing

Set `DDDA_CAPTURE` to a file path. Any session that installs a backend then
records to that file:

- `ddplay`, through `ddruntime::d3d9draw::set_backend`;
- a headless session, which uses the default recording backend.

`DDDA_CAPTURE_FRAMES=N` stops recording after N Presents. The game keeps
running; only the file stops growing.

```
set DDDA_CAPTURE=E:\captures\title.ddframe
set DDDA_CAPTURE_FRAMES=3
cargo run -p ddplay --release
```

Notes on the file:

- It is flushed at every Present, so a session that stops or crashes still
  leaves its complete frames.
- It records everything from the start of the session, because later
  frames depend on what the loading frames uploaded. Repeated content is
  stored once (see "File format"), so the extra frames cost little.
- It holds data from the game's files (textures, vertex data, shaders)
  and must not be committed or shared.

## Comparing

```
cargo run -p ddgpu --example frame_oracle -- E:\captures\title.ddframe --frames 2..3 --images E:\captures\title-diff
```

Options:

| Option | Meaning |
|---|---|
| `--frames A..B` | Compare frames A to B-1. Frame k is the commands up to and including the k-th Present, counting from 0. The default is every frame. Earlier frames are replayed but not compared. |
| `--last N` | Stop after frame N. |
| `--no-bisect` | Compare only at each Present. |
| `--tolerance T` | Allow a channel difference up to T, in unorm levels or as a float value. The default is 0, which requires equal bits. |
| `--images DIR` | Write each compared target as three PNGs: `wgpu`, `d3d9` and `diff`. |

The exit status is 0 when everything compared is equal, 1 when something
differs, and 2 for a usage or file error.

The D3D9 device cannot be created while a full-screen application holds
the adapter. CreateDevice then returns D3DERR_DEVICELOST, and the tool
stops with status 2.

### What is compared

At each Present in a compared frame, the oracle compares:

- the back buffer;
- every colour target the frame wrote: render-target textures,
  standalone render targets, and the destinations of copies and fills.

Each is read back from both backends as D3D9 stores it (GetRenderTargetData
layout and bits) and compared. The report lists each target as equal, or
gives the number of differing pixels, the largest channel difference, and
the first differing pixel with both values.

Two kinds of surface cannot be read back, because D3D9 refuses to:

- multisampled targets, which are checked through their resolve;
- depth-stencil surfaces, which are checked through the draws that test
  against them.

### Finding the first differing draw

Bisection is on by default. In a compared frame, the oracle reads back the
targets of every draw, clear, copy and fill on both backends right after
the command runs. The report names the first command that leaves a target
differing when that target was equal before it, in this form:

```
first difference: frame 0 command 6 (stream 9), draw: primitive type 4 x1, vs 3f2c… ps 91aa…, targets [0x321:113 - - -] depth -, textures [s0=0x300:21 s1=0x310:113]
  target 0x321 (format 113): 256 of 256 pixels differ, max 0.75, first at (0, 0): [...] vs [...]
```

The `vs` and `ps` values are hashes of the shader tokens, so the shaders
can be found again. Before a target's first write in a frame, the oracle
also reads it once. A target that already differs at that point (for
example, a texture left over from an earlier frame) is listed under
"already differing" and not blamed on this frame.

Bisection costs two read-backs per command. Limit it with `--frames` on
long captures.

## File format

A capture file has this layout:

- an 8-byte header, `DDFRAME\0`;
- a version word (1);
- a sequence of records. Each record is a tag byte, a length word and a
  payload.

There are two kinds of record:

- **Blob** (tag 1): a number, the raw length, then the deflated bytes.
  Blobs are numbered 0, 1, 2… in the order they appear.
- **Command** (tag 2): a kind byte, then its fields as little-endian
  words. A command refers to its large parts by blob number.

These parts of a command are stored as blobs:

- texture and buffer bytes;
- shader tokens;
- constant blocks;
- render-state lists;
- vertex declarations;
- inline vertices and indices;
- gamma ramps.

A blob is written once, the first time its content appears. The writer
identifies content by a 128-bit hash (two SipHash keys), so content that
repeats across draws and frames costs one word per use.

The reader keeps every blob in memory and decodes one command at a time. A
record cut short at the end of the file ends the stream at the last
complete command. Each `d3d9cmd` field has a round-trip test in
`crates/d3d9frame` (`every_command_reads_back_as_written`).

## The driver side (`d3d9ref::Driver`)

`Driver` is the D3D9 side of the comparison:

- Device objects live in a `Session`, looked up by the runtime's resource
  numbers.
- An image upload replaces that texture level. The texture is created
  again from its images when it is next used.
- A buffer upload replaces the buffer's bytes.
- Render-target and depth-stencil textures are created in
  D3DPOOL_DEFAULT with their usage. Their levels are taken with
  GetSurfaceLevel or GetCubeMapSurface.
- Draws and clears run inside BeginScene/EndScene. Copies, fills and
  read-backs run outside it.
- Shaders, declarations, buffers and queries are cached.
- `release` drops a resource's objects.

The older batch calls (`Oracle::render*`, used by the per-feature tests)
now run on a `Session` of their own, so the per-feature tests also cover
`Driver`.

## Tested

- **`crates/d3d9frame`**:
  - every command round-trips through the file;
  - a stream cut short ends at the last complete command;
  - difference counting and half-float decoding are correct.
- **`crates/ddgpu/tests/frame.rs`**:
  - the synthetic frame read back from its capture equals the commands;
  - `a_multi_pass_frame_matches_the_d3d9_driver`: a G-buffer pass (MRT
    with A8R8G8B8 and A16B16G16R16F render-target textures, D24S8 depth,
    an indexed draw from buffers and a DrawPrimitiveUP crossing it in
    depth), two additive lights into an A16B16G16R16F texture, a tone map
    into a 4x multisampled target with a blended slanted triangle, the
    resolve and Present, over two frames;
  - `bisection_names_the_first_differing_draw`: two wgpu backends, one
    with the second light changed, and the report names that draw and
    the light accumulation target.
- **`crates/ddgpu/tests/targets.rs`**
  `render_to_texture_matches_the_d3d9_driver`: render-target textures of
  every target format are drawn into before they are bound, then sampled
  with point and linear filtering and sRGB reads and writes. Both the
  level and the sampled values are compared with the driver.

Results and limits are in the [L41 results](tasks/L41-frame-oracle.md#results).
