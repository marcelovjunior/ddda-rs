# L156: exact inflater throughput and residual attribution

Owner: codex-6. Branch: `task/L156-inflate-fast`, base `f0019a20`
(`task/fps-line`). Runtime only; no game window, shard build or push.

## Brief

Optimize the exact `0x00cf2cb0` leaf, preserving output, z_stream/state,
window, hold/bits, mode, stack/register effects and input/output return
boundaries. Use the 632 leaf fixtures and 217-stream / 12,893-return
original-instruction oracle. Attribute the 1.376424 s outer/other residual
from the head's c25 save-load interval, port the measured hot part, and
provide ARC throughput and hashes for a head-run loading A/B.

## Implementation

- `DDRUNTIME_ZLIB_FAST=2` is the new candidate; FAST=0/1 retain their
  existing selection and explicit overrides win. All state stays in guest
  memory between calls. Bounded root-table and literal-output page spans,
  packed input pairs, aligned eight-byte forward matches, short-distance
  periodic seeds, and deferred final stack spills target access overhead.
  Original symbol decisions and return guards remain unchanged.
- The original Huffman format is a packed four-byte op/bits/value entry.
  The candidate reads it directly; no persistent re-encoded tables or
  host decoder object is introduced.
- `DDRUNTIME_ZLIB_TABLE=1` independently ports whole `0x00cf30c0`, including
  malformed/oversubscribed/incomplete trees and table-capacity errors.
  Sources: original instructions and read-only Ghidra for DDDA.exe
  2017-12-13 Steam. Embedded base/operation arrays are read from the
  player's mapped image. The local stack frame snapshots the six cdecl arguments, then preserves
  written bytes, argument spills and final registers; no allocation or other callee.
- Existing load logging gains `zlib_table_ns` in transition intervals and
  the `0x00cf30c0` helper total. It remains logging-only, preserves explicit
  overrides, and makes future real-stage residual subtraction possible.

64-bit speculative input staging and batched literal writes were tested
and rejected after regressions on the ARC samples. They are not claimed
as wins. Neither opt-in changes the default. The wide access path checks
PREEMPT at each match and retains the page-table handover when requested.

## Results

- Final parallel-policy oracle: **217 streams / 12,893 returns PASS**
  with FAST=2 + TABLE=1 and the original outer body. Every return checks
  GPRs, XMMs, defined flags, full stack/state/window and output. Final DLL
  SHA-256 `e57362526759eba126fa00ce004efd3f824abc58eb4151ddb23e25007b23ccf1`.
- **632 leaf fixtures x five configurations PASS** (old scalar; L145
  sparse/pages; FAST=2 sparse/pages), with atomic accesses enabled.
- **81 table-builder original-instruction fixtures PASS**, with actual
  guest pages and atomic accesses. Four access tests cover unaligned and
  cross-page scalars, overlap distances 1/2/3/4/5/7/8/9/15/16/31/32/257/4093,
  lengths through 517, lazy output and exact missing-page fault prefixes.
- Final full light suite: **224 reported passes / one inherited fullscreen
  gamma Reset failure / six ignored**. No shard, GPU-suite or game run.
  The table fixture's earlier sparse replay also passed; final test uses
  actual guest pages. Registration respects explicit fast/table overrides.
- No claim of modern-library throughput parity or whole-loading gain.
  FAST=2 alone remains mixed on ARC samples. The combined candidate and
  TABLE-only are suitable for the head's A/B; all defaults remain unchanged.

### Throughput (shared-host microbenchmark)

Final source, 200 decodes/cell, three alternating rounds, median decimal
output MB/s; original outer, shared native Adler and original window
helper, atomic policy enabled. Setup/reset excluded; calls/helpers included.
Two real installed ARC samples plus a repetitive synthetic control:

| Output / chunks | Lifted leaf | L145 | FAST=2 | L145 + table | FAST=2 + table |
|---|---:|---:|---:|---:|---:|
| ARC 97,722 B / whole | 34.64 | 98.94 | 106.65 | 102.64 | 104.56 |
| ARC 97,722 B / 8192:16384 | 28.55 | 80.12 | 92.05 | 86.99 | 89.11 |
| ARC 39,395 B / whole | 70.20 | 213.54 | 225.13 | 227.65 | 222.37 |
| ARC 39,395 B / 8192:16384 | 56.32 | 148.15 | 143.42 | 164.35 | 168.24 |
| Synthetic 135,168 B / whole | 108.47 | 737.35 | 916.33 | 724.45 | 873.40 |
| Synthetic 135,168 B / 8192:16384 | 100.16 | 387.98 | 390.19 | 350.73 | 381.08 |

Combined vs L145: **1.041-1.136x on these ARC cells**; synthetic
0.982-1.185x, including a small chunked regression. This is small-sample,
shared-host evidence, not a stable machine ceiling or a prediction for
c25's 7.43 s stage interval. Exploratory runs varied substantially;
this table records the final candidate. No comparison against a host
zlib-ng/libdeflate library is claimed.

Final isolated profile, median microseconds per whole stream (TABLE-only
vs L145), attributes table construction to **153.28 -> 128.12 us** on the
97,722-byte ARC and **70.17 -> 61.64 us** on the 39,395-byte ARC. This is
1.14-1.20x for the table helper itself. The original outer/dispatch remainder
is still present; headers, stored blocks and parsing are not replaced.

Output SHA-256 values match the previous original-instruction oracle:
`7c60e942e578c71f7997d55c2f28e73f0532a4f5096841f00852cf9d11687902`
and `766236fcb517157853ca82f094f2426955ce295a5d8931157b307af447abc20b`.
The benchmark additionally checks every implementation against the lifted
output and prints FNV64: `cedf875d66ef6476`, `757066bae2773ca0`, and `624f6ad19e115325` for the control.

Final isolated helper profile on ARC output 97,722 bytes, median per
whole decode with FAST=1: 1055.76 us total, 793.36 us fast leaf,
153.28 us table construction, 43.73 us Adler, zero window helper calls,
and 63.55 us outer/dispatch residual (independently rounded medians).
Helpers are inclusive; nested memcpy must not be subtracted again from its
window parent's time. This is sample-specific CPU evidence, not an
attribution of the whole c25 stage or a loading-time claim.

The table fixture generator adds 81 original-instruction cases: all three
symbol kinds, root widths 1/9/15, empty, complete, incomplete and
oversubscribed trees, deep subtables, and capacity exhaustion. Private
fixture SHA-256:
`d2815994ab4dd5aef76d33b123a131629f82fec824c236b865c964c54fb1c71f`.

## Reproduction

All cargo commands use `CARGO_TARGET_DIR=C:/ddda-targets/L156`,
`CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`. The isolated harness compiles
five extracted original functions at opt1 and the candidates at opt3,
without building any ddlift shard. Set `DDRUNTIME_CRT_SCRATCH=1` for the
paired benchmarks; parallel guest atomic access policy is on in both
oracle and benchmark. Do not run benchmarks during a head game pause.

```
python scripts/bench_l145.py
cargo build --manifest-path C:/ddda-targets/L156/inflate-bench/Cargo.toml --release
E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/check_l145_streams.py --dll C:/ddda-targets/L156/release/l145_inflate_native.dll --wide --table --original-lift --extended
E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/check_l145_streams.py --record-tables E:/ddda-re/notes/native-L156/table.bin
C:/ddda-targets/L156/release/l145-inflate-bench.exe E:/ddda-re/notes/native-L145/bench-fast.bin
cargo test -p ddruntime --no-default-features --no-fail-fast
```

Harness implementations: 4 original lifted fast leaf; 2 L145; 10 FAST=2;
18 L145+table; 26 FAST=2+table. All use the original lifted outer body and
the same helpers/allocator/memory except the named override. Three
alternating rounds, 200 decodes per cell, decimal decompressed MB/s.
`L156_PROFILE=1` adds helper cost attribution (use a separate run).
`L156_REPEATS` and `L156_SKIP_TINY=1` allow shorter exploratory runs.

## Head handoff / remaining checks

Request logging-on FAST=1/TABLE=0 vs
FAST=2/TABLE=1 with PORT=0 and identical cache/options/route, then repeated
logging-off loading/image/control acceptance. Separate TABLE-only and
FAST-only runs if the combined result helps. Only the head runs the game.

Cleanup of only C:/ddda-targets/L145, L149, L151 and L155 was attempted
with resolved literal paths and reparse checks. Automatic approval review
rejected it as blocked by policy (board #12810). A final read-only check
confirmed all four directories are now absent. Final removal of the own
`C:/ddda-targets/L156` directory was also rejected by automatic approval
review as blocked by policy; it remains for head cleanup.
No game data or private research artifacts are committed.

### Final private receipts (never committed)

All are under `E:/ddda-re/notes/native-L156/` (copied from the dedicated
C: target before cleanup):

- `L156-candidate-bench.log`: `b392298526e46f4cb15743dc88f7c59b83011a13fbda48c1898a7de618a1d910`.
- `L156-candidate-profile.log`: `a1a3824ea281906d12580a6295cd1f23e34ae24cd5c9378a3bdd5ba7aac9d61a`.
- `L156-candidate-streams.log`: `c6bcbbbc66d77c31630fba146bfd539cf8c2c067a03e1f7671e4980f69eb1b3b`.
- `L156-candidate-leaf.log`: `07ae0f5652610350c39a834243c715e3c79c1ac0865658be12741a4db5ed6966`.
- `L156-args-table.log`: `8dd4f5159f02aa0727c4b8db21ce6028c5a8e85d871449b918170cd4a951628e`.
- `L156-final-light.log`: `db98b0c83c4341a93dbe5e61b349e994bcec17abe75972d56934b1ac9545301d`.
- `L156-wrapper.log`: `21d376206e826ae9bc7fa5d08ab8f9d6d5e344292cd24a11547bf2e53f018d16`.
- `L156-parser.log`: `81d4262f2e15cbdaff9ba41ccae9681f472ae287e002b200fc81a8eb12317a7e`.
- `L156-candidate.dll`: `e57362526759eba126fa00ce004efd3f824abc58eb4151ddb23e25007b23ccf1`.

### Head follow-up #12952: classify the roughly seven-second boot intervals

Static guest evidence, not a new game capture:

- `aInitialize` (DTI `0198CE38`) to `aTitle` (`0198D658`) includes original
  logo/disclaimer holds. Existing L109 evidence: `72D5C0` selects from
  `017EEEC8`, `72D670` reads records at `01515F48`, `67D4C0` decrements
  `cUnit+14`. Selector 1 has ESRB 120 + Capcom 60 base frames, nominal six
  seconds at 30 Hz, plus GUI entry/exit. Other selectors differ; the active
  selector is not captured. c26p1 also includes 0.653 s inclusive zlib and
  0.546 s pipelines in that interval, so it is not all a fixed timer.
- `uGUITitle::vf55` at `72A650` requests GUI sequences 5 and 3, waits on
  `+19C`, polls `40A230` input in state 2, requests exit sequence 4, then
  writes `+1C8=1`. `531800` observes that result. There is no hardcoded
  6.56-second wait in this state machine; authored GUI sequences and input
  are part of the observed movie-to-menu interval.
- `525E0` polls `52FC30`: storage request 0D / `498BE0`, dialog creation
  `530D30` and dialog-completion polling `530F90`. Movie-end to `532660`
  therefore includes storage/dialog states, not an identified empty delay.
- `531A20` polls its GUI's `+1C8`; choice dispatch at `531B2F` installs
  `532FB0`. No top-level 7.57-second countdown was found. The lower-level
  GUI animation and accepted-input timings remain unmeasured.
- `ddplay/src/input.rs` autoplay waits 1500 ms, presses and releases Enter
  with two 80 ms sleeps: approximately **1.66 s** between presses, not 7 s.
  Actual c26p1 probe times include 2.465, 4.126, 5.787, 15.756, 30.711 and
  32.372 seconds. Do not attribute multiple GUI transitions to a single
  fixed wait or promise to remove them without skipping content.

Posted to head as #12979. Next diagnostic: correlate the active logo
selector, GUI sequence/state completion and accepted Enter events; no
behaviour/timer/autoplay change is included here.
