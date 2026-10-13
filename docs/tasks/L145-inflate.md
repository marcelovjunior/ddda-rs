# L145: exact outer inflate candidate

Owner: codex-6. Branch: `task/L145-inflate`, based on L140 `8ae81b76`,
then FPS-line merge `b79793b9`. Runtime-only; no game or shard build/run.

Head expanded the scope on board #12253 to the measured dominant
`0x00cf2cb0` fast decoder. The **fast-only candidate is the recommended
first loading A/B**; the outer prototype remains separately opt-in.

## Scope and implementation

Replace the whole `0x00cf02e0` entry behind `DDRUNTIME_ZLIB_PORT=1` through
the existing archive port installation and L136 override registration.
Explicit caller overrides win. Unset/`0` leaves the original entry selected.

`crates/ddruntime/src/zlib_port.rs` is a **register-level native translation**,
not a host library decoder or a new zlib state representation. It retains
the original control labels, guest state offsets, stack spills, all 29 modes,
headers, dictionaries, stored/fixed/dynamic blocks, slow/fast decoding,
checksums, error paths and resumable exits. The seven lifted outline
dispatchers become one control-flow graph. Local integer flag construction,
zero tests and the forward overlapping match loop avoid unnecessary helper
calls. Source: DDDA.exe 2017-12-13 Steam `0x00cf02e0..0x00cf158e`, checked
against read-only Ghidra and the original instructions. This is not yet a
compact high-level inflater rewrite.

The stream/window remains wholly in guest memory. There is no host state
cache to invalidate on reset, copy or resume. `cf01e0` window/allocation,
`cf38d0` CRC and `cf30c0` table construction remain original callees through
the existing dispatcher; fast decode and Adler retain their existing
independent overrides. Call instrumentation and explicit helper overrides
remain reachable. The port does not modify game files or real saves.

## Results

- Final original-instruction differential: **217 streams / 12,893 returns
  pass**. Every return compares all GPRs, XMM0..7, the native exit's defined
  arithmetic flag mask (including AF where defined), full scratch/state /
  32 KiB window, input/output mappings and the entire mapped stack. The
  native port executes against memory shared with Unicorn; each unowned
  callee executes its original instructions. No mocked decoder.
- Retains the original L140 **16 streams / 607 returns** gate. The expanded
  read-only census visits **8,536 ARC indexes / 358,431 entries / 144 type
  hashes**, testing one smallest compressed entry per class. This is class
  coverage, not replay of all 358,431 payloads.
- Seeded random input/output splits, compression levels 0/1/6/9, empty,
  repetitive and incompressible payloads, window wrap, raw deflate, zlib,
  gzip optional fields and trailer corruption, dictionary-needed return.
  Synthetic XML save payloads pass with flush 0..5. No real save opened.
  The existing-window path is tested; allocator success/failure callbacks
  and setting a dictionary after Z_NEED_DICT are not separately exercised.
- One Python-valid gzip fixture with optional fields and FHCRC is rejected
  by the **original** with `header crc mismatch`, including when supplied
  in one input buffer. The port preserves that rejection. Separate gzip
  streams without FHCRC complete, and CRC/length corruptions fail after
  producing the expected payload. No claim that every Python-valid gzip
  stream is accepted by this original version.
- **12 original callee-free guard/terminal/empty-input fixtures** pass in
  the runtime light suite. Registration tests pass both enabled and off,
  preserving explicit overrides. Existing fast-decoder oracle also passes.
- Full runtime light suite with the outer opt-in and required vectors:
  **205 reported passes / 1 inherited fullscreen gamma Reset failure /
  5 ignored**. Log: `C:/ddda-targets/L145-light.log`. Four build jobs,
  incremental disabled, dedicated L145 target. No GPU test or game run.

### Microbenchmark (not a loading improvement claim)

Isolated original outer lifted body at opt1 versus candidate at opt3;
same GuestMemory, original window/table helpers, existing native fast loop
and Adler, CRT scratch enabled on both sides. No shards compiled. Three
alternating rounds, 200 complete stream decodes per implementation/round;
setup and memory reset excluded, all inflate calls/helpers included.
Outputs agree. Decimal MB of decompressed output; median **ms/MB**:

| Payload bytes | Input/output chunks | Original | Candidate | Ratio original/candidate |
|---:|---|---:|---:|---:|
| 5,789 (ARC) | whole | 21.321 | 22.876 | 0.93 |
| 5,789 (ARC) | 8192/16384 | 21.466 | 23.694 | 0.91 |
| 5,789 (ARC) | 5/257 | 207.987 | 181.988 | 1.14 |
| 29,305 (ARC) | whole | 12.177 | 11.152 | 1.09 |
| 29,305 (ARC) | 8192/16384 | 11.978 | 14.388 | 0.83 |
| 29,305 (ARC) | 5/257 | 140.672 | 136.862 | 1.03 |
| 135,168 (synthetic) | whole | 4.021 | 4.135 | 0.97 |
| 135,168 (synthetic) | 8192/16384 | 5.158 | 4.811 | 1.07 |
| 135,168 (synthetic) | 5/257 | 42.827 | 26.304 | 1.63 |

Shared-host measurements: the large-buffer results are mixed, including
regressions. This candidate does **not** establish a large-call speedup and
must remain opt-in. The small-buffer repetitive case improves consistently;
it is not representative evidence for the loading run's 23,902 large calls.

The head's `run_c23def.log` resolves the inclusive-cost ambiguity:
27,135 outer calls / **8.019526 s**, including fast decoder **4.934958 s**,
Adler **0.722919 s**, window **0.466934 s**, CRC zero. Subtracting these
leaves **1.894714 s** in the outer body plus untracked helpers (notably table
construction) and dispatch. The 8 s cannot all be attributed to the outer
state machine. Main resource return PC `0x00db93f4`: 13,534 calls,
6.567099 s; archive `0x00e0d558`: 3,740 / 1.131994 s; save caller
site `0x004103b9`: 9,861 / 0.320432 s in that head-run log.

### Artifacts and reproduction

Private artifacts, never committed:

- `E:/ddda-re/notes/native-L145/inflate-guards.bin`, SHA-256
  `ae37f755e7e6b94ced2ae007e2d3cda1117821d3e48b2bb9d17fc01b863b8ee8`.
- `E:/ddda-re/notes/native-L145/bench.bin`, SHA-256
  `b20f59382b30eccdd5416830f2b061528a5d1817fd366a33f6fca0d47fee6830`.
- `C:/ddda-targets/L145-final-extended.log`, SHA-256
  `03b3514ac5c79348a6ac4d3b17f385101441cac990c48697aa79b1a5b9763315`.
- `C:/ddda-targets/L145-bench-final.log`, SHA-256
  `52280d81009744823533af2b736edd4ac47f1e101a1a725e21ffd0c9f3a92066`.
- Synthetic XML output (143,411 bytes), SHA-256
  `c453fa6b3fe75c2041480e41849611ce38b0667ee5af2e0a48e208063415e3cd`.

```powershell
python scripts/bench_l145.py
$env:CARGO_TARGET_DIR='C:/ddda-targets/L145'
$env:CARGO_INCREMENTAL='0'
$env:CARGO_BUILD_JOBS='4'
cargo build --manifest-path C:/ddda-targets/L145/inflate-bench/Cargo.toml --release
E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/check_l145_streams.py --extended
E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/check_l145_streams.py --record-guards E:/ddda-re/notes/native-L145/inflate-guards.bin
E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/check_l145_streams.py --bench-image E:/ddda-re/notes/native-L145/bench.bin
$env:DDRUNTIME_CRT_SCRATCH='1'
C:/ddda-targets/L145/release/l145-inflate-bench.exe E:/ddda-re/notes/native-L145/bench.bin
$env:DDRUNTIME_ZLIB_PORT='1'
$env:DDNATIVE_REQUIRE_VECTORS='1'
cargo test -p ddruntime --no-default-features --no-fail-fast
```

## Fast decoder follow-up (head #12253)

`DDRUNTIME_ZLIB_FAST=1` selects the same exact readable `cf2cb0` decoder
with a specialized local access policy (`zlib_access.rs`). Scalar accesses
check page permissions/backing and use the base atomic accessors. Forward
match copies check each source/destination page run once, then perform the
original ordered atomic byte loads/stores directly. This eliminates repeated
per-byte page lookup/helper calls without changing overlap semantics. Lazy
pages, inaccessible pages and crossing scalar accesses retain the Mem path.
The original per-copy cooperative preemption point is retained. No stream
state or buffers are cached in production and no host allocation is added.

- **632 original fast-leaf cases pass in three configurations**: existing
  decoder, new decoder on Sparse memory, and new decoder on actual guest
  pages. Final page-backed replay uses `ATOMIC_ACCESSES=true`. All recorded
  memory, GPRs, XMMs and arithmetic flags match. **1,896 fixture replays**.
- **217 streams / 12,893 returns pass with both new ports**, and the same
  **217 / 12,893 pass with only the fast port** and the isolated original
  lifted outer body. The oracle copies the real stream windows into a
  persistent *test-only* GuestMemory adapter for each native fast call,
  executing the real page path rather than its no-page-table fallback.
  The reference outer and helpers execute original instructions. Fixed
  decode tables are read from the privately mapped original image.
- Three focused access tests pass: overlap distances 1/2/3/31/4093 and
  page crossings, lazy output materialization and exact fault prefix, and
  aligned/unaligned/cross-page scalar values under parallel atomic policy.
- Fast-enabled full light suite: **207 reported passes / inherited gamma
  Reset failure / 5 ignored** (`L145-light-fast.log`). Final additional
  scalar-access test passes with all three focused access tests; final
  registration + 1,896 parallel-policy fixture replay passes separately.
  No repeated whole-suite count is inferred from these focused additions.

Final paired whole-stream benchmark uses the same conditions as above,
200 decodes per cell/round and three alternating rounds, adding the original
lifted `cf2cb0` baseline. Original outer body in all three columns below;
shared native Adler, original table/window helpers, CRT scratch on. Median
**ms per decimal decompressed MB**, including calls and helpers:

| Payload | Input/output chunks | Lifted fast leaf | Current L109 leaf | New page leaf | Current/new |
|---|---|---:|---:|---:|---:|
| ARC 97,722 B | whole | 37.908 | 21.275 | 12.754 | 1.67x |
| ARC 97,722 B | 8192/16384 | 36.798 | 21.261 | 13.390 | 1.59x |
| ARC 39,395 B | whole | 12.832 | 7.707 | 4.556 | 1.69x |
| ARC 39,395 B | 8192/16384 | 14.208 | 9.438 | 5.742 | 1.64x |
| Synthetic 135,168 B | whole | 7.559 | 4.839 | 1.082 | 4.47x |
| Synthetic 135,168 B | 8192/16384 | 8.741 | 5.701 | 1.831 | 3.11x |

The 5-byte input guard prevents fast-leaf entry, as expected: its fast-only
ratios are 1.03/1.05/1.03x (timing noise, not a fast-port improvement).
Both ports together were measured as well; the full matrix is in the log.
These are CPU microbenchmarks on a shared host, **not game loading gains**.
The two ARC outputs match the original SHA-256 values:
`7c60e942e578c71f7997d55c2f28e73f0532a4f5096841f00852cf9d11687902`
and `766236fcb517157853ca82f094f2426955ce295a5d8931157b307af447abc20b`.

Additional private artifacts:

- `E:/ddda-re/notes/native-L145/bench-fast.bin`, SHA-256
  `79f257c8b33e737900f06a61c6d50ad16e2b7871f7f1b3bbce4a160479a0e219`.
- `C:/ddda-targets/L145-bench-fast-final.log`, SHA-256
  `ed677c64402c1c51ffbb3e1b56977f00782fa4b5f6f53fc20d647868919ff357`.
- `C:/ddda-targets/L145-fast-only-streams.log`, SHA-256
  `57123132ec87b67c52fcd334f5456135c91fd190688d30c78c6cc19cb76e8c25`.
- Oracle DLL SHA-256
  `14dc98f74d9f1b73c4914350951c9494dad2f69697d0753f9a5c720a2dbd8ac6`.
- Both-new-ports oracle `C:/ddda-targets/L145-fast-streams.log` has the same
  per-case outputs/counts and hash as the earlier outer-only 217-stream log.

After building the private harness above, reproduce fast-only and combined
gates with `check_l145_streams.py --fast --original-lift --extended` and
`check_l145_streams.py --fast --extended`. Regenerate benchmark inputs with
`--bench-image E:/ddda-re/notes/native-L145/bench-fast.bin` and pass that file
to the benchmark exe. Implementation numbers in its log: 4 = lifted fast
leaf, 0 = current L109, 2 = new fast only, 1 = outer only, 3 = both new.
The final input generator selects the two largest of the eight ARC samples;
the earlier outer-only table used the last two samples instead.

### Head-requested c23 allocation census follow-up

Exact combo23 exe/PDB symbolization: `C:/ddda-targets/L145-alloc-sites.log`,
from `run_c23cen.log`. **62,820,141,904 / 63,084,922,457 requested bytes
(99.58%) overflowed the bounded stack table.** The retained top ten are
startup survivors, not an authoritative ranking of gameplay churn:

| Rank | Site | Requested bytes | Calls |
|---:|---|---:|---:|
| 1 | GPU Ring::new | 50,331,648 | 1 |
| 2 | Persistent::open | 40,106,397 | 1 |
| 3 | Naga Validator::validate | 38,319,512 | 516 |
| 4 | Image load guest pages | 23,113,728 | 5,643 |
| 5 | ExeFile::open | 21,841,776 | 1 |
| 6 | Registry::replace growth | 8,650,876 | 16 |
| 7 | Guest page-table array | 8,388,608 | 1 |
| 8 | Cached Naga IR deserialization | 5,474,216 | 516 |
| 9 | Second GPU Ring::new | 4,194,304 | 1 |
| 10 | Naga SPIR-V writer growth | 3,679,872 | 901 |

Reported to head, claude-9 and codex-11 on board #12213. A heavy-hitter
replacement policy or reduced stack grouping is needed before another
census can answer the real top-ten question; no unassigned census edits.

Next: head runs `DDRUNTIME_ZLIB_FAST=0/1` with `DDRUNTIME_ZLIB_PORT=0` first,
keeping Adler/CRT/cache options and pipeline-cache directory fixed, and
recording menu/gameplay markers. An outer-port A/B is a separate comparison.
No original-game comparison or whole-loading improvement is claimed yet.
