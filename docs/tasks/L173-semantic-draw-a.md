# L173: semantic model draw leaves

Assigned to codex-9 by head on 2026-10-13, branch
`task/L173-semantic-draw-a`, based on fps-line plus L171 template.
Scope: FF1920, FF14D0, then FEFB90; original ABI vectors and isolated
lifted-body timing. No game runs, shard builds, pushes or game-file changes.

## Implementation

`mtcore::semantic_draw_a` uses `Pages` and native loops over guest objects:

- `shadow_draws` (FF1920): optional shadow material/sampler/dependency
  records, shader binding save/restore, and append copies of draw records.
- `replay_draw_passes` (FF14D0): reverse change-log replay and material
  validation for each extra pass; duplicate draws or emit rejection records.
- `bind_render_target` (FEFB90): target binding/timestamps, runtime remap
  table, dimensions and viewport constants. The original consumes four
  argument words, including its unused fourth word.

Shared context: allocation cursor/end +24/+28; draw-slot bank/count +44/+48;
pass/key +164; binding data +1f8 (8-byte entries); change log +30c7c
(12-byte entries, count +6c). Draw payload tags 0..3 have 28/32/36/40
copied bytes in 32/48-byte allocations. Copies keep MOVQ grouping and
REP MOVSD direction. Shader/remap tables are read from guest memory.

External callees remain guest calls with original argument frames and return
sites. FF1920 retains extended x87 arithmetic where rounding before the
comparison/division would change behavior. FEFB90 uses scalar SSE operations
under guest MXCSR. No flags or volatile-register instruction emulation in
the integer loops; incidental final EAX is nevertheless checked exactly.
All three registrations are under `DDRUNTIME_SEMANTIC=1`.

## Results

Implemented and tested in isolation, not compared in the running game:

- **480 original returns**, 160 per function: all four rounding modes,
  zero/one/four/twelve records, early returns, cached states, pass/material
  variants, reject paths, all draw tags plus unsupported tags, allocator
  success/failure, shader null, both viewport paths, remapping, zero and
  maximum dimensions, and quiet NaN. One FF1920 variant uses DF=1.
- **Seven isolated tests pass**: semantic and unmodified lifted bodies
  against originals, plus both bodies on production serial/atomic memory.
  Final validation checks EAX, EBX/ESI/EDI/EBP, ESP, FP control, call ABI
  and memory, and all written memory except discarded stack bytes. Heap
  below ESP is explicitly included; extra heap writes fail.
- Runtime light: **255 passed, zero failed, seven ignored**.
- Full `mtcore`/`ddports` suite: **25 passed, zero failed, nine ignored**.
  Follow-up oracle mutation test corrupts a below-stack heap byte separately
  for each port and proves all three corruptions fail; the four focused
  leaf/oracle tests pass (the full suite preceded this additional test).
- Paired production atomic-Pages timings, 200 iterations, seven alternating
  rounds, four 12-record scenarios per entry; reset work outside timers,
  identical synthetic callees. Lift opt2 / port and memory opt3. Median
  ratios across scenarios: FF1920 **1.07–2.02x** (23/28 pairs faster),
  FF14D0 **2.25–2.56x** (28/28), FEFB90 **1.48–1.67x** (27/28).
  Shared-machine noise is visible; these are isolated leaf costs, not FPS.

Private artifacts: `E:/ddda-re/notes/native-L173/`, files `00ff1920.json`,
`00ff14d0.json`, `00fefb90.json`, `bench-final.log`, `runtime-light.log`.
Corpus SHA-256 respectively:

```
f26df10a58e007df3426ad2c21453e6139c565399e90e1b193c0948786e69598
eaf21e81f8e2da641b431cdcad78d8416edc0765031a8f8ad4c94ae3abd17297
b2c4110e2a97aa85acf3a86e7290eb45370a07ab61eb0905cf7218b72f1043ac
```

Reproduce with research Python through heavy.py:
`research/native/record_l173.py --entry ff1920` (also ff14d0/fefb90), then
`python scripts/check_l173.py`; set `L173_BENCH=1` for paired timings.
Only the three bodies are extracted into private scratch for compilation.

Limit: synthetic callee effects, not recorded gameplay state. Unicorn
preserves a signaling NaN in x87 FSTP where hardware and the unmodified
lift quiet it. That diagnostic is excluded from the accepted corpus and
requires a hardware oracle; neither production arithmetic nor the equality
checks were changed to accommodate it. Quiet NaN remains covered.
Next: head integration train and matched image/per-job acceptance.

## Priority follow-up from head (#14007)

Record EA5320 draw-mesh vectors to `native-L171/draw_mesh.json` and run the
head's `semantic_draw::draw_mesh` in isolation.

`record_l173_mesh.py` records 302 cases across fixed/dynamic LOD, quality
1/2/3, sort modes 0x10/0xe/3, primitive depth, bounds callback, pass filters,
material switches, command-buffer growth, flags 0x100/0x50 and both early
exits. Different primitive LOD masks make selection observable. External
material/grow seams have explicit recorded heap effects and randomized
volatile returns. No game data is copied into the repo.

`check_l173_mesh.py` snapshots a pinned commit from `task/L171-semantic`
into private scratch and runs its ABI harness with mesh call arities and
the below-stack heap fix, then L173's stricter per-call memory/sphere check.
An additional isolated lifted-body replay validates the recorder itself.
EA5320 is void, so incidental EAX is excluded here as in the head's harness.

Initial 302-case replay stopped on an unrecorded incoming argument for an
early-return path. The recorder now includes the complete known incoming
ABI frame (28 bytes) instead of only arguments read by that original path.
Corrected recording and final replay are waiting for the head's paused
game comparison; no semantic equivalence claim yet. Head source compiled
at `fb948fc26080c6bbc3272979ff5f093bfc94ddcc`.
