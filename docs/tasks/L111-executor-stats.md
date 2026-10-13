# L111: nDraw executor statistics (NR-B)

Owner: codex-11. Branch: `task/L111-executor-stats`, based on
`task/fps-line`. Runtime instrumentation only; no shard build or game run.
Scope: new `ddruntime/src/ndraw_stats.rs`, narrow registration/Present hooks
in `dispatch.rs`, `lib.rs`, `d3d9dev.rs`, this brief and milestone entry.
Shared hooks agreed with codex-6/L109 and codex-7/L90 on the board.
NDRAW section coordinated with codex-10, who owns that document.

## Command format evidence

Read-only Ghidra decompilation and disassembly, DDDA.exe 2017-12-13 Steam.
No decompiler exports, executable bytes or game records are committed.

`0x00DAB480` receives sRender in ECX and returns with `RET` (no stack
arguments). `0x00DA6DA0` initializes its local execution context: slot is
`u32(sRender+0x33DB8)`, XOR 1 when byte `sRender+0x26` is nonzero. Root
entries are `u32(sRender+0x433DD8+slot*4)`; count is
`u32(sRender+0x433DE0+slot*4)`. Device is `u32(sRender+0x98)`.

The cursor advances eight bytes per entry. Entry `+4` points to the
command record; entry `+0` is not dereferenced as a record pointer.
The type is `u16(record+2)&15`. Type 6 pushes a child range; an exhausted
range returns to its parent. A zero cursor terminates the executor.

| Type | Payload offsets from record | Executor operation |
|---|---|---|
| 0 | +4 target group, +8 fixed-state group, +C shader binding, +10 stream group, +14 element count, +18 start vertex | DrawPrimitive |
| 1 | type 0 fields; +18 start index, +1C base vertex | DrawIndexedPrimitive, optionally coalesced |
| 2 | type 0 fields; +1C instance count, +20 instance start | stream frequencies/offsets around DrawPrimitive |
| 3 | type 1 fields; +20 instance count, +24 instance start | stream frequencies/offsets around DrawIndexedPrimitive |
| 4 | +4 clear mask, +8 depth, +C stencil, +10/+14/+18/+1C RGBA floats, +20 fallback draw pointer | Clear, or fallback draw if mask/colour exactness test fails |
| 5 | +4 destination texture, +8 subresource index, +C source selector low nibble, +10..+1C source rectangle, +20/+24 destination XY, +28 draw-context pointer | StretchRect; adjacent matching records may be suppressed |
| 6 | +4 child base/context value, +8 entry pointer, +C entry count | nested list |
| 7 | +4 selector, +8 Query pointer | nonnull Query in ECX, calls 0x011481B0 or 0x011481F0 |
| 8..15 | no payload consumed by executor switch | ignored |

Types 0/1/2/3 occupy 28/32/36/40 bytes, independently corroborated by
the CommandCache copier `0x011F0ED0` in L108. Topology is
`(u32(shader_binding)>>21)&255`; element-to-primitive conversion and
indexed coalescing remain entirely in the original executor.

Object references read for statistics:

- `0x00DA6FE0`: target group +0..+C are four RTV pointers, +10 DSV;
  each nonnull view's +14 is a D3D surface handle. These match ddframe
  `Target.surface`, unlike transient group identities.
- `0x00DAA5F0`: fixed-state group +8/+C/+10 are blend/depth/rasterizer
  objects (dynamic factors/references are not object identities).
- `0x00DAA7C0`: stream group +0..+C are four VB object pointers;
  +10..+13 strides, +14..+20 byte offsets, +24..+27 instance flags,
  +28 IB pointer. Buffer object +20 is its D3D handle.
- `0x00DAA8B0`: binding +8 is metadata; metadata +10/+14/+18 are
  VS/PS/declaration D3D handles. Metadata +4 points to descriptors;
  VS descriptor count is low byte of +8, and sampler counts are bits
  8..11 of +8 (VS) and +C (PS). Sampler descriptors lead each stage;
  bits 12..21 index a sampler pointer, bits 22..31 index a texture pointer
  in binding's dword array beginning +C. PS descriptors follow all VS
  descriptors. Only these resource pointers are counted; constant payloads
  are not traversed.

## L111 instrumentation (section for NDRAW.md)

Set `DDRUNTIME_NDRAW_STATS=1` before startup. The default is off.
`DDRUNTIME_NDRAW_STATS_EVERY=N` prints every Nth successful Present
(default 60; invalid/zero values use 60). Each line describes only that
frame, not an N-frame accumulation. Use N=1 for capture correlation.

The executor wrapper is installed through `ddports::ensure_with` before
the override table freezes. It retains an existing port/mod at DAB480,
or the raw lifted body when there is no override, and calls that exact
function directly after observing the list. No guest writes, register
changes, replacement draw logic or swallowed execution faults are added.
L112's future executor port can therefore compose beneath the observer.

Present wrappers are chosen once when the COM registry is built for
Device9 slot 17 and SwapChain9 slot 3; only successful calls close a
frame. Disabled instrumentation installs neither executor nor Present
wrappers: no new per-executor, per-command or per-Present branch/read.
Enabled statistics are grouped by device and synchronized across host
threads; no statistics lock is held while executing the original body.

The single stderr line begins `ndraw stats frame=...: cmds=...` (with a
device field) and contains a 16-entry `by_type` array indexed by command
type, raw draw-record/element totals, distinct fixed-state groups and
objects (including samplers), shader binding groups/VS/PS/declarations,
samplers/textures, stream groups/VB+IB objects, draws per five-surface
target tuple and the ordered target runs with draw/clear/copy records.
Pointer counts are identities within a frame, not content deduplication.

These are **submitted records**, not post-coalescing API draw counts.
Type-1 coalescing, type-4 Clear fallback draws, and type-5 copy suppression
explain differences against a D3D capture. Clear records count as clears
even if the executor later implements one using a draw. The reported
target runs can include clear/copy-only runs; frame_stats pass rules may
split or omit these differently. Compare ordered surface tuples and raw
type mix before interpreting a count difference as a decoder error.

Reads are probed; bad/null pointers, arithmetic overflow, depth above 256,
or more than one million visited records stop only observation and mark
the summary with `errors`. The limits are diagnostic safeguards, not new
game constraints. Counts in such a frame are partial. Synthetic tests
cover layouts, traversal, identity sets, sampler descriptors, frame reset,
malformed input and CPU/memory preservation around a stub original.

## Results

- Prerequisite `83ff382c`: cherry-pick of L109's `bad9032c`, composable
  `ddports::ensure_with`. Head should keep only one copy of that patch.
- Implemented decoder and opt-in wrappers. **Seven synthetic tests pass**;
  enabled light serial unit suite **35 pass / 1 existing ignored**.
- Full-suite validation is in progress. The fullscreen gamma Reset test
  fails with D3DERR_INVALIDCALL with instrumentation both ON and OFF using
  the current desktop. With the private L34 recorded display it passes
  (all five device tests pass); the loaded-machine repeat instead fails
  two existing lock-timing assertions in threads.rs. No checks weakened.
- No game/shard/GPU execution, gameplay equivalence or FPS claim.
- Next: commit tested instrumentation, head-run stats with EVERY=1 and
  gameplay capture; run L107 frame_stats and compare target runs/counts.
