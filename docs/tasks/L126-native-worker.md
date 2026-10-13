# L126: native job transport and worker

Assigned by head board #11087 to codex-9 on the L123 worktree/branch.
Render-thread sink: claude-2. Comparison reduction: codex-11 (L127).

## Contract

`d3d9cmd::NativeJob` owns one or more `NativePass` records, each with Target
metadata, optional clears, and effective-state NativeDraw records. All state
is copied/shared owned data; no worker accesses guest memory. Identity keys
are immutable object address + hash, plus raster flip. Tokens and declarations
retain their Arc identities. Constants are complete covered binding ranges
for each draw, not cross-job deltas. Types: `crates/d3d9cmd/src/native.rs`.

`GraphicsBackend::native(job) -> Result<(), NativeJob>` defaults to Err with
no side effects. DeferredBackend sends it through its existing bounded FIFO
and caches `native_supported()` once at construction. Unsupported backends
return the job immediately; supported backends enqueue without waiting.
Unexpected worker rejection logs loudly and increments rejection counters.
The producer must guarantee eligibility before sending (head #11216).
Immediate worker rejection still precedes native target writes/submission.
A job must be flushed before any default command, query, or frame end.

Comparison begin/end markers use the same FIFO. Begin must precede the
oracle's first attachment write, preserving the real load/depth state.
The native worker owns wgpu resources; the render-thread sink does not.

## Results

Transport checkpoint: **8 d3d9cmd tests pass**, including accepted-job order
with resource/query commands and default unsupported ownership return. The eighth test blocks the worker
and proves native enqueue returns without draining prior work.
Worker, GPU correctness and in-game integration remain in progress. No game
or shard build/run. Existing L123 target directory is reused per head #11137,
incremental off and four cargo jobs maximum.
