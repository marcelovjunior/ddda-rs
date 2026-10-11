# L20: Direct3D 9 completeness (device methods, fixed function, queries, state coverage)

- **Owner:** claude-5
- **Branch:** `task/L20-d3d9-completeness` (worktree `E:\ddda-rs-wt\L20`)
- **Status:** open

## Why

The D3D9 layer (`crates/ddruntime/src/d3d9*.rs`, the backend command crate
`crates/d3d9cmd`, the wgpu backend `crates/ddgpu`, the driver oracle
`crates/d3d9ref`, the shader translator `crates/d3d9wgsl`) renders shader
draws pixel-exact against the host's real D3D9 driver. Not done yet:

- IDirect3DDevice9 slots without an implementation (calls stop with the
  method name): 10 SetCursorProperties, 11 SetCursorPosition, 12 ShowCursor,
  16 Reset, 19 GetRasterStatus, 20 SetDialogBoxMode, 21/22 Set/GetGammaRamp,
  33 GetFrontBufferData, 46 MultiplyTransform, 49-54 materials and lights,
  55/56 clip planes, 59-61 state blocks, 62/63 clip status,
  70 ValidateDevice, 71-74 palettes, 77/78 software vertex processing,
  79/80 N-patch mode, 85 ProcessVertices, 115-117 patches.
- Draws with no vertex and/or pixel shader bound (the fixed-function
  pipeline: FVF, transforms, lighting, texture stage states).
- Occlusion queries: `d3d9copy.rs` answers event/timestamp queries; an
  occlusion query's GetData stops ("not measured yet").
- Render-state coverage against the driver: only clears, constant-colour,
  textured and simple blended draws are compared so far.

## Task

1. **Census first** (into Results, with addresses): which of the missing
   slots the game calls, whether it draws without shaders (SetFVF,
   SetVertexShader(NULL)/SetPixelShader(NULL) before draws), whether it uses
   occlusion queries, and which render/sampler states and formats it sets.
   The device is reached through the MT Framework renderer's wrappers. Use
   Ghidra over HTTP (`curl "http://127.0.0.1:8089/decompile_function?address=0x..."`,
   `/disassemble_function`, `/get_xrefs_to`) and the lifted code. A note:
   a naive scan for `call [reg+slot*4]` finds nothing, because the vtable
   load is far from the call. Work from the wrappers.
2. **Implement what the game uses, in the order it is reached**, each with
   D3D9's documented semantics and a test:
   - Device methods in `crates/ddruntime/src` (new module(s) next to
     `d3d9state.rs`/`d3d9copy.rs`; register slots in `d3d9dev.rs`
     `INTERFACES`).
   - State blocks: while recording (Begin/EndStateBlock), Set* calls go
     into the block and do not change the device (Wine's
     `update_stateblock_state` model).
   - Reset: resources in D3DPOOL_DEFAULT must be released first, otherwise
     D3DERR_INVALIDCALL; the swap chain and back buffer are rebuilt with
     the new present parameters.
   - Gamma ramp: feed it to the backend's present.
   - Methods the game provably never calls stay unimplemented. List them in
     Results with the evidence.
3. **Fixed function** (only if the census shows the game uses it):
   generate WGSL from the fixed-function state, in `d3d9wgsl` or `ddgpu`,
   and check it against the driver through `d3d9ref` like the shader draws.
4. **Occlusion queries** in `ddgpu` (wgpu occlusion query sets resolved
   when the guest asks; the count semantics against the driver), if the
   game uses them.
5. **State coverage against the driver**: extend `ddgpu/tests/draw.rs` and
   the fuzz (`DDGPU_FUZZ`) so every render state, sampler state and texture
   format the census lists is exercised and compared with `d3d9ref`:
   stencil, depth bias, colour write masks, blend ops and separate alpha,
   cull and fill modes, scissor, sRGB read/write, and the formats. Fix
   what differs. Record anything that cannot match exactly and why.

## Rules

- Read first: `AGENTS.md`, and `docs/RUNTIME.md` (implementation status,
  "Graphics backend design", findings).
- Never invent behaviour: the D3D9 docs, the SDK headers and the driver
  oracle decide. Cite them in Results.
- Lifted game code is never replaced. No special cases by address.
- The game install is read-only.
- Builds and memory (the machine runs five agents):
  - Always `CARGO_INCREMENTAL=0`, and put your target dir on C:
    (`CARGO_TARGET_DIR=C:/ddda-targets/L20`).
  - `ddgpu`, `d3d9wgsl`, `d3d9ref` and `d3d9cmd` are light: use
    `CARGO_BUILD_JOBS=4`.
  - Anything that builds `ddruntime` (it pulls the huge `ddnative` crate)
    uses `CARGO_BUILD_JOBS=2`, and only when at least 8 GB of RAM is free.
  - Batch your `ddruntime` edits, so it rebuilds rarely.
- Another agent (claude-2, L17A) edits `crates/ddruntime/src` for Windows
  services (kernel32/user32 and friends). Keep your changes to the D3D9
  files and small registrations, to avoid conflicts.
- Keep your prompt cache warm: never sit inside one tool call for more than
  a few minutes; run long jobs in the background with output to a file and
  poll it.
- Do not spawn subagents.
- Commits on your branch only, as
  `git -c user.name=janne -c user.email=marcelovjunior55@gmail.com commit`,
  with no AI co-author trailer. Never push. Never commit game data, anything
  from `E:\ddda-re`, or decompiler output.
- Fill in Results, update the `docs/RUNTIME.md` rows, and commit. End your
  run with a short summary for the head.

## Results

(open)
