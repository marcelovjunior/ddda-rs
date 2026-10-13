# L117: nDraw state semantics

Owner: codex-10. Branch: `task/L117-state-semantics`, worktree L117,
base `task/fps-line`. Assigned by head/user on 2026-10-13.

Scope: RasterizerState, DepthStencilState, BlendState, SamplerState,
InputLayout, shader metadata, target/depth views and stream bindings.
Deliver descriptor/packed field mappings and D3D9-to-wgpu guidance in
NDRAW; original-instruction Unicorn recording generator; independent Rust
decoder and differential tests. Coordinate consumers claude-2/L112 and
codex-9/L110 through the shared board. No game runs or shard builds.

Files: `crates/mtstate/**`, `research/native/state_vectors.py`,
`docs/NDRAW.md`, this brief. Narrow Cargo.lock package addition and
MILESTONES paragraph agreed with codex-9 (board10798).

## Results

Step 1: standalone call-level decoder and private oracle, **1,536 cases /
7,207 recorded D3D9 calls**, all calls and checked cache/payload words equal.
Covers four constructors, four packed-state applies, fixed bindings,
streams, targets, shader bindings. Constructor vectors exposed the exact
raster multiplier `0x33800001` and sampler max-mip truncation.

The decoder is not a register/stack ABI port and is not hooked into runtime.
Pointer arguments to D3D9 are compared by copied contents; the COM `this`
and transient stack addresses are normalized. Ghidra read-only; vectors
are private at `E:/ddda-re/notes/native-L117/state-vectors.json`.
InputLayout, complete mapping tables and expanded cases are in progress.
No GPU, FPS or image-equivalence claim.
