# L27: structured exception handling (Windows' SEH dispatch for the guest)

- **Owner:** Codex
- **Branch:** `task/L27-seh` (worktree `E:\ddda-rs-wt\L27`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md). Codex cannot receive messages
  mid-run; record questions in Results and carry on.

## Why

32-bit Windows programs use SEH: a chain of registration records at
`fs:[0]`, which the CRT's `__SEH_prolog4`/`__SEH_epilog4` (L17 part B,
claude-3) push and pop. When an exception is raised, Windows walks that
chain and calls each handler. The handlers are `_except_handler4` for
`__try/__except` and `__CxxFrameHandler3` for C++ `throw`/`catch`, both
ordinary CRT code that is lifted. Windows also offers unwinding
(`RtlUnwind`), the unhandled-exception filter and vectored handlers.

The runtime has none of it: RaiseException and guest faults stop the
session with a report (`docs/RUNTIME.md`, findings: "Exceptions are rare").
That is correct only while no exception happens. The boot (L21, claude-2)
will reach code that relies on dispatch, for example probes guarded by
`__try`, C++ `throw` on failure paths, or the CRT's own exception filters.

## Task

1. **Census** (put it in Results, with addresses):
   - every `__try` scope in the exe (scope tables of `_except_handler4`
     frames, which `__SEH_prolog4` callers pass);
   - every C++ throw site (`_CxxThrowException`) and every function with an
     EH frame (`__CxxFrameHandler3` FuncInfo);
   - the imports involved: RaiseException, RtlUnwind,
     SetUnhandledExceptionFilter, UnhandledExceptionFilter,
     AddVectoredExceptionHandler and IsDebuggerPresent;
   - which of these the start-up and main loop can reach (Ghidra xrefs).
2. **Implement Windows' x86 dispatch** in `crates/ddruntime`:
   - EXCEPTION_RECORD and CONTEXT on the guest stack, laid out as Windows
     lays them out;
   - the `fs:[0]` chain walk, calling each handler as Windows does
     (`EXCEPTION_DISPOSITION` results, nested-exception records, the
     stack-limit checks);
   - `RtlUnwind`, with unwinding calls to handlers (EXCEPTION_UNWINDING
     flags) and resuming at the target frame;
   - the unhandled-exception filter and vectored handlers if the census
     shows they are used.
   Guest faults (access violations, integer divide by zero, the faults the
   lifted code and memory model can detect) become guest exceptions with
   the right codes. The CONTEXT the guest sees must be the real register
   state at the fault.
3. **Resuming.** ExceptionContinueExecution after a handler changes the
   CONTEXT means resuming at an arbitrary address with arbitrary
   registers. Find out what the lifted code's control flow allows. That
   includes resuming at a lifted function's address and at labels inside
   functions; the resolver knows the lifted entries. Implement what the
   census needs, with tests. Anything else stops with a precise report;
   no guessing.
4. **Tests against Windows.** For each case, small x86 test programs (C
   compiled with MSVC /EHsc, `/SAFESEH` and the like, under `research/`)
   run on real Windows and through the runtime's dispatcher with the same
   handlers, and must give the same results:
   - handler order;
   - dispositions;
   - unwinding;
   - nested exceptions;
   - the CONTEXT contents that the handlers observe.
5. Update the `docs/RUNTIME.md` row ("SEH dispatch").

## Dependencies

- The SEH prolog/epilog helpers and `_except_handler4` must be verified
  lifts. claude-3's L17 B/C covers the prolog/epilog; check the rest in
  the inventory. Report any that are missing in Results; the head routes
  them to the lifter agents.
- Use the heavy slot for every build that pulls `ddnative` (one slot per
  agent; take slots only through `heavy.py`, never by other means).

## Results

Runtime implementation and evidence are below. **L27 is partial:** general
instruction-fault delivery and nonlocal continuation require changes to the
lifter/Cpu owned by other lanes; this branch does not pretend those paths run.

### Census (read-only, 2026-10-11)

`research/runtime/seh_census.py --output E:/ddda-re/notes/runtime-L27/census.json`
reads the unpacked image, both function inventory TSVs, exported Ghidra call
edges and PE imports. No shared data
is written. It finds 42 EH4 registrations (41 prolog calls and one inline
frame), 48 structurally valid scope entries, seven C++ handler thunks used by
eight functions, and five throw instructions. A raw E8/E9 reference cross-check
finds all 41 prolog calls, seven C++ handler jumps and five throws; recovered
thunks are validated by instruction shape and FuncInfo magic, and the extra
throw by its catch-map entry. No raw-reference candidates remain unresolved. Scope parsing stops at the first
invalid enclosing-level/filter/handler triple; these are structural bounds,
not proof that every scope state is executed. The initial inventory-only pass missed two handler thunks and a catch
funclet after a nonreturning call. The raw-reference cross-check closes those
gaps; other, unrecognized compiler forms remain a census limit. The exported
call graph reaches 13 of these EH4 frame functions from CRT startup and
eight from WinMain; none of the five throw sites has a confirmed direct
path from those roots.

Every identified EH4 registration is listed below. In the scopes column,
`level: filter -> handler`, with `finally` for a null filter. `S`/`W` are
positive closure of the exported Ghidra call graph (`calls.tsv`) from CRT entry `0x012EFA14` / WinMain
`0x004079E0`; `-` means **unconfirmed**, not unreachable (virtual calls and
missing edges are not invented). Ghidra independently confirms the EH4
layout in `0x012F7240` and inline registration in `0x012F9AB0`.


| Frame function | Scope table | Scopes: filter -> handler | Direct roots |
|---|---|---|---|
| `0x012ED1F5` | `0x0161E8E8` | 0: finally -> `0x012ED26A` | - |
| `0x012ED430` | `0x0161E908` | 0: finally -> `0x012ED4B4` | - |
| `0x012ED66F` | `0x0161E928` | 0: finally -> `0x012ED6CA` | - |
| `0x012ED741` | `0x0161E948` | 0: finally -> `0x012ED7AA` | - |
| `0x012ED89C` | `0x0161E968` | 0: finally -> `0x012ED8D2` | S  |
| `0x012EDAF5` | `0x0161E988` | 0: finally -> `0x012EDC20` | S W |
| `0x012EF8A7` | `0x0161E9A8` | 0: `0x012EF9D6` -> `0x012EF9EA` | S  |
| `0x012F2BE8` | `0x0161E9C8` | 0: finally -> `0x012F2C9A` | - |
| `0x012F2DB2` | `0x0161E9E8` | 0: finally -> `0x012F2E83`; 1: finally -> `0x012F2E4F` | - |
| `0x012F4EBC` | `0x0161EA10` | 0: finally -> `0x012F4FBE` | - |
| `0x012F6C80` | `0x0161EA30` | 0: finally -> `0x012F6CF2` | - |
| `0x012F6D81` | `0x0161EA50` | 0: finally -> `0x012F6E61` | S W |
| `0x012F7AA8` | `0x0161EA70` | 0: finally -> `0x012F7B93` | - |
| `0x012F7ECD` | `0x0161EA90` | 0: finally -> `0x012F7F96` | - |
| `0x012F803D` | `0x0161EAB0` | 0: finally -> `0x012F80F6` | - |
| `0x012F914F` | `0x0161EAD0` | 0: finally -> `0x012F9208` | S W |
| `0x012F9244` | `0x0161EAF0` | 0: `0x012F9264` -> `0x012F9268` | - |
| `0x012F9290` | `0x0161EB10` | 0: `0x012F92B4` -> `0x012F92B8` | - |
| `0x012F933B` | `0x0161EB30` | 0: finally -> `0x012F949C` | - |
| `0x012F95B6` | `0x0161EB50` | 0: finally -> `0x012F9652`; 1: finally -> `0x012F9661` | S W |
| `0x012F96FD` | `0x0161EB78` | 0: finally -> `0x012F9814`; 1: finally -> `0x012F9820` | - |
| `0x012F9AB0` | `0x0161EBA0` | 0: `0x012F9B3B` -> `0x012F9B4E` | S  |
| `0x012FAD40` | `0x0161EBC0` | 0: finally -> `0x012FADD8` | S W |
| `0x012FB049` | `0x0161EBE0` | 0: finally -> `0x012FB1AA` | S  |
| `0x012FB4C1` | `0x0161EC00` | 0: finally -> `0x012FB52E` | S W |
| `0x012FC1DC` | `0x0161EC20` | 0: `0x012FC209` -> `0x012FC225` | S  |
| `0x012FD29D` | `0x0161EC40` | 0: finally -> `0x012FD304` | - |
| `0x012FE4ED` | `0x0161EC60` | 0: finally -> `0x012FE5A9`; 1: `0x012FE56B` -> `0x012FE575` | - |
| `0x012FE60E` | `0x0161EC88` | 0: `0x012FE651` -> `0x012FE65A` | - |
| `0x012FE74B` | `0x0161ED00` | 0: finally -> `0x012FE86B`; 1: `0x012FE7DB` -> `0x012FE7E4` | - |
| `0x012FE8E7` | `0x0161ED28` | 0: `0x012FEA52` -> `0x012FEA56` | - |
| `0x012FEA66` | `0x0161ED48` | 0: `0x012FEAEB` -> `0x012FEAEF` | - |
| `0x01302F75` | `0x0161EDA0` | 0: finally -> `0x013031F3` | - |
| `0x01303656` | `0x0161EDC0` | 0: finally -> `0x0130369C` | - |
| `0x013036A5` | `0x0161EDE0` | 0: finally -> `0x013036DD` | - |
| `0x01303A70` | `0x0161EE00` | 0: finally -> `0x01303B97` | - |
| `0x013043FF` | `0x0161EE20` | 0: finally -> `0x013044C8` | S W |
| `0x013044D3` | `0x0161EE40` | 0: finally -> `0x013045A1` | - |
| `0x013049D9` | `0x0161EE60` | 0: finally -> `0x01304A6C` | - |
| `0x01304FEE` | `0x0161EE80` | 0: finally -> `0x0130507F` | S W |
| `0x013050B4` | `0x0161EEA0` | 0: finally -> `0x01305244`; 1: finally -> `0x01305180` | - |
| `0x013081CE` | `0x0161EEC8` | 0: finally -> `0x0130825F` | - |

C++ handler `0x012EFAAB` forwards to `0x012FEFFF`. All seven FuncInfo
records have magic 0x19930522. Six have no try-map entries; the seventh
(`0x0161ECD8`) has one catch-all record at `0x0161ECA4`, entering
`0x012FE733` (the rethrow funclet of `0x012FE702`). Its TryBlockMap is
`0x0161ECC4`. The actual handler entry precedes `mov eax, FuncInfo`
(cookie validation); treating the mov as the entry misses the references.

| Handler thunk | FuncInfo | Max state | Try-map count | Functions installing frame |
|---|---|---:|---:|---|
| `0x0132A423` | `0x0161E700` | 2 | 0 | `0x00CEE940` |
| `0x0132A44F` | `0x0161E72C` | 1 | 0 | `0x00CEEBC0` |
| `0x0132A478` | `0x0161E758` | 1 | 0 | `0x00CEE9F0`, `0x00CEECE0` |
| `0x0132A4A8` | `0x0161E784` | 1 | 0 | `0x00CEEF70` |
| `0x0132A4D8` | `0x0161E7B0` | 1 | 0 | `0x00CEF170` |
| `0x0132A50F` | `0x0161E7DC` | 1 | 0 | `0x00CEF430` |
| `0x0132A52A` | `0x0161ECD8` | 2 | 1 | `0x012FE702` |

Four `_CxxThrowException` (`0x012F530B`) sites are confirmed by Ghidra
`get_xrefs_to`: `0x012E1A3D` in `0x012E1A13`, `0x012E1A8A` in
`0x012E1A60`, `0x012E1AF4` in `0x012E1AD4`, `0x012FEDE1` in
`FindHandler` (`0x012FEC6D`). The fifth, `0x012FE745` in catch funclet
`0x012FE733`, is validated from the original instructions and the above
catch map; Ghidra does not report that xref. Direct startup/WinMain reachability was not
established for these. They are not dead code: Ghidra finds, for example,
`0x0040DBB4`, `0x00DB83C1`, `0x00DB142C`, `0x00D0E94D` calling
`0x012E1A13`. Main-loop indirect reachability remains unconfirmed.

| Import IAT | API | Ghidra instruction references |
|---|---|---|
| `0x0139D124` | RaiseException | `0x012F534D`, `0x012FBB1E`, `0x0130A38B`, `0x0130A443`, `0x0130A51B` |
| `0x0139D12C` | RtlUnwind | thunk `0x0130A30C`, called at `0x012EFA7D`, `0x01303CE1`, `0x01306FE3` |
| `0x0139D138` | SetUnhandledExceptionFilter | `0x012EDDA0`, `0x012FF20C`, `0x012FDDDA` |
| `0x0139D13C` | UnhandledExceptionFilter | `0x012EDDAD`, `0x012FF217` |
| `0x0139D26C` | IsDebuggerPresent | `0x012EDD96`, `0x012FF1F7`, `0x00FA9790` |

No AddVectoredExceptionHandler/RemoveVectoredExceptionHandler import.
Dynamic lookup/use was not proven absent. No vectored implementation was
added without a caller. IsDebuggerPresent retains the existing no-debugger
contract. CRT startup has its own EH4 frame (`0x012EF8A7`, table
`0x0161E9A8`) and the startup-reachable image probe (`0x012F9AB0`) has
one. Thus dispatch cannot be justified away as only a rare gameplay path.

Independent handler-list cross-check: load config `0x0161E1F8` declares a
72-byte structure although its PE directory size is 64. The SafeSEH table
at `0x0161E6B0` has twelve entries: the seven C++ thunks above,
`0x012F7240`, `CatchGuardHandler` `0x012EFAE1`, `TranslatorGuardHandler`
`0x012EFBEB`, and unwind guards `0x01303C40` / `0x01306FF0`. The latter
two return disposition 3 on their unwind paths (`0x01303C80` /
`0x0130702F`); these are concrete users of the dispatcher-context collided
unwind result now implemented. The two C++ guards forward to `0x012FEFFF` with the existing
FuncInfo carried in their registration. No additional language-handler
family is present in the SafeSEH table.

### Implementation and limits

- Per-thread NT_TIB/TEB pages initialize fs:[0], StackBase, StackLimit and
  Self for Session and CreateThread; worker TEBs do not share chains.
- `seh.rs` writes 80-byte EXCEPTION_RECORDs and 716-byte x86 CONTEXTs on
  the guest stack, calls the original handlers using cdecl, checks frame
  alignment/bounds/forward progress and preserves the interrupted Cpu.
- ContinueSearch, ContinueExecution and NestedException dispositions;
  explicit protective registrations model nested dispatch even when the
  handler registers a newer guest handler. A nested software RaiseException
  has a null ExceptionRecord link (measured), not an invented link to the
  outer record. Explicit Exception.nested is available for linked records.
- Target-frame RtlUnwind visits younger registrations, sets UNWINDING,
  removes the registrations and excludes the target. It initializes context
  EAX=ReturnValue; a handler's modification takes effect on return (Windows
  oracle sets 0xBEEF and observes it afterward).
  The x86 TargetIp parameter is ignored: tested with 0x12345678, not just
  NULL. A returned CollidedUnwind disposition advances past the completed
  registration named by the dispatcher context, with bounds/progress checks
  (Windows comparison case 5). Exit unwind, and an exception or recursive
  unwind raised *inside* an unwind callback, remain precise unsupported
  stops; the latter needs a nonlocal transfer between active unwinders.
- RaiseException is bound to dispatch; the registered unhandled filter is
  called with EXCEPTION_POINTERS, also through the explicit imported
  UnhandledExceptionFilter API. No guest code is replaced with a hand port.
- CONTEXT advertises CONTROL and INTEGER only. Segment selector low words
  0x23/0x2b were measured under Windows x86/WoW64. Floating/extended/debug
  context groups are not advertised as fake zero state. Undefined arithmetic
  flags without a recorded value stop explicitly. Handler memory views reject
  reads of unprovided context groups (including whole-context copies), rather
  than exposing zero-filled floating/debug state. DF is preserved in the
  saved context but cleared for handler execution, as measured on Windows.
- Software raises snapshot the caller-side API-return boundary (including
  stdcall cleanup). This is an explicit runtime contract, **not** a claim
  that the kernel-internal RaiseException EIP/volatile registers match the
  host DLL. A continued same-EIP/same-ESP raise resumes its Rust caller with
  the modified general registers. Other continuations report the exact
  requested EIP/ESP; no manufactured function call is substituted for a jump.
- Invalid dispositions/noncontinuable resumes stop; secondary Windows
  status-exception dispatch for these errors is still missing. Stack
  validation failures also stop rather than pretending to handle them.
  SafeSEH image-table and SEHOP validation are not implemented; the current
  checks cover stack bounds, alignment, chain progress and handler return ABI.
- The AV constructor produces 0xC0000005 and read/write/address parameters
  when supplied an exact instruction checkpoint. Automatic lifted memory
  and integer-fault delivery is **not wired**: generated code has no EIP
  checkpoint. Memory retains its checked fault reports; no guessed conversion
  is attempted. DebugBreak uses the same software API-boundary contract.

### Required follow-up for head to route

Two C++ handler thunks are absent from the inventoried functions:
`0x0132A44F` (installed at `0x00CEEBC2`) and `0x0132A52A` (loaded at
`0x012FE704`). The census tool recovers them without modifying the inventory.
Route these and catch funclet `0x012FE733` to the lifter/inventory lane.

At inspection, the shared lifted list contains `0x012F7240`,
`0x012EFAAB`, `0x012FEFFF`, `0x012F530B`; it lacks prolog/epilog
`0x012F71E0` / `0x012F7225` (claude-3's lane).

Entry availability matters independently of dispatch: **0/10 filters** and
**19/48 scope handler references** are in the shared lifted-entry list at
inspection. The ten missing filter entries are `0x012EF9D6`, `0x012F9264`,
`0x012F92B4`, `0x012F9B3B`, `0x012FC209`, `0x012FE56B`, `0x012FE651`,
`0x012FE7DB`, `0x012FEA52`, `0x012FEAEB`. For example, the startup image
probe's filter is `0x012F9B3B` and handler is `0x012F9B4E`. These use the
establisher's EBP via `_EH4_CallFilterFunc`, not a normal standalone C
function fixture. Route these contextual entries to the lifter lane; the
scope table above identifies the remaining handler entry addresses.

The board assigns `research/native/lift.py` to claude-6 and
`crates/ddnative-base/src/x86.rs` to claude-9. Neither was edited here.
The continuation contract needs:

1. An instruction EIP checkpoint **before** any faulting operation, plus
   original pre-instruction state where emission partially changes Cpu
   before a memory access (push/pop, multi-access instructions).
2. A generated continuation resolver for *instruction* addresses and
   return addresses, not only function entries or basic-block leaders.
   `rust_fn` initializes a local pc to the function entry; straight-line
   functions have no pc at all. Calling a lifted entry cannot safely
   implement an arbitrary resumed jump or preserve the Rust call stack.
3. A nonlocal transfer mechanism discarding intervening Rust guest calls,
   then continuing at the selected instruction with handler-modified Cpu;
   normal and callee-cleanup returns must retain their guest return EIP.
4. Typed divide/overflow and memory faults with that checkpoint, before
   dispatching 0xC0000094 / 0xC0000005. Do not classify arbitrary Rust
   panics as guest exceptions.

Concrete interior continuation needed: `_EH4_TransferToHandler`
`0x01303CB9` ends in `jmp esi` at `0x01303CD0`, after changing EBP and
clearing volatile registers. `0x012F7240` reaches it after global/local
unwind. The scope handler addresses above are not all lifted function
entries. This is a real blocker to complete original __except execution,
not an unexercised optional API.

### Verification

`cargo test -p ddruntime --tests` passes: **70 passed, 0 failed, 3 ignored**
(existing install-dependent input/audio/movie tests). Run through
`E:/ddda-re/tools/pyenv/Scripts/python.exe E:/ddda-re/tools/heavy.py codex --`
with `CARGO_TARGET_DIR=C:/ddda-targets/L27`, `CARGO_INCREMENTAL=0` and the
helper's job cap. All **14 SEH regressions** pass: Windows layout/order,
nested flags/linkage and new-handler ordering, target and returned-collided
unwind, modified integer/DF context, exact supplied DIV/AV checkpoint records,
software API-boundary continuation, unhandled filter, per-thread TEB,
stack/disposition/ABI failures, unsupported resume and unavailable-context
guards. One regression executes the actual lifted `_except_handler4`
`0x012F7240`, its cookie validation and filter-call helper against a synthetic
scope table/filter; it confirms the establisher-frame callback contract.

The native Windows oracle compiled and
ran with MSVC x86 /Od /EHsc: compiler __try/divide/AV-resume tests with /SAFESEH,
and raw registration order/nesting/unwind tests with /SAFESEH:NO (C handler
has no assembler .safeseh declaration). `research/runtime/seh_oracle.ps1`
reproduces both and asserts the layout, thirteen handler invocations, nested
flags/linkage, modified unwind EAX and AV parameters. All comparisons pass,
with all objects/executables/logs outside the repository.
Rust tests use equivalent authored handler decisions; they do **not** lift
and execute the compiled C oracle bytes, and no end-to-end game EH claim
is made. Logs are under E:/ddda-re/notes/runtime-L27.

References: Windows SDK winnt.h/excpt.h, the executable addresses above,
[RaiseException](https://learn.microsoft.com/en-us/windows/win32/api/errhandlingapi/nf-errhandlingapi-raiseexception),
[RtlUnwind](https://learn.microsoft.com/en-us/windows/win32/api/winnt/nf-winnt-rtlunwind),
and [exception dispatch](https://learn.microsoft.com/en-us/windows/win32/debug/exception-dispatching).
