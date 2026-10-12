# L27b: instruction faults and resumption for SEH (the lifter/Cpu part)

- **Owner:** Codex
- **Branch:** `task/L27b-seh-faults` (worktree `E:\ddda-rs-wt\L27b`, from
  task/L27-seh merged with main)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md), merge-train rule. Codex
  cannot receive messages, so coordinate through the board
  (`scripts/board.py`).

## Why

L27 delivers software-raised exceptions: RaiseException, the guest chain
search, nesting, unwind, filters, and a per-thread TEB. Its Results say
what is still missing: faults raised by an instruction inside lifted code
(access violation, integer divide by zero, the ones the memory model and
lifter can detect), and resumption at a non-local or interior address
(ExceptionContinueExecution with a changed CONTEXT, or `__except` blocks
in the middle of a function). Both need lifter and Cpu changes.

## Task

1. **Implement the contract** documented in L27's Results:
   - Fault detection in the lifted code, at the instruction level: a
     faulting access and a divide by zero leave Rust with an exact CONTEXT.
   - Re-entry into a lifted function at an interior address (`__except`
     handler bodies, resumption targets): through the lifter's existing
     block structure, or a generated dispatcher per function. Use a
     general rule.
2. **Coordinate on the board:**
   - claude-6 owns research/native/lift.py (it also adds MXCSR and the SSE
     oracle there);
   - claude-9 owns ddnative-base's x86.rs/Cpu and mem.rs (the page table
     and fast paths).
   Post your design before editing their files.
3. **Tests against your Windows oracle** (the same programs as L27): the
   guarded access violation, divide by zero, resume with a modified
   CONTEXT, and `__except` bodies. All must match Windows.
4. **Prove nothing else changed:** the required-vector replay in the
   integration train.

## Results

### Batch 1: instruction transaction and flat executor (2026-10-12)

**Historical first handoff (`f0d9e0f`): runtime contract only.** New
`crates/ddruntime/src/executor.rs` executes registered instruction callbacks,
delivers their AV/#DE traps through L27's dispatcher, and follows the
handler-selected EIP and Cpu. It does not yet execute the production lifted
program: no changes were made to `lift.py`, `x86.rs`, `mem.rs`, the existing
Registry or Session routing. There is no end-to-end original `__except`
execution claim for this batch.

The design was posted to claude-6, claude-9 and head before any shared-file
edits (board #9437, clarified #9441). No reply to those requests had arrived
at this batch's handoff. The dispatch.rs claim overlaps claude-4's L36
claim; coordination request #9438 is pending. Work therefore stayed in a
new runtime module and its tests, using existing `GuestMemory::check`.

Implemented:

- `Instruction = fn(&mut dyn Mem, &mut Cpu) -> Flow`: one restartable
  instruction per callback, with an explicit instruction address supplied
  by the executor. CALL pushes the guest return address and selects a
  successor. RET reads that address, applies its cleanup and returns
  `Flow::Return(target)`. The executor keeps no Rust guest caller stack.
- The boundary snapshots the complete Cpu before executing an instruction.
  All writes are permission-checked and buffered; success commits them,
  while a typed guest trap restores Cpu and discards pending writes. Reads
  see earlier buffered writes. Failed PUSH/multi-store operations therefore
  cannot leave a changed ESP, flags, XMM/x87 state or partial memory writes.
- The access trap identifies the first inaccessible byte, including split
  pages; ordinary GuestMemory reports identify the whole access's start.
  This distinction is handled locally without changing the shared memory
  implementation. Typed divide faults cover zero divisors and quotient
  overflow (`0xC0000094`). Arbitrary Rust panics are rethrown unchanged.
- Known interior instructions are ordinary lookup keys. Modified CONTEXT
  can retry the same instruction, skip it, or restore an outer stack and
  enter a nonlocal body. Missing instructions stop with their exact address;
  there is no whole-function fallback. The root return token is explicit.
- REP's producer contract requires one iteration per boundary. A regression
  confirms that a later fault preserves completed iterations and presents
  the faulting iteration's ECX/EDI, then resumes that iteration correctly.

Restrictions are deliberate: a callback must not invoke services, allocate,
change mappings, release the guest lock or publish side effects outside
Mem/Cpu. Mappings must stay stable through preflight and commit. Callbacks
that violate this contract are not supported instruction producers. The
current executor has an explicit instruction budget and reports exhaustion.

### Windows comparison and cheap verification

`research/runtime/seh_oracle.ps1 -Output E:/ddda-re/notes/runtime-L27b`
passes in both x86 MSVC configurations (`/SAFESEH` compiler scopes and
`/SAFESEH:NO` raw registrations). Added observations:

- A dword store spanning writable memory and PAGE_NOACCESS reports the
  inaccessible second page in ExceptionInformation[1], and leaves the
  accessible two-byte prefix unchanged (`0x1234`).
- A guarded AV two calls below `__try` reaches the outer `__except` body;
  neither abandoned caller continuation executes.
- Existing DIV/AV modified-CONTEXT comparisons still pass: DIV reports
  exact fault EIP, EAX=1/ECX=0/EDX=0, saved DF=1 versus handler DF=0, and
  resumes with EAX=42 at the changed EIP.

`cargo test -p ddruntime --no-default-features`: **94 passed, 0 failed**,
including **9 new executor tests**. CARGO_INCREMENTAL=0,
CARGO_TARGET_DIR=C:/ddda-targets/L27b, CARGO_BUILD_JOBS=4; no heavy slot.
The original `_except_handler4` regression from L27 is now correctly gated
with `#[cfg(feature = "game")]`: the first light run exposed that missing
feature gate. Its assertions and game-enabled behavior are unchanged.

The Rust instruction/handler callbacks are authored fixtures equivalent to
the oracle decisions, not lifts of the compiled oracle bytes. The nonlocal
Rust test changes CONTEXT to an outer body; it does **not** claim to test
the original CRT's `_EH4_TransferToHandler` or actual `__except` unwinding.
Private oracle binaries/logs and `light-tests.log` remain under
E:/ddda-re/notes/runtime-L27b. No shared data was changed.

### Batch 2: production producer and runtime routing (2026-10-12)

**Implementation complete for the assigned fault/continuation contract;
light checks pass. Full generated-game verification belongs to train 3.**
This supersedes batch 1's producer/Session/callback limitations, not its
CONTROL/INTEGER-only CONTEXT limits or the census's missing code coverage.

- Posted exact shared-file plan #9456 before editing; recorded the direct
  head follow-up assignment as authorization for the narrow conflicting
  claims (#9460). No Cpu layout, MXCSR, SSE oracle or memory fast-path edits.
- `ddnative_base::execution` owns `Flow`, the instruction resolver ABI and
  typed divide trap. `Mem::probe` is an object-safe, non-mutating permission
  check, returning the first inaccessible byte. Its default explicitly
  refuses instruction execution; GuestMemory and the SEH ContextMem adapter
  implement/forward it. Legacy vector memories and fast lifts are unchanged.
- `lift(..., step=True)` uses the existing instruction semantics with every
  instruction as an entry. CALL evaluates its target before pushing; JMP
  and taken conditional branches return explicit jumps; RET reads the actual
  guest return address and applies its immediate cleanup. REP emits one
  iteration, including ECX=0's no-access case. DIV/IDIV use the typed trap.
  No host call, service or platform model runs inside an instruction.
- Production `gen` emits both the existing vector/fast functions and these
  alternate step functions, plus exact-address sorted lookup tables. The
  facade checks all shards because discovered bodies can cross address
  bands/share epilogues. Mid-instruction addresses never match. Body caches
  are released per function; entry indexing avoids quadratic leader lookup.
  Generation errors identify the owner address and fail, without fallback.
- `Registry::with_lifted_program` installs that resolver, so Session, guest
  threads and `call_ext` callbacks enter the flat executor. Services retain
  precedence and run outside transactions. A missing generated instruction
  stops with its exact IP; a bound fast lift is not a fallback. Session
  renders the executor's typed stop in its fault report.
- Active host callback boundaries track both return address and return
  **slot**, distinguishing nested uses of the same callback sentinel.
  Modified software-raise/unwind contexts and restored outer-stack branch
  targets propagate a typed Cpu/continuation payload; abandoned services,
  ABI checks and guest continuations do not run. Return-address POP/PUSH
  and naked POP/JMP epilogues are supported. Transient ESP arithmetic alone
  does not abandon a boundary. This implements the runtime's existing
  downward-growing Windows x86 callback-stack ABI, not fiber/alternate-stack
  switching. Arbitrary Rust panics are still host failures.

### Batch 2 verification

- `cargo test -p ddruntime --no-default-features`: **94 passed, 0 failed**.
- `cargo test -p ddnative-base`: **7 passed, 0 failed**.
- `PY -m unittest discover -s research/native -p 'test_lift_*.py'`:
  **52 passed** (includes a four-function synthetic shard workspace, not the
  generated game). Its temporary builds now stay on C: with incremental off.
- `PY -m unittest discover -s research/native -p test_steps.py`: **2 Python
  tests pass**, including **12 Rust execution tests**. Synthetic bytes go
  through the production emitter, then a real Registry/Session/SEH. Covers
  exact Windows DIV context/modified EIP, AV retry, split-page atomicity,
  REP restart, operand-before-push, cdecl emitted handlers, software raises,
  two nested service callbacks, nonreturning handler jumps, normal callbacks
  and naked epilogues. Generator checks cover exact/cross-shard keys and
  unchanged-file timestamps.
- With the private exe available, that last suite also emits and executes
  the **original** `_EH4_TransferToHandler` (`0x01303CB9`, final jump
  `0x01303CD0`) and its notification callee `0x013070E5`. After runtime
  RtlUnwind, they reach an authored outer scope body; notification memory
  (`0x0163ACD4/8/C`), restored stack, registration chain and discarded host
  continuations are checked. Neither original body is mocked. Private
  source is generated temporarily on C: and deleted, never committed.
  This is not a claim that the entire original `_except_handler4` search
  and every real scope body have been exercised end to end.
- Windows x86 oracle SAFESEH/raw configurations pass again, including saved
  versus handler DF, modified DIV/AV context, split-page atomicity and
  nested-call nonlocal handling. The Rust tests compare those observations;
  they do not claim instruction-for-instruction Windows dispatcher equivalence.
- Read-only generation audit also succeeds for original `__except_handler4`
  (`0x012F7240`: 131 instructions), `__SEH_prolog4` (`0x012F71E0`: 21),
  `__SEH_epilog4` (`0x012F7225`: 11), and the transfer helper (11).

Logs: `E:/ddda-re/notes/runtime-L27b/{final-light-tests,base-tests,
python-tests,emitted-tests,oracle-final}.log`; audit
`original-step-audit.json`. No shared data, generated game code, native gate
or full required-vector replay was changed/run. All builds use
CARGO_INCREMENTAL=0 and CARGO_TARGET_DIR=C:/ddda-targets/L27b.

### Train handoff and remaining coverage

1. Merge with claude-6's L22 emitter/MXCSR lines and claude-9's memory work,
   regenerate with the research Python (gen now needs Capstone, the private
   image and inventory), then run the head's required-vector replay and
   game-enabled runtime tests. Raw/fast replay remains intact for comparison;
   production Session uses the step path. Whole-game size, compile memory
   and runtime performance of the extra instruction path remain unmeasured.
2. A read-only audit of the shared selected-list snapshot against L27's
   census found **58 distinct filter/body addresses: 19 cached instruction
   keys present, 39 absent**. These are data/lifter-lane gaps, not permissive
   resume fallbacks. Sent #9468 to claude-6/claude-3/head; private machine
   list: `E:/ddda-re/notes/runtime-L27b/scope-coverage.json`. The absent keys:

   `0x012ED7AA 0x012EF9D6 0x012EF9EA 0x012F2E4F 0x012F7B93 0x012F7F96
   0x012F80F6 0x012F9264 0x012F9268 0x012F92B4 0x012F92B8 0x012F949C
   0x012F9652 0x012F9814 0x012F9820 0x012F9B3B 0x012F9B4E 0x012FADD8
   0x012FC209 0x012FC225 0x012FE56B 0x012FE575 0x012FE5A9 0x012FE651
   0x012FE65A 0x012FE7DB 0x012FE7E4 0x012FE86B 0x012FEA52 0x012FEA56
   0x012FEAEB 0x012FEAEF 0x013031F3 0x01303B97 0x013044C8 0x013045A1
   0x0130507F 0x01305180 0x0130825F`.

3. Existing explicit limits remain: unadvertised floating/debug CONTEXT
   groups and unknown arithmetic flags stop; guest fiber/alternate stacks,
   full secondary-exception dispatch and SafeSEH/SEHOP validation are not
   added here. Faulting accesses inside Rust services have no fabricated
   instruction EIP; only instruction producers deliver hardware faults.
