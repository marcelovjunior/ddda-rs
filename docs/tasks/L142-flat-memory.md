# L142: flat guest memory prototype

Owner: codex-10. Branch: `task/L142-flat-memory`, base `335318fc`.
Assigned by head/user 2026-10-13. Scope: ddnative-base memory backing and
accessors, runtime memory adapter, optional scalar lifter lowering, tests,
benchmarks and documentation. No game runs or game-data changes.

## Design posted before implementation

Board 12054 records the design; 12051/12064 agree disjoint edits with
codex-7's L139 `fast_set_u32_inner`/`store128` experiment. L142 leaves those
existing accessor bodies and the emit helpers unchanged. Board 12109
keeps L139 gate 2 frozen; a third integration pin needs head selection.

The default heap page table is unchanged. Opt-in Windows x64 memory uses
one 4 GiB pagefile-backed `SEC_RESERVE` section, mapped twice: the guest
view at `base + guest_address`, and a privileged writable alias used by
existing PageTable pointers. This differs deliberately from a single
private `VirtualAlloc(MEM_RESERVE)` reservation: `poke` ignores permissions,
and temporarily making a read-only guest page writable would admit
concurrent forbidden stores. Both views share physical backing; neither
commits 4 GiB at construction. Pages commit through `VirtualAlloc` on the
guest view with `PAGE_NOACCESS`, then `VirtualProtect` applies permissions.
Committing through the alias first would briefly expose guest RW bytes.

Backing/publication, permission changes and retirement are serialized
under a per-address-space mapping mutex. Normal flat accesses do not lock
or read the software page table. There is no global current player/thread
address space: up to 64 simultaneously live flat reservations register
with the process-wide VEH. Construction fails explicitly if exhausted.

Unmapping protects the guest view NOACCESS and preserves its alias bytes,
matching the existing retired-pointer lifetime. That slot is permanently
retired: remapping it obtains fresh heap backing and leaves its flat view
NOACCESS, so future flat operations there use the exact checked path.
The section cannot be decommitted. Existing runtime backing likewise
retains freed pages until address-space destruction. This conservative
remap policy may be expensive under address reuse; it is a prototype limit.

There are no host file-view mappings/aliases in this base runtime to
relocate (`MapViewOfFile`/`CreateFileMapping` source survey). Its guest
VirtualAlloc adapter accepts only READONLY and READWRITE, and rejects
PAGE_GUARD rather than implementing a guard-page state machine. L142 does
not invent guard semantics: explicit NOACCESS is tested, other existing
service rejections remain unchanged.

## Fault and concurrency contract

Each flat access uses inline x64 assembly (avoids a Rust non-atomic access
data race) with a fixed MOV encoding and a tagged NOP. The VEH accepts
only access violations inside a registered view, with matching address,
read/write type, zero failure register, MOV encoding, preceding register
clear and trailing L142 marker. It checks the instruction region before
reading that marker. It advances past only the recognized MOV and sets
the failure register. Ordinary Rust then calls the original Mem accessor.
The handler does not unwind, commit, allocate, lock, or call guest code.
Unrelated access violations continue exception search.

The successful scalar path has no indexed page-table load. The marker and
failure test remain overhead: this is not literally a single host MOV.
An initial recovery-label design was replaced with the tagged NOP to
remove a per-access LEA/jump and avoid clobbering the Mem receiver registers.

Explicit fallbacks preserve behavior where a raw MOV alone is insufficient:

- Lazy reads return zero **without materializing**; lazy writes fault once,
  allocate through the normal Rust slow path and write the original value.
- Forbidden/unmapped accesses retain the original panic string, operation,
  original starting address and whole width. No panic crosses the VEH ABI.
- Cross-page and 32-bit-wrap operations use the preflight/bytewise path;
  a failed store publishes no partial bytes.
- Parallel unaligned operations remain bytewise Acquire/Release operations.
  Aligned accesses use native aligned MOV with compiler memory barriers.
- Windows has no write-only protection: WRITE-only pages are NOACCESS in
  the guest view, with both operations routed to the original checked path.
- Fixture/watch memories have no flat view unless they explicitly opt in.
  Sparse remains strict. Existing emit/x87 helpers retain their old path.

`Mem::entry` owns the scheduling boundary. `access_context` invokes it and
returns the table independently; old `page_table` still preempts for old
generated shards. The new optional lowering uses `access_context`, so it
does not depend on `page_table` to schedule. Split helpers capture the
flat view on part entry; lean/SSE/push/pop helpers retain `pt` unchanged.

## Opt-in and generation contract

- Build with `ddruntime/flat-memory` (Cargo package-qualified feature).
- Set `DDRUNTIME_FLAT_MEMORY=1` before constructing the runtime session.
- Generate scalar sites with `LIFT_FLAT_MEMORY=1` for inlinable accessors,
  or `LIFT_FLAT_MEMORY=outline` for the optimized outlined experiment.
- Unset `LIFT_FLAT_MEMORY`: default generated source stays identical.
  Without runtime feature/env, existing heap storage remains the default.
- Outline mode takes only the flat token and address, keeping the Win64
  argument list short; a missing token uses the trait path. Inline mode
  retains the old page-table fast path when its flat token is absent.

No `inline(always)` was introduced. All flags are explicit; there is no
runtime global switch changing an existing memory object's backing.

## Verification commands

All Cargo runs set `CARGO_INCREMENTAL=0`,
`CARGO_TARGET_DIR=C:/ddda-targets/L142`, `CARGO_BUILD_JOBS=4`.

```text
cargo test -p ddnative-base
cargo test -p ddnative-base --features flat-memory --profile play
cargo test -p ddruntime --no-default-features --no-fail-fast
cargo test -p ddruntime --no-default-features --features flat-memory --no-fail-fast
DDRUNTIME_FLAT_MEMORY=1 cargo test -p ddruntime --no-default-features --features flat-memory --no-fail-fast
cargo test -p ddruntime --no-default-features --features flat-memory --test flat_memory
PYTHONPATH=research/native E:/ddda-re/tools/pyenv/Scripts/python.exe -m unittest test_flat_memory test_lift_codegen test_lean_emission
```

For the benchmark, build only `ddnative-base --features flat-memory
--profile play`, then compile `crates/ddnative-base/examples/flat_cost.rs`
twice using `rustc --edition=2021 --extern ddnative_base=<that rlib>
-L dependency=C:/ddda-targets/L142/play/deps -C opt-level=0|1`.
This keeps the base at opt 3 while changing only the caller's optimization,
as in the real shard profiles. No generated shard is built by these commands.

## Results

Implemented: experimental arena/alias backing, VEH scalar recovery, optional
inline/outlined emission and independent preemption entry. **Not enabled by
default, not accepted as an FPS improvement.**

Focused tests: **2 base + 5 runtime** pass. They cover all six recovery sites,
permission combinations, privileged alias coherence, retired pointer bytes,
fresh remap zeros, lazy first touch, wrapping/cross-page operations, original
fault strings, no partial failed store, eight concurrent guest handles,
and four contending direct/alias aligned readers/writers. **28 Python**
emitter/lean tests pass. Full base suite: **31 tests pass** without the
feature, **33 pass** with the feature in optimized `play`.

Runtime default light suite: **198 pass / 1 inherited fullscreen-gamma
failure / 4 ignored**. Feature enabled, heap-default run: **201 pass / 2
failures / 4 ignored** (before the fifth new runtime test was added).
Feature + `DDRUNTIME_FLAT_MEMORY=1`: **202 pass / 2 failures / 4 ignored**.
Both feature runs add the timing-sensitive
`present_runs_without_the_guest_lock_in_parallel_mode` failure; its isolated
rerun passes, and the final default suite passes it. The fullscreen-gamma
failure is the inherited Reset-to-fullscreen host-context failure. These
are complete `--no-fail-fast` counts excluding duplicated child-test
summaries. No check was weakened. After the final VEH marker refinement,
the five focused runtime tests and optimized base suite were rerun and pass;
the flat-enabled full-suite count is from the preceding equivalent marker.

Microbenchmark: 2,000,000 operations/sample, nine samples with rotating
table/inline/outline order, median ns/access. Hot one-page working set,
shared machine; these include caller loop/black_box overhead and are not
game-profile or cache-miss measurements. Latest marker implementation:

| Caller opt | Operation | Table | Flat inline | Flat outlined |
|---|---|---:|---:|---:|
| 0 | aligned read32 | 16.512 | 23.300 | 18.468 |
| 0 | aligned write32 | 16.845 | 23.454 | 17.344 |
| 0 | unaligned read32 | 17.128 | 23.498 | 17.059 |
| 0 | unaligned write32 | 18.450 | 26.149 | 17.229 |
| 1 | aligned read32 | 2.965 | 2.943 | 3.627 |
| 1 | aligned write32 | 3.502 | 2.473 | 4.686 |
| 1 | unaligned read32 | 3.666 | 2.889 | 3.664 |
| 1 | unaligned write32 | 3.544 | 2.867 | 3.878 |

Opt 1 inline stores/unaligned reads improve in this sample, but opt 0
inline regresses, and outlining does **not** provide a consistent win over
the original optimized helper. Earlier samples vary under shared load;
no broad speedup or opt-0 success is claimed. Logs/binaries/assembly stay
under `C:/ddda-targets/L142` and are not committed.

Open acceptance: full native gate + supplemental suites on an explicit
generated-source pin, actual shard compile-time/size comparison, and head's
same-scene correctness/FPS/loading run. No original-game comparison, game
execution, GPU test, generated shard build, or native gate was run here.
Board 12106/12115 requests the next combined gate from head/codex-7, without
changing their already frozen L139 gate 2. The lower-level runtime tests
exercise real flat pages; strict native vector fixtures deliberately stay
on their watched trait path, so that gate alone cannot certify the VEH.

Next action: head selects an experimental gate pin; retain default heap
access until full gating and realistic workload measurements justify it.
Opt-0 improvement and remapped-slot cost remain explicit performance blockers.

## Platform evidence

- [CreateFileMappingW / SEC_RESERVE and alias coherence](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createfilemappingw)
- [VirtualAlloc / committing reserved pages](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualalloc)
- [VirtualProtect / per-view page permissions](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualprotect)
- [Vectored exception handling](https://learn.microsoft.com/en-us/windows/win32/debug/using-a-vectored-exception-handler)

Only public Windows ABI bindings/host code and synthetic tests are committed.

## Follow-up: game build and opt-0 experiments (2026-10-13)

Merged `task/fps-line` at `4233e4ed` into this branch as `49764236`.
This is the frozen inline build/gate source pin, including L140, L113,
L137 and the two-encode-thread default. Codex-7 owns the separately queued
L146 native gate3; the earlier L139 gates do not cover flat emission.
The requested `play-o1` profile was absent at that pin: the first Cargo
invocation stopped before compiling anything. `ad299d65` adds this profile,
inheriting `play` and explicitly setting all 32 address shards to opt 1
(the generated hot shard inherits opt 1). This matters because hot-list
generation sets ordinary `play` address shards to opt 0. Accessor and
emitter sources remain identical to `49764236`.

The private build driver is `C:/ddda-targets/L142o1/build.sh`, run through
`heavy.py codex-10` with `HEAVY_BIG=1`. It runs full `lift.py gen` with
`LIFT_DATA=E:/ddda-re/notes/native`, `LIFT_WORKERS=6`,
`LIFT_FLAT_MEMORY=1`, `--hot-functions E:/ddda-re/notes/L66/profile-text-hot.txt`
and the default scoped instruction steps. Cargo uses `CARGO_INCREMENTAL=0`,
`CARGO_TARGET_DIR=C:/ddda-targets/L142o1`, `RUSTC_BOOTSTRAP=1`,
`RUSTFLAGS=-Zthreads=4`, and
`cargo build --profile play-o1 -p ddplay --features emu,ddruntime/flat-memory
--timings --message-format=json-render-diagnostics`. The slot controls jobs.
Build output, Cargo timings and receipts stay outside the repository.
Generation ran 11:17:10–11:28:15 UTC (**665 s**), producing **59,734
functions / 1,535 modules / 33 crates**, with **44 scoped step functions**.
The compilation-only retry driver is `C:/ddda-targets/L142o1/compile.sh`;
it reuses this completed generation. `generated-receipt.json` in the same
directory records input/source SHA-256 values and 1,572 emitted Rust files
(279,819,064 bytes; **+5.10%** against the pin's tracked generated source).
Its aggregate file-manifest SHA-256 is
`0a519745df752d6b51ae7322e5c057ab893a6184371fde6b12677a9ef35ad64b`.
Codex-7 independently checked all 1,572 files and both private-input hashes
before adopting them for L146 gate3 (`dd1c0002`, combo24 plus `49764236`;
board 12485). That gate uses `ddnative-base/flat-memory` and includes the
base VEH tests; its result remains separate from the prior green gates.
Head confirmed on board 12260 that flat-generated shards remain local;
only documentation/results are committed until gameplay decides adoption.

Merged-pin checks: **36 base tests pass** with the feature in `play`,
**5 focused runtime tests pass**, and **28 Python emitter/lean tests pass**.
Default light suite: **202 pass / 1 inherited fullscreen-gamma failure /
5 ignored**. Feature plus `DDRUNTIME_FLAT_MEMORY=1`: **207 pass / the same
1 failure / 5 ignored**. Counts exclude duplicate filtered child-process
summaries. The timing-sensitive Present test passes in both merged runs.
Logs are `base-merged.log`, `flat-merged.log`, `python-merged.log`,
`light-merged.log` and `light-flat-merged.log` under `C:/ddda-targets/L142`.

### Build handoff

Build succeeded at **12:20:04 UTC**. Head-only game artifacts:

- `C:/ddda-targets/L142o1/run/ddplay.exe`, 296,527,872 bytes,
  SHA-256 `4647232d101f4ac214f0ed7158832c7b7da967692ac3bbcf24e843cae7b48639`.
- `C:/ddda-targets/L142o1/run/ddplay.pdb`, 35,151,872 bytes,
  SHA-256 `60cf7a52eef4896295a341009a02bb93d28ea0102e13a5e034bed5df2dc52d2b`.

The **build code pin is `ad299d65`**, with the local flat emission receipt
above. Cargo JSON confirms **all 33 shards have `opt_level=1` and
`fresh=false`**. Their rlibs total **525,330,570 bytes / 500.99 MiB**.
The final Cargo timing report gives first shard start 706.18 s, last shard
end 1,557.31 s, hence **851.13 s cold shard wall span**. This is about
**2.6–3.4 times** the supplied normal-o1 250–330 s baseline. Summed shard
durations are 6,882.49 s; this is overlapping elapsed time, not CPU time.

Whole-build elapsed time is **1,596.5 s** (219 dirty units, jobs=9,
maximum reported concurrency 10). Head's combo24 runs repeatedly suspended
the build during dependencies, so total elapsed time includes those holds.
This shared-machine run is not a controlled paired compiler benchmark;
nevertheless it does not establish compile-speed improvement. Detailed
Cargo unit timings are in `C:/ddda-targets/L142o1/cargo-timings/`, artifact
hashes in `build-receipt.json`, and stdout/stderr in `compile.log` and
`compile.err`. The heavy slot was released after the successful copy.

Required flat runtime switch: **`DDRUNTIME_FLAT_MEMORY=1`**. For head's
parallel/emulator game route also set `DDRUNTIME_PARALLEL=1` and
`DDRUNTIME_EMU_FALLBACK=1`; `DDGPU_ENCODE_THREADS=2` makes the inherited
default explicit. For a matched combo23 comparison retain
`DDDRAW_NATIVE=drawcall`, `DDRUNTIME_ZLIB_ADLER=1`,
`DDRUNTIME_CRT_SCRATCH=1`, and leave `DDGPU_NATIVE_SIM` unset. Use the same
`--no-vsync`, original exe/save/cache/scene configuration as the baseline.
Only the flat switch selects the new backing; no `LIFT_*` variable is
needed when running the already generated/compiled executable.

Board 12564 requests head's image/loading/FPS comparison and posts the
artifact path, hashes, environment and timings. Board 12566 informs
codex-7 that the slot is free for gate3. **No game was launched here, and
no game/image/FPS acceptance is claimed.** Gate3 and head's comparison
remain pending at this handoff. Flat-generated files stay local and
uncommitted as head requested; the committed changes are the profile and
these results, not the private performance probes.

### Opt-0 experiments, not promoted

A separate copy of the base source under
`C:/ddda-targets/L142/experiment` was compiled at opt 3, with standalone
callers at opt 0 and opt 1. The queued game source was not edited.
Two variants were measured in addition to table, inline and outlined:

- Thin-reference outlined accessor: pass `&mut &mut dyn Mem` for stores
  (shared reference to the receiver for loads), reducing the Win64 argument
  count; retain the existing inline accessor body and all its checks.
- Raw scalar experiment: cache the base integer once at function entry;
  do base-plus-address and the same cross-page/parallel-unaligned admission
  in assembly, preserving the marked MOV/VEH recovery and trait fallback.
  This removes nested opt-0 Rust helper calls. It was a private u32-only
  performance probe, not an accepted/tested replacement backend.

The existing outlined accessor already provides one optimized base-crate
call with no table lookup on its admitted flat path. The raw experiment
and outlining remove most of the opt-0 inline regression, but neither
establishes a consistent improvement over the table path. No experiment
was promoted, and the game build keeps the requested `LIFT_FLAT_MEMORY=1`.

One five-way rotating run (2M accesses/sample, nine samples, median ns):

| Caller opt | Operation | Table | Inline | Outline | Thin reference | Raw |
|---|---|---:|---:|---:|---:|---:|
| 0 | aligned read32 | 16.223 | 20.899 | 15.640 | 16.284 | 15.959 |
| 0 | aligned write32 | 16.043 | 19.219 | 15.962 | 15.850 | 15.929 |
| 0 | unaligned read32 | 16.217 | 22.785 | 15.839 | 15.859 | 16.160 |
| 0 | unaligned write32 | 16.697 | 20.873 | 16.569 | 16.584 | 15.970 |
| 1 | aligned read32 | 2.985 | 3.317 | 3.478 | 3.580 | 3.113 |
| 1 | aligned write32 | 3.679 | 2.143 | 3.769 | 3.765 | 2.935 |
| 1 | unaligned read32 | 3.483 | 3.489 | 3.726 | 3.947 | 2.505 |
| 1 | unaligned write32 | 2.163 | 1.798 | 2.494 | 2.275 | 1.784 |

Shared-machine timings vary substantially, including within this run.
Loop/black-box overhead is included. Private source, binaries and outputs:
`C:/ddda-targets/L142/experiment-cost.rs`, `experiment/src/mem.rs`,
`raw-o0.txt`, `raw-o1.txt`, `thinref-o0.txt`, `thinref-o1.txt`.
Gameplay, not these microbenchmarks, determines acceptance.
