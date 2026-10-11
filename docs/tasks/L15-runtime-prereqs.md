# L15: runtime prerequisites: original allocator bodies, the FS segment, image-backed reads

Owner: Codex (same session). Branch `task/L15-runtime-prereqs` from the head's
`main` after L14 is merged, worktree `E:\ddda-rs-wt\L15`.

## Why

After L14, 55,884 of 56,593 functions are verified (98.7%). Among the 161
lift rejections, the `segment override` ones (10 in the CRT range, 9
earlier) are all `fs:` accesses: the thread environment block (TEB), mostly
the structured-exception-handling chain at `fs:[0]`. They include the
CRT's `__SEH_prolog4` (0x012F71E0) and `__SEH_epilog4` (0x012F7225), which
`___tmainCRTStartup` (0x012EF8A7) and many CRT functions call. The runtime
(crates/ddruntime, now with guest threads) needs these bodies: it will give
every guest thread its own TEB in guest memory.

## Task A: original bodies at the five platform-model addresses

Lifted code calls the five models in `crates/ddnative-base/src/platform.rs`
directly (`platform::mt_alloc(m, cpu)` etc.). The head adds a runtime
override there: when the runtime installs it, each model call goes to the
dispatcher at the model's address instead (tests and the recorder keep the
models). For the allocators that address must then hold the **original**
body: `pool40_alloc` 0x00CF73F0, `mt_alloc` 0x00DE0B80, `mt_free`
0x00DE0B90 (they use the game's allocator object at 0x018D0AA0 and the pool
at 0x01877DA0; a replacement allocator would corrupt blocks the game frees
through that object directly). memset 0x012EFE20 and memcpy 0x012EFEA0
may stay models in the runtime, but lift them too if they lift.

1. Lift and record these five bodies as ordinary functions (their own
   instructions, callees mocked or closed as usual) under a distinct
   original-entry contract: their callers keep calling the model, and no
   existing record or case changes. L14 found 0x012EFEA0 rejects an
   unproven indirect switch table: resolve it or record it with evidence.
2. Make sure the generated program's lifted table (`lifted::all()`) includes
   them, so `Registry::with_lifted_program` binds them.

## Task B: the FS segment

1. `x86::Cpu` gets `pub fs_base: u32` (default 0). An `fs:`-prefixed
   memory operand's linear address is `fs_base + effective address`
   (wrapping); `ds`/`es`/`ss` overrides stay flat as today. Emit it through
   the existing address path so generated code stays readable.
2. Recorder (`harness.py run_sparse`): give every vector the same fixed
   TEB base (choose an address no other sparse region uses, e.g.
   `0x7FFD_E000`, and document it), set it as FS's base in Unicorn (a GDT
   entry, or `UC_X86_REG_FS_BASE` if Unicorn 2.1.4 honours it in 32-bit
   mode: verify which), and record TEB bytes read/written exactly like any
   other memory. Generated tests set `cpu.fs_base` to that constant. No
   vector format change unless unavoidable (then say why).
3. Re-scan and record the segment-override rejections; report which now
   verify. `__SEH_prolog4`/`__SEH_epilog4` adjust the caller's frame
   (they push and pop on the caller's stack and return through it):
   confirm the recorder's own-return/stack checks accept that pattern or
   explain precisely what they need. Do not special-case addresses.
4. Optional, only with evidence: the deleting destructor 0x00DB8CD0 is
   blocked because its callee tail-jumps through the IAT slot of
   `LeaveCriticalSection` (0x0139D164) and the mock's stack cleanup is
   unknown. If the mock can take an import's documented stdcall argument
   size (KERNEL32!LeaveCriticalSection: 4 bytes), propose the smallest
   general mechanism (a table of import argument sizes with its source)
   and apply it.

## Task C: image-backed reads for start-up code

The runtime now starts the program through the CRT start-up steps
(`Session::start`, a temporary translation of 0x012EF8A7). Two steps are
recorder rejections with "execution outside the image": `__mtinit`
0x012F982C and `__cinit` 0x012EDA5E. They call through function pointers
stored in the image's data (encoded FLS pointers, `_FPinit`, the
initializer tables), and the recorder's random fill makes those calls
jump to garbage. Add a structured-input strategy in which reads inside the
image's `.rdata` and `.data` (file-backed part) return the exe's actual
bytes, as in the loaded process, while everything else stays as today.
Keep it a separate strategy (no change to existing cases), record the
outside-image rejections with it (all 533 recorder failures are
candidates, these two first), and report what verifies. Where such a call
reaches an import (GetProcAddress results, FLS functions), the callee is
mocked as usual.

Out of scope: exception dispatch itself (RaiseException, unwinding), which
the runtime implements.

## Rules

As in L12 (`docs/tasks/L12-remaining-lift-rejections.md`, "Rules"). The head
seeds `LIFT_DATA=E:/ddda-re/notes/native-L15`. Keep CARGO_BUILD_JOBS=4 and
at most six vector workers; never run two heavy jobs at once. Report in
Results below with before/after counts.

## Results

Completed and compared against the original under the recorder contracts
below. All artifacts remain private in `E:/ddda-re/notes/native-L15`.

| Measure | Before | After |
|---|---:|---:|
| Selected lifted entries | 56,169 | 56,306 |
| Required lifted cases | 167,227 | 167,621 |
| Verified non-thunks / 56,593 | 55,884 (98.7%) | 56,021 (99.0%) |
| Verified non-thunk bytes / 15,516,093 | 14,847,363 (95.7%) | 14,906,956 (96.1%) |
| Verified thunks | 289 | 289 |
| Lift rejections | 161, plus five excluded model entries | 145, including memcpy |
| Liftable entries without fixtures | 533 | 417 |
| Quarantined entries | 10 | 10 |

All **137 additions / 394 cases** passed replay. The image-v1 pass tried
554 candidates and added 114 functions / 331 cases; zero tried 440 and
added 14 / 42; small-v1 tried 426 and added 9 / 21. Objects-v1 and
zero-objects-v1 each tried the remaining 417 and added none. Every pass
used three seeds, a 20-second per-function budget and at most six workers.
Canonical hashes confirm that all 56,169 prior records and 167,227 prior
cases are unchanged. No new replay quarantine was needed.

**Verification.** All 65 native Python tests and nine supplementary-inventory
tests passed. The subset replay covered all additions and closed callees
(14,323 entries / 42,763 cases), plus seven unit tests. The final
`DDNATIVE_REQUIRE_VECTORS=1 cargo test -p ddnative` passed all **32 tests**,
including every one of the **167,621 lifted cases**. It took **415.19 s**
(build 5m34s; lifted replay 72.82s), with **9.81 GiB minimum free RAM**,
`CARGO_BUILD_JOBS=4` and `CARGO_INCREMENTAL=0`. Logs and resource metrics are
`cargo-full.log` and `cargo-full-metrics.json`. The earlier interrupted
small-input pass resumed without losing progress; after the head clarified
the light-job exception, no competing heavy job interrupted verification.
No other agent's process was stopped. Temporary full-pair backups were
restored/removed; E: retained about 28.8 GiB free after the full build.

**Original entries.** The pool allocator 0x00CF73F0, MT allocation wrapper
0x00DE0B80, MT free wrapper 0x00DE0B90, and memset 0x012EFE20 now have
separate `original_entry` metadata and `original-entry-v1` body-key input.
Only root instructions bypass the recorder model; nested model calls keep
their old behavior. The originals are external-call entries in `lifted::all()`;
the replay dispatch table still binds model addresses to models. Existing
caller records/cases are preserved. Full scan, incremental preparation,
relifting and vector recording retain this distinction.

Memcpy 0x012EFEA0 remains rejected with private evidence in
`memcpy-rejection.json`: `jmp [eax*4+0x012EFF24]` at 0x012EFF11 has an invalid
slot zero (`0x90012EFF`, overlapping instruction bytes). The earlier alignment
test/branch at 0x012EFEE4/0x012EFEEA excludes zero before EAX is masked to
three, but the strict recognizer cannot prove that nonzero range. The
separate underflow-index path at 0x012EFF18 would also require proof; no
table entries or bounds were guessed.

**FS.** `Cpu.fs_base` defaults to zero. Common memory lowering adds it with
wrapping arithmetic for FS, leaves DS/ES/SS flat, and ignores segments for
LEA. Recorder/replay use TEB `0x7FFDE000`. A measured Unicorn 2.1.4 probe
read address zero after writing FS_BASE, but the correct TEB after loading
a GDT descriptor. `configure_fs` uses GDT `0x7FFDC000`, selector 16, and an
explicit flat 32-bit SS (selector 8) to avoid Unicorn's reset 16-bit stack
cache. TEB reads/writes are ordinary recorded memory; no vector-format
migration is needed. Segment-register values themselves remain unsupported:
0x012EDCAC and 0x012FF13C reach `mov ...,ss`.

The scan gains 17 FS bodies and four original model bodies. It has 554
missing candidates versus 533 before; 145 rejections versus 161 plus five
model exclusions before (the new rejection list includes memcpy).
A fresh scan with `014d91a`'s lifter confirms **17**, not the brief's
estimate of 19, segment-override rejections; both SS-register rejections
already existed. `baseline-scan.json` preserves that comparison privately.

The twelve FS bodies with passing new fixtures are 0x00CEE940, 0x00CEE9F0,
0x00CEECE0, 0x00CEEF70, 0x00CEF170, 0x00CEF430, 0x012EFA57, 0x012EFB14,
0x012EFDA2, 0x012F9AB0, 0x01303BB0 and 0x01307035. Five remain unrecorded:
0x00CEEBC0 and 0x012EFA1E reach invalid continuations (outside-image
execution; one object-input seed at 0x012EFA46 instead reads unmapped
memory); 0x012F71E0, 0x012F7225 and 0x01306F7F fail the stack-return guard
under all five tried strategies. The private strategy JSONLs retain every
seed and rejection reason.

`__SEH_prolog4` 0x012F71E0 and `__SEH_epilog4` 0x012F7225 now lift and can
return to the original sentinel, but their frame contracts fail the final
ordinary-stack guard. Prolog returns ESP = entry ESP - 24 - entry `[esp+8]`;
epilog returns ESP = entry EBP + 4. They need caller-frame initial state
and entry/exit stack contracts, preferably paired-frame fixtures. No
address exceptions or guard relaxation were added. Exception dispatch is
still outside this task.

**Startup recorder diagnosis.** Image pages were already initialized from
the executable, including file-backed `.rdata`/`.data`; random image bytes
were not the cause. `image-v1` explicitly tags that policy with baseline
random inputs elsewhere, without changing prior fixtures. Startup traces
instead expose mock cleanup: `__cinit` 0x012EDA5E cleans its indirect call
at 0x012EDA7E with POP ECX; `__mtinit` 0x012F982C has arguments pending
across DecodePointer calls at 0x012F994B/0x012F997A. General fixes recognize
consecutive volatile-register pops, preserve pending argument counts using
the actual return ESP, recognize nonvolatile saves after calls, and keep
register eligibility across identity MOVs (the hotpatch `mov edi,edi`).
These are mock ABI heuristics, not complete recovered callee contracts.

The small named import-cleanup table resolves slots/initial target values
from the PE and gives EncodePointer, DecodePointer and LeaveCriticalSection
their documented one-pointer stdcall cleanup (four bytes; sources in
NATIVE_PIPELINE.md). It also proves cleanup through direct IAT tails:
0x00DB8BF0 jumps through 0x0139D164 at 0x00DB8C9B, unblocking destructor
0x00DB8CD0. Import effects/returns remain mocked as before. Both startup
routines now record three image-v1 cases, without claiming closed startup
execution or live-game comparison.

**Handoff.** Adopt the matching private `patterns/lift.json` and
`vectors/lift.json`, together with the cleaned `patterns/lift_novector.json`;
retain the existing mismatch/quarantine lists (this copy has no novector
body-key cache).
Do not rescan first: that can change closed-call boundaries and invalidate
the preserved cases. After merging source, regenerate with `lift.py gen`,
run the required-vector test above, then rebuild inventory with
`scripts/inventory.py build` and check `scripts/inventory.py summary`.
For the shared rebuild, the head selects the adopted shared `LIFT_DATA`
and shared `INVENTORY_DB`/`INVENTORY_EXTRA`; L15 itself used only the private
database and supplementary TSV. No supplementary inventory changes are
needed. `final-stats.json`, `baseline-scan.json`, `subset-summary.json`,
the strategy JSONLs and before/after startup traces preserve the evidence.

Open work is memcpy's range proof, explicit caller-frame fixtures for the
SEH helpers, and the remaining 417 recorder failures / 145 lift rejections.
`___tmainCRTStartup` 0x012EF8A7 still has no accepted fixture. The next runtime
action is to set each thread's `Cpu.fs_base` and bind the four verified
original entries; exception dispatch remains outside this task. No runtime
code, generated code, private data, shared writes, native gate or push is
part of this commit.
