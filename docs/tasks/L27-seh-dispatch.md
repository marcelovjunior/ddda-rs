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

(open)
