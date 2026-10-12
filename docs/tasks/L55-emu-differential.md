# L55: check the emulator fallback against the recorded test cases

- **Owner:** Codex
- **Branch:** `task/L55-emu-differential` (worktree `E:\ddda-rs-wt\L55`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

The boot's bring-up now runs through the emulator fallback (L51,
`crates/ddruntime/src/emu.rs`, feature `emu`): untranslated functions
execute as original machine code in Unicorn, with registers, EFLAGS, x87
(FNSAVE image), MXCSR, XMM and FS synced to and from the runtime's `Cpu`.
With it the game reaches the title screen. Every bring-up result depends
on that state sync being exact, and so far one unit test checks it.

We already have exact references: the recorded cases (inputs and expected
outputs, from the original program) of about 59,000 verified functions.

## Task

1. A test (ddruntime, feature `emu` plus `game`) that takes recorded cases
   from `LIFT_DATA/vectors/lift.json` and, for each:
   - sets up the case's guest memory and registers as the replay does
     (`crates/ddnative/tests/lifted.rs` shows how);
   - runs the function through `emu::run` instead of the lifted body. The
     exe's code pages must be mapped: load the image as `Session` does.
     Mocked calls (`calls` in the case) must be bound as call-outs that
     return the recorded results;
   - compares registers, flags, x87 state, MXCSR, XMM and memory writes
     with the recorded outputs, exactly as the replay does.
2. Run it over a sample that stresses the sync: functions using x87 (the
   `x87_*` records), SSE (MXCSR, XMM), flags across calls, FS accesses, and
   a random sample of the rest (say 2,000 functions).
3. Fix every mismatch in `emu.rs` (one cause at a time, with a test), or
   document why the case cannot run in the emulator (e.g. a mock protocol
   it cannot express).
4. Results: counts per class, the fixes, and the remaining exceptions.

## Notes

- Building with `emu` needs CMake on PATH:
  `C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin`.
- Use plain compiler vocabulary in code and messages (see the head's
  earlier note): this is a test of an emulator against recorded outputs.
- Heavy builds go through heavy.py as codex. The 32-shard build fits 6
  jobs in a slot now.

## Results

### Harness and scope

- `research/runtime/emu_sample.py` selects verified inventory addresses with
  recorded cases, seed 55, in disjoint classes: FS 15, flags/calls 200,
  x87 200, SSE 200, random 2,000 (2,615 functions). Classes describe static
  lifted bodies; they do not imply every recorded path executes that feature.
- `crates/ddruntime/tests/emu_differential.rs` runs original roots through
  the same emulator implementation as `emu::run`. Closed callees retain the
  replay's lifted bindings; recorded opaque calls check target/order and all
  eight input registers before returning their recorded results.
- Checks cover all recorded GPRs, XMM0's recorded low word, x87 control,
  depth and extended top where present, MXCSR where present, and the whole
  byte write set (including unchanged stores). The recording format does
  **not** contain final EFLAGS or all eight XMM registers. Synthetic tests
  separately check flags, DF, all XMM words, MXCSR and extended x87 values
  across emulator/native call-outs.
- The executable is loaded once. Each case restores touched pages, applies
  the recorded memory inputs, permits the recorder's expected stores, and
  invalidates emulator mappings before execution. Call-out stores use the
  same write observer through `Mem`.
- Explicit unsupported protocols: recorded CPUID/RDTSC events, a mock target
  now compiled as a closed callee, or CPU-dependent cases recorded on a
  different processor. These are reported separately from mismatches.
  Actual call-target observation supports arbitrary mock-target counts;
  target/order and register checks remain exact. A 60-second bound is
  checked between call-outs and every 4,096 guest blocks; it cannot
  interrupt a native lifted callee. This avoids creating/joining Unicorn's
  timer thread for each short segment.

### State fixes

- Guest selector reads previously returned setup values
  `10/00/10/10/18/10`, instead of `Cpu.seg`'s `2b/23/2b/2b/53/2b`
  (ES/CS/SS/DS/FS/GS). Descriptors now use guest selectors, and selectors
  plus FS base are transferred back on return and call-out.
- Setting CS through Unicorn's register API leaves its cached privilege
  level unchanged. Previously `PUSH 3002h; POPFD; PUSHFD` returned IOPL 3,
  IF 0. A private IRET establishes CPL 3 once per instance, so POPFD now
  preserves IOPL 0 and IF 1. Synthetic regressions cover both directions
  of selector transfer and the POPFD behavior.
- Older recorder fixtures explicitly carry flat selectors
  `CS=0, SS=8, FS=16`. They use a separate CPL-0 instance and preserve
  Unicorn's initial cached 32-bit attribute for null CS. Reloading null CS
  would incorrectly decode the state stubs as 16-bit code. Normal runtime
  CPUs retain the CPL-3 behavior above.
- Unaligned operands spanning a mapped and an unmapped page attempted to
  map the first page twice. The memory hook now installs only missing pages.
  This caused the observed faults at `004c8543` and `0057ac01`; a synthetic
  read/store test crosses both boundaries after touching each first page.
- Unicorn's `until=0` stops before the block hook when an indirect call token
  is zero. Boundary handling now recognizes recorded bindings and returns
  at that stop as well. Regressions check a zero call token and zero return.
- Unbound caller return addresses no longer require ESP to increase: stack
  allocation helpers legitimately return with a lower ESP. The regression
  preserves the return address while the callee adjusts ESP.
- Private descriptors/state stubs now choose unused guest pages rather than
  occupying recorded high addresses. The write observer excludes only those
  private pages, instead of discarding all high-address stores. A regression
  occupies both preferred and relocated ranges across repeated entries.
- The pinned Unicorn 2.0.1 hook wrappers hold an `Rc` back to their engine.
  Explicit removal of instance hooks breaks that ownership cycle on drop;
  a weak-reference regression checks release. A lease returns instances to
  the pool when diagnostic or terminal call-outs unwind, removing the
  per-case observer before reuse. The first broader run ended after 3,475
  cases with translator-buffer allocation failure; its partial results are
  retained in `full-baseline.jsonl` rather than counted as a completed run.
- The recorder executes flag-setting SSE operations and hardware-dependent
  x87 operations on the host CPU. Unicorn's SSE arithmetic omitted sticky
  MXCSR exceptions; its FYL2X produced a one-bit difference in a stored
  double. `emu_numeric.rs` now routes legacy SSE floating arithmetic,
  comparisons/conversions/approximations and supported x87 D9 register
  operations through existing native `ddnative-base` instruction helpers.
  Roots still execute original code; no lifted function body substitutes
  for the root. Regressions check full packed results, exception flags and
  an extended x87 logarithm result. Unmasked SSE exceptions remain rejected
  by the native helper boundary, as in the recorder.

### Reproduction and private evidence

Use `LIFT_DATA=E:/ddda-re/notes/native`, `L55_SAMPLE` pointing to the sampler's
manifest, and `L55_REPORT` to a private JSONL output. Run in the heavy slot:
`cargo test -p ddruntime --features emu,game --test emu_differential -- --ignored`.
Synthetic checks: `cargo test -p ddruntime --features emu --test emu`.
Artifacts are under `E:/ddda-re/notes/L55`; no recordings or executable bytes
are committed. Shared native data is read-only; native-L28c remains frozen.

### Initial verification (coverage corrected during integration)

The initial 7,764 passing comparisons included **573 bound roots / 1,704
cases** that could take a native binding at their first instruction fetch.
They are a mixed emulator/native baseline, not full original-root coverage.
The L56 integration below excludes root bindings and reruns the sample.

| Class (disjoint static classification) | Functions | Cases passed | Mismatches | Unsupported |
| --- | ---: | ---: | ---: | ---: |
| FS | 15 | 43 | 0 | 0 |
| Flags/calls | 200 | 592 | 0 | 0 |
| x87 | 200 | 580 | 0 | 0 |
| SSE | 200 | 596 | 0 | 0 |
| Random remainder | 2,000 | 5,953 | 0 | 0 |
| **Total** | **2,615** | **7,764** | **0** | **0** |

- 15 integration regressions and one hook-release unit test pass.
  This includes FS/SIB memory operands, scalar-to-GPR conversions, and a
  pure integer loop stopped by the diagnostic deadline.
- Smoke sample: 58/58 cases, 20 functions.
- Focused numeric/final-boundary rerun: 57/57 cases, all 19 functions that
  had an exception in the first complete sample. The previous complete
  sample is retained as `full-before-numeric.jsonl`; the final run uses
  `full-final.jsonl`.
- Full sample: **7,764/7,764**, **2,615 functions**, **187.23 seconds**.
  No selected case was skipped. This is sample coverage, not a claim about
  all possible inputs or every instruction of the emulator backend.
- Audit: `input-before.json`, `input-after.json`, `input-audit.txt` report
  **UNCHANGED** for shared `patterns/lift.json` and `vectors/lift.json`.
  SHA-256 prefixes: patterns `9e6a76c221b0c4f3`, vectors
  `f35ccad4d3950a12`. The earlier selection manifest predates the head's
  train-17 input edits; the run receipt identifies the actual inputs.
- No shared recordings were written; native-L28c was untouched. No game
  generation, inventory mutation, push or game boot was performed.
- Remaining exceptions: **none in this sample**. The explicit protocol and
  CPU-state limitations above remain. Next action: head merges this batch
  and rechecks the title-screen boot with the corrected fallback.


### L56 integration

- Merged main through `b4462b88`, including L56 `38b05003`. Kept direct
  register state-out, a host-backed scratch page, stable emulation stop
  configuration and selective segment reloads. The selector cache tracks
  the values read back after guest execution, including a changed FS base.
- Numeric instruction hooks now cover candidate instruction addresses in
  inspected blocks. Integer loops do not run an instruction callback.
  Opcode-looking immediate bytes are harmless: Unicorn invokes these hooks
  only at actual instruction boundaries, and the encoding is checked again.
  Installing a batch of new hooks flushes translations once; normal repeated
  calls retain their cache. Unicorn 2.0.1's range-cache removal and mapped-page
  replacement each missed a synthetic case, so this uses its explicit
  `UC_CTL_TB_FLUSH` control (absent from the pinned Rust wrapper). Guest code
  stores clear the classification, and remapping or observed fixture
  replacement removes cached hooks.
- Added regressions for changing code into an SSE instruction and switching
  TEBs on a reused instance. Existing L55 checks and all three L56 tests are
  retained, alongside the root-binding regression below.
- Integration review found that the unmapped-fetch hook could dispatch the
  selected root natively if it also had a closed registry binding. Against
  `d9847a07`, this affects 573 selected functions / 1,704 cases (FS 9, x87
  132, SSE 163, random 1,400). `previous-bound-roots.json` lists them.
  The runner now omits that binding and rejects any root call-out. The
  emulator also keeps its explicitly requested root in original code on
  first fetch and loop-back, including after numeric resumes. A synthetic
  root with a failing native binding tests both entry and a three-iteration
  SSE loop. The initial count above is superseded by the corrected run.


#### Corrected release verification

| Class | Functions | Cases passed | Mismatches | Unsupported |
| --- | ---: | ---: | ---: | ---: |
| FS | 15 | 43 | 0 | 0 |
| Flags/calls | 200 | 592 | 0 | 0 |
| x87 | 200 | 580 | 0 | 0 |
| SSE | 200 | 596 | 0 | 0 |
| Random remainder | 2,000 | 5,953 | 0 | 0 |
| **Total** | **2,615** | **7,764** | **0** | **0** |

- Final `--release --features emu` differential gate: **174.98 seconds**.
  All selected roots execute through the corrected boundary; closed callees
  retain the documented replay bindings. No selected case was skipped.
- **21 integration tests plus one hook-lifetime unit test pass**, including
  all L56 tests, the bound-root loop, changing code and changed-TEB cases.
  Integration suite: **2.21 seconds**. Measured **100,000 short calls in
  396.4775 ms** and the **3-million-iteration loop in 20.0241 ms**.
- A redundant cache flush on entering a fresh memory raised the cold loop
  to 325 ms. It was removed; the final full sample and synthetic gate above
  reran on the restored fast path. Numeric hook installation still flushes
  when required. The earlier corrected full pass (199.38 seconds) and the
  interrupted extra-reset trial are retained as separate evidence.
- Shared input audit: **UNCHANGED**, including SHA-256, size and mtime.
  Patterns `fc10bf973d025461`, vectors `64eaf25a77dd43d3` (SHA-256 prefixes).
  These are the current shared inputs; the older baseline's hashes above
  predate the head's train updates. Native-L28c remains untouched.
- Evidence: `E:/ddda-re/notes/L55/integration/full-release.jsonl`,
  `full-release.log`, `unit-final.log`, `resource-final.log`, input audit
  receipts and `handoff.json`. One cold-build compiler termination without
  a Rust diagnostic was retried with lower concurrency; all 32 shards and
  the final gates completed. No game generation or boot was performed.
- **Next action:** head merges this L56-compatible batch and rechecks boot.
