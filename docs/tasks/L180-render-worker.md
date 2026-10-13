# L180: the render worker's per-draw cost

Owner: claude-2. Branch `task/L180-render-worker`, from `task/train5`.
Assigned by the head (#14285, #14334). With the semantic rewrites on, the
main thread waits on the render worker (guest thread 0x1034). With
`DDDRAW_NATIVE_TARGETS=all`, the native sink takes nearly every draw:
verify mode found 0 mismatches over ~5.4M compared draws in combo38.

## Measured before (combo37/38 profiles of the render worker)

| Item | combo37 | combo38 (all targets) |
|---|---|---|
| sink `command` | 39% | 74% |
| `draw_call` | 30% | 54% |
| `begin_frame` table clear | — | 9.2% |
| BindingPart drops | — | ~12% |
| `Mirror::textures` | — | 7.6% |
| guest reads (`fast_u32`) | — | 8.4% |
| D3D9 path for the draws the sink left (applies, services, snapshot) | ~45% | — |

## Done (each tested; not yet run in game unless noted)

- **4ca0e76c (cherry-picked d3dab094).** No Vec per sampler change in
  `shadow_samplers`. A changed binding keeps the unchanged stage's bank.
  `flush_uploads` keeps its capacity.
- **5c5c3b99.** Stream and target parts are kept across frames: rebuilt
  the first time a frame uses them, and the earlier `Arc` kept when equal.
- **efebe1d8.** claude-9's inline reads in the sink's guest reader
  (1b843386).
- **d9ad063d.**
  - Float uploads are read 16 registers at a time, one page check per page.
  - A changed constant bank moves into the part instead of being copied.
  - A taken draw makes one dirty pass instead of two.
  - `run_service_as` looks up names only for an enabled diagnostic.
  - Merged into train5.
- **09220b1d.**
  - Binding parts and texture lists are also kept by content, confirmed by
    a full compare. The game's bindings move between frames, but their
    content mostly does not.
  - The binding, content and texture-list tables evict entries unused for
    three frames, instead of clearing everything at 16,384.
  - `mtstate::packed_each` replaces the Vec of calls per sampler change.

Release `NDRAW_BENCH`, single binding:

| Case | Before | After |
|---|---|---|
| `draw_call` from the caches | 0.34–0.41 µs | 0.20–0.30 µs |
| a new frame's first draw | 2.5–2.7 µs | 1.31–1.43 µs |

The content caches help bindings that move between frames, which this
single-binding bench does not exercise.

## Next

- The head's train 6 profile.
- The remaining per-draw guest reads.
- The per-draw `objects()` dirty check: a lock-free "nothing dirty" test
  needs a counter in d3d9dev (claude-9's file).

## The combo39/42 verify mismatches (fixed in 3d2417f4)

- combo39 had 1 mismatch (ps c4) and combo42 had 4 (vs c9). fefc60d2's
  verify context showed what they were.
- In combo42, all four were a written register (not an uncovered one).
  The binding came "from this frame's cache", and the records shared one
  binding.
- The binding had been used earlier in the frame and was used again after
  other bindings. Its constant source had changed in between. The
  original's apply (0x00DAA8B0) re-reads the values whenever the context
  holds another binding. The sink had reused its first read of the frame.
- Fix: the decoded part is reused only by the draw right after. A use after
  another binding, a clear, a copy or a nested list reads the values again.
  The content cache still avoids allocating when they did not change.
- Oracle test: a binding used again after another one in the same frame
  sees its new values; the consecutive draw keeps its part.

## The original's upload skipping (cb495a30, verified in combo46)

- combo44 still had a ps c4 mismatch on a written register, marked
  "decoded for this draw". It is the original's upload skipping in
  0x00DAA8B0, as `mtstate::shader` decodes it:
  - the same binding as the context holds: nothing is uploaded;
  - after a binding with the same vertex shader (and pixel shader, for the
    pixel stage), an upload whose value slot holds the same pointer is
    skipped, so the device keeps the earlier values even though the memory
    changed.
- The sink now models this. The previous apply is the sink's model after a
  decided draw, else the executor context's +0x10/+0x24/+0x28.
  - Same binding: the previous draw's constants.
  - Skipped registers: the previous draw's constants, else the device's.
  - A per-draw part only when held values differ.
- combo46 verify: 0 mismatches over the run, but 4.2–4.8 µs per decided
  draw.
- ca5a50d6 brings the cost down:
  - copy-on-write banks: no clone when the held or uncovered values equal
    the part's;
  - texture lists found by slots, textures and states, with a per-frame
    slot check.
