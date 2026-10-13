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
Step 1 commit: `c85605a8`.

Step 2 completes InputLayout synthesis/combined declaration semantics and
NDRAW section9, with descriptor -> packed field -> D3D9 -> wgpu tables,
dynamic-state/cache rules, sampler option asymmetry and native gaps.
**3,584 original cases / 17,928 D3D9 calls** pass, including every checked
cache/payload/constructed-layout word. Four synthetic contract tests and
`cargo clippy -p mtstate --all-targets -- -D warnings` pass. Exactly256
cases for each of14 categories; finite coverage, not universal proof.

Runtime light suite: initial parallel run had a lock-timing assertion
failure (`interlocked_does_not_wait_for_the_guest_lock`). Serial
`--no-fail-fast -- --test-threads=1`: **155 passed, 1 failed, 1 ignored**;
the sole failure is the inherited fullscreen gamma Reset test,
`a_gamma_ramp_is_shown_on_a_full_screen_swap_chain_only` (returned
0x8876086C instead of0). No runtime code is changed by L117. Private log:
`E:/ddda-re/notes/native-L117/runtime-light-all.log`.

Integration boundaries: semantic API, not ABI replacement; descriptor
enum/table bounds checked; derived shader constants are restricted to
known kinds3/7,count0/1 (other forms not modeled); no GPU equivalence claim.
Bulk resources remain guest-owned. Native depth bias,
line-AA/MSAA/clipping, arbitrary border colors and MirrorOnce require the
explicit treatment documented in NDRAW; no unsupported state is tuned.

The base lacked NDRAW, so this branch restores the exact L108 document
from62fabf37 before adding section9/corrections. When merging into a line
already containing L108, retain that base and L117's appended section and
the small corrected L108 statements. Consumer updates posted to head,
claude-2/L112, codex-9/L110; no tiny-target producer work duplicated L113.

Next action: consumers integrate generation-safe native handles and the
state mappings, then head runs D3D9/native frame comparison. No game run,
GPU test, shard build, push or Ghidra mutation performed by this task.
