# L177: semantic draw-context commit and model mesh

Assigned to codex-6 by head/user, 2026-10-13. Worktree
`E:/ddda-rs-wt/L175`, branch `task/L175-semantic-draw-c`, merged
`task/fps-line` at `1fcd6330` before implementation.

Scope: FEE320, FFC350, 01145BF0; native Rust using `Pages`, isolated
original-instruction ABI vectors, opt2 lifted baseline, paired benchmarks,
and additive `DDRUNTIME_SEMANTIC=1` registrations. Head owns game runs
and shard integration. No game, GPU or shard execution by this agent.

## Implementation

- `commit_primitive_state` (FEE320): snapshot dirty context blocks,
  select shader states, compute alpha constants with guest x87 control,
  resolve shader dependencies, and compare/publish dependency packets.
  The opaque path compares the unrounded extended product; blend paths
  compare rounded single-precision values. Guest constants/tables stay
  runtime reads. Allocator, memcpy and dependency calls retain original
  argument frames and return addresses.
- `draw_model_mesh` (FFC350, uModel::vf26): select LOD and sort keys,
  filter primitives, open/close material scopes, bind static/skinned
  streams, and append indexed draw commands plus sort entries. Extended
  data absent at model +0x360 dispatches original EA5320. Arithmetic uses
  hardware SSE under the guest MXCSR, preserving operation order.
- `apply_standard_material` (01145BF0): retain the Material binding call
  and conditional MaterialStd application. This tiny wrapper has large
  inclusive cost from its callees; no independent large gain is assumed.

## Validation method

`research/native/record_l177.py` executes only original function instructions
in Unicorn over synthetic objects. x87 follows the existing `host-v1` oracle.
External callees have recorded contracts; tests compare targets, ECX for
thiscall, outgoing ESP/argument words, prior observable writes, and replay
recorded effects. These are isolated caller proofs, not callee/gameplay proof.

The L175 ABI comparator checks EAX, all callee-saved registers, ESP, every
recorded heap/global/live-stack byte, and unexpected writes. It excludes
only popped mapped stack bytes, never heap below ESP. Its mutation test
rejects deliberately corrupted heap output with correct registers.

`scripts/check_l177.py` extracts only the three lifted bodies into private
research, compiles them at opt2 with semantic mtcore at opt3, and replays
serial/atomic `Pages`. Memcpy and view-matrix local bindings forward to the
same recorded external seams for both implementations. No shards are built.

## Results

Implemented and compared against **1,664 original returns**:

| Entry | Cases | Instruction coverage |
|---|---:|---|
| FEE320 | 1,024 | 761/765 decoded: excludes two skipped alignment instructions and two unreachable FADD branches (alpha is masked to eight bits before the signed test). |
| FFC350 | 448 | 440/440 decoded instructions. |
| 01145BF0 | 192 | 17/17 decoded instructions. |

FEE320 covers all six allocator return sites, zero allocator returns,
unchanged/replaced dependency packets, both comparison loops, state saving,
unaligned contexts, NaNs and alpha boundaries. Repeated commits use valid
successful arena state; failed allocations are tested on their first call.
The float corpus covers three x87 precisions, four rounding modes, and
MXCSR rounding/DAZ/FTZ combinations. FFC350 covers base-model delegation,
culling, all LOD/filter/sort paths, equal/changed stream bindings, skinned
transforms, scope switching and command-buffer growth.

**Eleven isolated tests pass**, including original lifted replay and both
production page-access policies. Two mutation tests reject corrupted heap
state despite correct return/registers (the new one changes FEE320 dirty
flags). The unchanged L175 corpus also remains green in this harness.
Runtime light: **260 reported passes, zero failures, seven ignored**.
Whole mtcore/ddports gate with SEMANTIC=1 and JOB_LOOKUP=1: **35 reported
passes, zero failures, nine ignored**, including registration uniqueness
and semantic precedence over the older lookup port.

Final synthetic benchmark: atomic pages, opt2 lifted versus opt3 semantic,
eleven inputs per entry, seven alternating pairs, 10,000 iterations each.
Each iteration restores the same written input words and argument frame.
Call contracts are decoded outside timing; replay assertions and callee
effects remain inside timing for both implementations.

| Entry | Faster pairs | Median paired gain | Per-input median range |
|---|---:|---:|---:|
| FEE320 | 76/77 | 1.665x | 1.067–2.066x |
| FFC350 | 77/77 | 2.126x | 1.658–2.480x |
| 01145BF0 | 77/77 | 1.102x | 1.066–1.248x |

These include synthetic callee seams, not real callee cost. In particular,
01145BF0's absolute wrapper saving is small (sample medians 88.49/81.29 ns);
its large inclusive share does not imply a large whole-job gain. Earlier
JSON-decoding-per-call measurements were dominated by harness allocations,
including a slightly regressing FEE320 input; the final harness removes
that measurement artifact symmetrically. Shared-host noise remains, with
one regressing final FEE320 pair. No gameplay/image/FPS claim.

Next: head accumulated train with `DDRUNTIME_SEMANTIC=1`, then real shadow
job/image acceptance. No real FEE320 gameplay capture was available;
L162e's capture/replay tool remains available for the head's follow-up.

## Reproduction and private receipts

Run the research Python through `E:/ddda-re/tools/heavy.py codex-6 --`:

```
research/native/record_l177.py --entry fee320 --cases 1024
research/native/record_l177.py --entry ffc350 --cases 448
research/native/record_l177.py --entry 1145bf0 --cases 192
```

Then `python scripts/check_l177.py --bench`. It sets target
`C:/ddda-targets/L177`, incremental off and four cargo jobs. Runtime gate:
`cargo test -p ddruntime --no-default-features` with the same environment.
Registration gate: `DDRUNTIME_SEMANTIC=1 DDRUNTIME_JOB_LOOKUP=1 cargo test
-p mtcore -p ddports` (set environment variables using the shell's syntax).

Private files under `E:/ddda-re/notes/native-L177`:

- `00fee320.json`: SHA-256 `9a09d9a6eface4da5ffce952574d75af3f431e801698e727bbcd65d9113ab022`.
- `00ffc350.json`: SHA-256 `50972689919eeb2175ab8b61542f07692e69056b18b5056eb37a124ab5c94d0c`.
- `01145bf0.json`: SHA-256 `fbc6436c1c9c2c20509030ef879955a1973ab5ed35b6dd3a56f219f9ef2f96e0`.
- `bench-final.log`, `bench-summary.json`: paired measurements and gates.
- `runtime-light.log`, `crate-gate.log`: crate validation receipts.

Each corpus also records the original function's code hash and host-v1
oracle provenance. Extracted lifted bodies, raw research, and guest bytes
remain private; none is committed.
