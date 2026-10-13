# L182: Clears and StretchRects on the native path

Owner: claude-9. Branch `task/L182-native-clear-copy`, from
`task/L180-render-worker` fefc60d2 (claude-2's latest ndraw_native).
Assigned by the head (#14316, #14379) with claude-2 (#14373): move the
D3D9 path's remaining commands onto the native sink, so the D3D9 path ends
up a fallback/oracle. claude-2 owns `draw_call`'s cost; this task owns
coverage. claude-2 stays out of `decide()` and the verify code until this
lands.

## What the D3D9 path still did (census, run_c39sync.log, per frame)

| Site | Command | Count |
|---|---|---|
| 0x00DABD02 | executor type 4: Clear | 15 |
| 0x00DABF2A / 0x00DABF7B | executor type 5: StretchRect | 8 |
| 0x00DABD8C | DrawPrimitive: type-0 draws **and** type-4 fallback draws (both return there) | 11 |

## Done (tested in isolation; not yet run in game)

- `crates/ddruntime/src/ndraw_ops.rs` (a child module of ndraw_native):
  - `clear_args`: translated from 0x00DAB480's type 4 (0x00DABAF0..
    0x00DABD02). It covers the colour bytes (SSE, as the original), the
    exactness check (x87), the write mask against the target's channel
    count, the D3DCLEAR flags and the D3DCOLOR. A record whose colour
    cannot be cleared exactly has its fallback draw (+0x20, a type-0
    record) decided as a type-0 draw.
  - `copy_args`: translated from type 5 (0x00DABEA5..0x00DABF7B). It
    gives a rectangle of the binding's target `+0xC` to (+0x20, +0x24) of
    `[[+4]+0x44][+8]`, or the whole of target 3 to `[[+4]+0x48][+8]`.
  - `Native::clear` and `Native::copy` send the ClearCall / CopyCall in
    order with the taken draws (`g.send()` first). A copy flushes its
    source first, as the D3D9 StretchRect does.
  - Verify mode: `expect_op` and `observe_op` (hooked into
    `d3d9draw::clear` and `d3d9copy::stretch_rect`) compare whole
    commands. The report adds a "verify clears/copies" line, and every
    report adds a line with the clears and copies taken, the fallbacks and
    the refusals.
  - `DDDRAW_NATIVE_OPS=0` leaves types 4 and 5 to the D3D9 path.
- `mtdraw::sink::x87_control()`: the guest's x87 control word as the
  executor began the frame (one read per frame, only with a sink).
- `decide()` is split: kinds 4 and 5 go to ops, and the draw part moves
  unchanged into `Native::draw`.

## Finding: the Clear's alpha check depends on x87 precision

The original compares `alpha_byte * (1/255)` (FMULP) with the alpha float
by FUCOMIP, without rounding the product to single. At 53- or 64-bit
precision the product is exact, and `255 * f32(1/255) != 1.0`. So an
opaque colour clear (alpha 1.0) never clears: it makes its fallback draw.
At 24-bit precision, which D3D9 sets on CreateDevice without
D3DCREATE_FPU_PRESERVE, it clears. The sink uses the guest's live control
word, so it matches our D3D9 path (the oracle). Whether the original runs
at 24 bits depends on DDDA's CreateDevice flags (not checked; Ghidra is
down). This may also be where many of the 11 "type-0" draws per frame
come from.

## Not covered / known differences

- The executor merges a following identical copy into one StretchRect.
  The sink copies the same rectangle twice, which gives the same result.
  In verify mode the second copy shows as "not made by the D3D9 path".
- The Clear's targets and states are read from the fallback draw's
  bindings. This rests on the same assumption as `draw_call`: the device
  holds what the last apply of each object set. Verify mode checks it.
- `APPLY_TARGETS` (0x00DA6FE0) storing the binding's views as the
  context's targets (the copy source) is inferred from `target_part` and
  the executor's reads, not read from the function. Verify mode checks it.

## Results

- Also here: claude-2's `dirty_count()` guard (#14395) in `decide`'s
  DrawCall and Compare arms (cherry-picked 26244237 from L144). Both arms
  run under `device_lock`, so the count is exact there.
- Tests: 7 new `ndraw_native::ops` unit tests (colour packing, exactness
  at 24 and 53 bits, masks, flags, a quieted SNaN z, the rounding mode
  refused, null fallbacks, both copy forms, verify counting). All
  ddruntime lib tests pass (146) and all mtdraw tests pass (executor
  differential included).
- Not run in game: needs `DDDRAW_NATIVE=verify` (the "verify
  clears/copies" line) and a stats run from the head.
