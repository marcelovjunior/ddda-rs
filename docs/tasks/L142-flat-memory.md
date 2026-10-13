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
