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

**Light-checked runtime contract; L27b is not complete.** New
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

### Next integration batch / train obligations

1. Agree the cross-crate producer ABI with claude-6/claude-9 (the current
   Flow/Instruction types live in ddruntime; generated shards cannot depend
   on it). Move the agreed small contract to ddnative-base or expose an
   equivalent callback bridge. Preserve their MXCSR/page-table changes.
2. Add the alternate generated per-instruction path and resolver, including
   straight-line functions, instruction/return addresses, indirect branches,
   actual guest RET targets, and one REP iteration per checkpoint. Division
   must use the typed trap helper; ordinary whole-function lifts are not
   acceptable instruction callbacks. Keep the existing path for differential
   comparison until the train validates the new producer.
3. Integrate Registry/Session and service/callback boundaries. Handle typed
   nonlocal transfer out of active Rust service/handler callbacks, including
   `_EH4_TransferToHandler` at `0x01303CB9` / jump `0x01303CD0` from L27's
   census. The flat guest loop solves its own guest call stack, not these
   still-active host callback stacks. Integrate the census's missing filter
   and scope-body entries; do not manufacture function calls for jumps.
4. L27 CONTEXT limits remain: CONTROL/INTEGER only, unknown arithmetic
   flags and unavailable floating/debug groups stop. Capture/export any
   further hardware state required by actual handlers without inventing it.
5. Run emitted oracle instruction fixtures and original handler bodies,
   then **required-vector replay and game runtime tests in the head's
   integration train**. Neither was run on this branch, per batched
   verification. Production game instruction faults still follow the old
   path until these integration steps land.
