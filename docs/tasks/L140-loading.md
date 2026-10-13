# L140: faithful startup and save-loading costs

Owner: codex-6; branch `task/L140-loading`, base FPS line `335318fc`.
Assigned by head/user 2026-10-13. Runtime-only; no game or shard runs.
Scope: zlib helpers/caller telemetry, persistent GPU caches, host allocation
census and cheap allocation reuse. Original boot/menu waits remain intact.

## Evidence and implementation

Head's combo21 at 108 s: 4,224 archive reads / 1.24 s, 3,740 archive
inflates / 1.19 s, 27,135 total zlib calls / 8.30 s. Large-buffer entries:
23,902 / 8.04 s; short output: 3,199 / 0.26 s. GPU: 517 modules / 0.45 s,
translation 0.28 s, 2,331 pipelines / 2.66 s. Rust requested 90.6 GB.
These are inclusive cumulative service times, not additive elapsed loading.
Read-only audit of the now-copied `run_c21draw.log`: requested bytes are
already 9.397 GB at 10 s, 20.595 GB at 20 s, 33.525 GB at 30 s and
44.542 GB at 40 s; autoplay reports gameplay at 46 s. Substantial churn
precedes gameplay, but the aggregate counter cannot identify its call sites.

Read-only Ghidra identifies three callers of `0x00cf02e0`:

- `0x00db92c0`: sResource::DecompressStream::vf12, 128 KiB input segments.
- `0x00410180`: nSaveStream::DecompressStreamZLib::vf12, 64 KiB input buffers.
- `0x00e0d320`: rArchive::DecompressStream::vf12 (the existing archive counter).

The existing "archive" total excludes the resource and save stream classes.
New `DDRUNTIME_LOAD_LOG=1` events report each return PC's completed calls,
inclusive time, consumed input and produced output. Separate helper totals
cover fast decoder cf2cb0, Adler cf34d0, CRC cf38d0 and window cf01e0. Thus
the large-buffer count is not misrepresented as a fast-decoder invocation
count. Dynamic caller/helper ranking awaits the head's next run.

`DDRUNTIME_ZLIB_ADLER=1` installs the whole Adler-32 helper through existing
archive port registration before load wrappers and L136 Registry binding.
It preserves original stack spills, argument mutation, GPRs and defined flags.
The checksum reads bounded stack scratch with no heap allocation. **The outer
inflate state machine remains original**: no substitution of a modern decoder
with a different private stream/window representation is claimed.

`DDRUNTIME_CRT_SCRATCH=1` retains at most 2 MiB of memcpy scratch per host
thread; larger/reentrant copies keep independent storage. The whole source
is still read before any destination write, preserving overlap and fault
behavior. Memset uses a fixed page-sized stack buffer. The cap is a host
retention policy, not a limit on guest requests. Both changes default off.

`DDPLAY_ALLOC_STACKS=1` alongside `DDPLAY_ALLOC_LOG=1` records successful
allocation/reallocation request sizes and up to 16 host frames in a fixed
8192-slot atomic table. It neither allocates nor symbolizes inside the
allocator. Bounded probe overflow is explicitly counted in bytes. The
existing once-per-second load logger prints the top ten cumulative stacks.
`python scripts/alloc_sites.py LOG --exe EXACT_RUN_EXE` ranks their latest
rows and resolves the matching PDB offline; it never starts/attaches to a
game. Census runs add stack-walk overhead and are not timing baselines.
No top-ten game attribution is claimed before that run.

Pipeline audit: L71 already persists wgpu/Vulkan driver caches and L116
already warms recorded pipeline recipes on a background thread. The default
cache directory now follows the runtime sandbox (`DDRUNTIME_HOME/pipelines`,
otherwise `%TEMP%/ddruntime/pipelines`). Explicit `DDPLAY_PIPELINE_CACHE`
directory/`0` overrides still work; this directory change starts cold once.
`DDGPU_NATIVE_PIPELINE_CACHE=1` also shares that cache with native-device
pipelines, whose descriptors previously used `cache: None`. Normal drawcall
pipelines already used the cache; no new speedup is attributed to them.

## Results (in progress)

- **45 original-instruction Adler cases pass**, including eight real ARC
  decompressed payloads (5,789–97,722 bytes) read only from the install.
  All GPRs, all recorded scratch/stack bytes and defined flags agree.
  Synthetic cases include null/empty input, arbitrary seeds, 16/5552-byte
  boundaries and 128 KiB. Private vectors:
  `E:/ddda-re/notes/native-L140/adler.bin`, SHA-256
  `39b283565af720ab06d069dd2830c2964425b959ce3facd5d9e17b90383310c0`.
- Paired CPU-only microbench, three alternating rounds, 1,000 calls each,
  original isolated lifted leaf opt1 versus native/runtime opt3 (production
  profile intent), without building shards. Median ns/call: 5,552 bytes
  **19,997 -> 1,788**; 97,722 **399,322 -> 33,517**; 131,072
  **495,049 -> 43,952**. Empty input **66 -> 108** is a small-call regression.
  Shared-host timings, not loading/FPS results. Generator `scripts/bench_l140.py`;
  private log `C:/ddda-targets/L140-bench.log`.
- **16 full-stream original/native cases pass** using the actual Rust Adler
  port through a private DLL callback in Unicorn's original `cf02e0` stream
  execution: **607 inflate returns / 599 native checksum calls**. At every
  return, all GPRs, XMMs, defined flags, full scratch/state/window/output and
  stack agree. Eight original compressed ARC payloads plus boundary cases
  (input/output 1/1, 5/257, 6/258, 4096/4096), corrupt checksum/header,
  truncation and raw deflate. Valid output also equals Python zlib. Uses
  flush=0 as the three game callers do, with a preallocated guest window;
  window allocation itself is not replaced/tested here. Private log
  `C:/ddda-targets/L140-stream-oracle.log`, SHA-256
  `49e60d1d7c0ef2cf5acd9c0e7ce7fda9364acdad82f08bf1dcc68515b2c76e95`.
  Example 97,722-byte archive output SHA-256:
  `7c60e942e578c71f7997d55c2f28e73f0532a4f5096841f00852cf9d11687902`.
- Allocator standalone off/on/stack-enabled forwarding, startup and offline
  exact-PDB symbolization pass (`scripts/test_alloc_log.py`), as does the
  cumulative-row/deduplicated-stack ranking regression (two Python tests).
- CRT identity/allocation test passes off/on. After warmup, 20 copies of
  1,382,400 bytes request **27,648,000 -> 0 host bytes**. Oversized 2 MiB+1
  copies retain **41,943,060 bytes** for 20 calls. Memset requests zero at
  all tested sizes with the opt-in. Existing overlap/page/lazy-page test
  passes with scratch enabled; nested/unwind/bounded-retention test passes.
- First light suite found the inherited fullscreen gamma Reset failure and
  a telemetry fixture without a return PC; the latter was corrected.
  Focused rerun: **57 unit + 1 Adler + 1 scratch tests pass**, two unit
  ignores. **Final full light suite: 203 passed, one inherited fullscreen
  gamma Reset failure, four ignored**, with both runtime opt-ins, load
  logging and required vectors enabled. Log:
  `C:/ddda-targets/L140-light-final.log`. All cargo commands use the L140
  target, CARGO_INCREMENTAL=0 and CARGO_BUILD_JOBS=4; no shard builds.
- **Two bounded GPU cache regressions pass** while pause.py reported not
  paused: native cold/warm pipelines retain identical pixels across four
  blend/scissor cases, and the existing L116 test reuses persisted pipelines
  without cold shader/pipeline creation and with identical images. Each
  process had a 60-second hard timeout; both completed in about one second.
  Logs `L140-native-cache-test.log` and `L140-warmup-test.log` in the target
  parent. No game FPS or loading-time claim.

Commits: GPU cache `3e0eb771`, runtime/census `c9371641`, followed by the
stream oracle/benchmark tooling and final results. Head run requests and
verified-number reports: board #12101 and #12110. The requested dynamic
top ten and game loading A/B are still external measurements, not inferred
from microbenchmarks. No full cf02e0 replacement or unrelated boot shortcut.

Reproduce CPU stream check (never launches the game):

```powershell
python scripts/bench_l140.py
$env:CARGO_TARGET_DIR='C:/ddda-targets/L140'
$env:CARGO_INCREMENTAL='0'
$env:CARGO_BUILD_JOBS='4'
cargo build --manifest-path C:/ddda-targets/L140/adler-bench/Cargo.toml --release --lib
E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/check_l140_streams.py
```

Next: head runs a short allocation census and an Adler/CRT on/off loading
pair, supplying unskipped menu/gameplay timestamps and load-log totals.
Use separate census and timing runs. Then rank the measured remaining zlib
helpers and host sites before expanding ports or changing more buffers.
